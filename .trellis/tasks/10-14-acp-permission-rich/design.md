# Design: 权限授权富化（changes / diff / 队列）

需求 `prd.md`，codeg 事实 `research/codeg-permission.md`。桥接规范
`.trellis/spec/frontend/tauri-bridge.md`（ACP 段）。现状代码：
`src-tauri/src/acp.rs`（Rust）、`src/composables/useAcpPermission.ts`、
`src/views/coding/components/PermissionCard.vue`、`src/types/acp.ts`。

## 1. 数据流现状与缺口

```
agent ──session/request_permission{params:{sessionId, toolCall, options[], _meta?}}──▶ Rust
Rust handle_client_request(:338)
  ├ 存 conn.pending_perm[requestId] = (rpc_id, sessionId)
  └ emit acp://permission {connId, requestId, sessionId, toolCall, options}   ← _meta 被丢
前端 acpService listen acp://permission → dispatchPermission → useAcpPermission
  → 队列(confirm) → ConversationView 渲染 PermissionCard
```

- **`options` 已原样透传**（`params.options.cloned()`），故选项级 `_meta.permission.changes`
  **已在 events payload 里**，只是 types 未声明、前端未读。
- **请求级 `params._meta` 被丢**：事件未带 `meta`。
- **`rawInput` 原样透传**，但无 `_start_line` 注入 → 前端需自行解析补丁。

## 2. Rust 改动（最小面，`src-tauri/src/acp.rs`）

1. **请求级 `_meta` 透传**：`acp://permission` 事件加 `"meta": params.get("_meta").cloned()`
   （缺失 → `Value::Null`）。无新命令、无结构变更。
2. **`rawInput` 行号注入（best-effort）**：在 emit 前，对 `toolCall.rawInput`：
   - 若是 object：注入 `_start_line`（对 `file_text`/patch 字段，参照 codeg `inject_start_line`）。
   - 若是 string 且含 `@@`（patch）：尝试解析为对象（含 `_start_line`），失败保持原串。
   - 全程 best-effort：任何失败都**保持原值**，不阻断事件。
   > 目的仅为前端 diff 视图能定位行号；**不 host 任何 fs/terminal**（D2）。

不改 `pending_perm` 结构、不加新命令、不改 `respond_permission_inner`。
→ opencode 零回归（无 `_meta` 时 meta=null、rawInput 不变）。

## 3. 队列深度（D3 取舍：不动 Rust）

前端 `useAcpPermission` 已持有**该连接的全部**待审批请求（`queue: AcpPermissionEvent[]`，按
connId 过滤）。「还有 N 个待审批」= 同会话队列中除当前卡外的条数。**数据源真实且完整**
（Rust 不会丢弃请求——它逐个 emit，前端逐个入队）。

→ **R5 采用前端计数**，Rust 不重构队列。理由：codeg 的 Rust 队列是为其「单槽快照 + 任务
取消」语义服务（`SessionState.pending_permission` 单槽）；本项目前端本就是多卡队列，
`queue.length - 1` 即准确值，无需引入 Rust 侧队列带来的并发风险。
（若将来需要「后台/冷附加恢复」的 pending 快照，再单列任务。）

`PermissionCard` 增 `queuedCount` prop（由 `ConversationView` 计算传入）。

## 4. 前端改动

### 4.1 类型（`src/types/acp.ts`）
```ts
export interface AcpPermissionOption {
  optionId: string
  name?: string
  kind?: 'allow_once' | 'allow_always' | 'reject_once' | 'reject_always' | string
  meta?: unknown          // 新增：选项级 _meta（内含 permission.changes）
}
export interface AcpPermissionEvent {
  connId: string; requestId: string; sessionId: string
  toolCall: AcpToolCallUpdate
  options: AcpPermissionOption[]
  meta?: unknown          // 新增：请求级 _meta
}
```

### 4.2 解析工具（新 `src/utils/permissionMeta.ts`，纯函数，可测）
- `parsePermissionTitle(event) -> string`：`meta.claudeCode.title ?? meta.permission.title ??
  toolCall.title ?? toolCall.toolCallId`。
- `parsePermissionDescription(event) -> string | null`：`meta.permission.description`。
- `isDefaultToNo(event) -> boolean`：`meta.permission.defaultToNo === true`。
- `parseOptionChanges(option) -> { description: string; scope: 'session'|'persistent'|null }[]`
  （仿 codeg `parsePermissionOptionChanges`；支持 `changes[]` 与单 `description` 两形态；
  上限条数/字符）。
- `extractDiff(rawInput) -> string | null`：从 `rawInput`（object 的 patch/file_text 或
  string 补丁）取 diff 文本。

### 4.3 组件（`PermissionCard.vue`）
- 头部：`ShieldAlert` + `parsePermissionTitle` + 理由（`parsePermissionDescription`）；
  右侧 `queuedCount > 0` 徽章「还有 N 个待审批」+ kind 徽章。
- 身体区块（按存在性渲染）：
  1. **命令**：`toolCall.rawInput.command`（若有）→ `CodeBlock` 风格。
  2. **diff**：`extractDiff(rawInput)` → 统一 diff 视图（见 4.4）；无则回退 JSON 摘要。
  3. **选项授予内容**（`parseOptionChanges`）：按选项分组列 changes + scope 徽章。
- 按钮（D4 动态）：遍历 `options`；文案 `option.name ?? KIND_LABEL[option.kind]`；
  `isDefaultToNo` 时强调色给 reject 类；点击回 `option.optionId`。
- 保留既有「允许一次/本会话总是允许/拒绝」作为**无 options / 无 meta 时的回退**（AC7 零回归）。

### 4.4 diff 视图
- 复用项目已有能力：检查是否已有统一 diff 组件（`src/components` 或 `utils`）。若无可视
  组件，则最小实现 `src/views/coding/components/DiffPreview.vue`：按行渲染
  `+`/`-`/` `（green/red/neutral），或直接用 `<pre>` 着色。**不引入新依赖**（除非已有）。

### 4.5 队列计数
- `ConversationView.vue`：`permissionQueue` 已知；给每张卡传
  `:queued-count="permissionQueue.length - 1"`（本期一次渲染多卡时近似；若 View 改为只渲染首卡，
  则按「队列长度-1」同理）。

### 4.6 mock（`src/services/acpMock.ts`）
- 现有 mock 权限请求加 `meta`（含 `permission.changes`、`defaultToNo` 之一）与带补丁的
  `rawInput`，使浏览器 dev 可见富卡。

## 5. 兼容 / 回滚

- 回滚 = 回退 `PermissionCard.vue`、`types/acp.ts`、删除 `permissionMeta.ts`；Rust 的
  `meta` 字段与 `_start_line` 注入可保留（无 `_meta`/无补丁时为 no-op）。
- **opencode 零回归**：opencode 权限请求不带 `_meta` → 富区块全不出现；按钮回退旧三态。
- **无新依赖**；不 host fs/terminal；不动 `pending_perm`。

## 6. 测试要点

- 单测（`permissionMeta.ts`，vitest 若可用；否则以脚本/手测）：title 回退链、changes 两形态、
  defaultToNo、extractDiff（object/string/无）。
- Rust：`cargo test --lib` 现有权限相关测试保持绿；新增 `_start_line` 注入纯函数单测
  （若抽为纯函数）。
- 前端门禁：`vue-tsc` + `build`。
- 实机（人工）：codex Bash/edit 审批卡展示 changes/diff；opencode 卡与旧版一致。
