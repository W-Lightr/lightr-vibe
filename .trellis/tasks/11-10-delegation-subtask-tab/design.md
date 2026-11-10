# Design：子任务统一委派与右侧栏子任务页签

## 1. 现状

| 位置 | 现状 |
|---|---|
| `src-tauri/src/bin/lightr_ask_mcp.rs:165+` | `delegate_to_agent` 描述中性，只说「把子任务交给本地子代理」 |
| `src/views/coding/components/RightPanel.vue` | 本地 `tab` ref，页签固定为 文件 / 变更 / 详情 |
| `src/views/coding/CodingWorkspace.vue:246-261` | `delegatedChildSession` 非空时**整块替换**右栏（子代理抽屉），否则渲染 `RightPanel` |
| `src/stores/workspace.ts:30-66` | `inspectorVisible`、`delegatedChildSessionId`、`openDelegatedChild` / `closeDelegatedChild` |
| `src/main.ts:55-82` | `delegation://started` → `projectDelegation(record, true)` → `openDelegatedChild(child.id)` |
| `src/stores/session.ts:459+` | `ensureDelegationCard()` 按 record 在父会话 transcript 插入 `delegate_to_agent` 工具卡 |
| `src/views/coding/components/DelegationCard.vue` | transcript 内卡片；状态文案 / 配色目前内联在这里 |
| `src/stores/delegation.ts` | 全量 `records`，无按父会话的派生视图 |

## 2. 设计决策

### D1 · 工具描述引导（Rust，单点）

只改 `src-tauri/src/bin/lightr_ask_mcp.rs` 中 `delegation_tool_defs()`：

- `delegate_to_agent.description`：追加「本应用内子任务的唯一受支持入口；不要使用引擎自带的
  子代理 / task 工具，否则用户无法看到子任务进度面板」。
- `get_delegation_status` / `cancel_delegation.description`：注明操作对象是
  `delegate_to_agent` 创建的子任务。

不改 `inputSchema`、不改 socket 协议、不改引擎启动参数。描述文本属于 wire 内容但不参与解析，
不存在兼容问题。

**Claude 会话的系统提示追加。** 实测表明 Claude Code 适配器把 4 个 MCP 工具都注册进了
`tools` 列表（transcript 的 init 消息可见 `mcp__lightr-ask__delegate_to_agent`），但模型仍调用
原生 `Agent` / `SubagentHandback`。因此对 `AgentType::ClaudeCode` 的
`session/new` / `session/load` 参数补：

```json
"_meta": { "systemPrompt": { "append": "<DELEGATION_SYSTEM_HINT>" } }
```

- `@agentclientprotocol/claude-agent-acp` 会把 `append` 并入 Claude Code 预置系统提示；
  已用直接探针验证 `session/new` 接受该字段并成功建会话。
- 按引擎门控（其它引擎不认这个私有键）+ 委派开关门控（关闭时工具不存在，提示会指向空工具）。
- 子代理会话（`run_delegated_child`）同样注入，保持行为一致；深度限制仍由 broker 拦截。
- 本期**不**注入 `_meta.claudeCode.options.disallowedTools`，保留原生 Agent 工具（用户选择软引导）。

### D2 · 页签类型与状态提升

新增共享类型（放在 `src/stores/workspace.ts` 导出，避免为单一枚举新建文件）：

```ts
export type RightTab = 'files' | 'changes' | 'detail' | 'subtasks'
```

`useWorkspaceStore` 新增：

```ts
const rightTab = ref<RightTab>('files')

function openDelegatedChild(sessionId: string) {
  delegatedChildSessionId.value = sessionId
  rightTab.value = 'subtasks'
  inspectorVisible.value = true
}

function closeDelegatedChild() { delegatedChildSessionId.value = null }        // 详情 → 列表
function closeInspector() { delegatedChildSessionId.value = null; inspectorVisible.value = false }
```

`openDelegatedChild` 的既有调用方（`main.ts`、`DelegationCard.openChild`）签名不变，
自动获得「切到子任务页签并展开右栏」的行为。

### D3 · 右栏结构

`CodingWorkspace.vue` 的子代理抽屉整块删除，改为把 `childSession` 交给 `RightPanel`：

```
RightPanel
  props: { session: Session | null }
  computed:
    childSession = sessionStore.get(workspace.delegatedChildSessionId)
    hasSubtasks  = session && delegation.records.some(r => r.parentSessionId === session.id)
    tabs         = 固定三项 + (hasSubtasks || childSession ? 子任务项 : 无)
  watch:
    tab === 'subtasks' && !hasSubtasks && !childSession → tab = 'files'
  内容:
    childSession ? <DelegatedChildView :session="childSession" />
                 : <DelegationTaskList :parent-session-id="session.id" />
```

- 页签数量徽标取 `hasSubtasks` 的计数；详情态下计数仍按当前活动会话（R6）。
- `tab` 从本地 ref 改为 `workspace.rightTab` 的 `v-model` 代理；TopBar 的显隐按钮只动
  `inspectorVisible`，无需改动。

### D4 · 子任务列表

新组件 `src/views/coding/components/DelegationTaskList.vue`：

- props：`parentSessionId: string`
- 数据：`delegation.records.filter(r => r.parentSessionId === parentSessionId)`，
  按 `createdAt` 倒序；空态文案「当前会话还没有子任务」。
- 每项渲染 `DelegationTaskCard.vue`。

新组件 `DelegationTaskCard.vue`（展示型，不复用 transcript 里的 `DelegationCard`，
后者依赖 `ToolCallPartData`）：

- 基础信息：状态徽标、引擎标签（`agentType`）、任务单行摘要（`title` 全文 + 截断）、
  耗时 `formatDuration(updatedAt - createdAt)`、短 id `#taskId.slice(0, 8)`。
- 点击整卡 → `workspace.openDelegatedChild(record.childSessionId)`。
- 若 `childSessionId` 对应的 session 不存在（极端恢复场景），卡片禁用并提示。

**状态展示共用**：把 `DelegationCard.vue` 里的 `statusText` / 配色 / `isWaiting`
抽到 `src/utils/delegationStatus.ts`（`delegationStatusText(status)`、
`delegationStatusClass(status)`、`isWaitingStatus(status)`），两处共用，避免第三份拷贝。

### D5 · 子会话详情

从 `CodingWorkspace.vue` 抽出 `src/views/coding/components/DelegatedChildView.vue`：

- 顶部：`← 返回`（`workspace.closeDelegatedChild()`）、标题、`×`（`workspace.closeInspector()`）。
- 主体：`ConversationView :key="session.id"`（自带 `ensureMessages`，无需额外加载）。
- 底部：`MessageComposer :session-id="session.id" passive`（复用现有权限 / 提问入口）。
- 组件从 store 读取 `delegatedChildSessionId` 或由 RightPanel 传入 session，二选一；
  建议**传入 session**，保持组件无 store 副作用、便于复用。

### D6 · 自动打开与关闭语义

| 动作 | 结果 |
|---|---|
| `delegation://started` | `openDelegatedChild(child.id)` → 右栏可见、页签 `subtasks`、展示详情 |
| 详情「返回」 | `delegatedChildSessionId = null` → 列表态 |
| 详情「×」 | `closeInspector()` → 整个右栏隐藏，`rightTab` 保持 `subtasks` |
| 顶栏右栏按钮 | 切换 `inspectorVisible`；重新打开回到上次页签 |
| 切换会话 | 详情保持可看；返回后展示新会话的列表（可能为空） |

### D7 · 边界与异常

- 无活动会话（文件标签 / 空标签）：`session === null` → 不显示子任务页签。
- 浏览器 mock：`delegation.records` 为空 → 页签隐藏，右栏照常渲染文件/变更/详情。
- 应用重启：`delegation.load()` 恢复 `interrupted` 记录；`main.ts` 已为每条记录
  `registerDelegatedSession` + `ensureDelegationCard`，列表可直接展示。
- 记录对应的 child session 缺失：卡片置灰并提示「子会话已不可用」，不抛错。
- 页面刷新后 `rightTab` 不持久化，回到默认 `files`（可接受，不在本期范围）。

## 3. 数据流

```
delegate_to_agent (MCP) → companion socket → acp.rs → delegation.rs broker
      ├─ delegation://started  ──► main.ts projectDelegation(record, true)
      │                              ├─ delegation.upsert(record)
      │                              ├─ sessionStore.registerDelegatedSession(record)
      │                              ├─ sessionStore.ensureDelegationCard(record)   // 父会话卡片
      │                              └─ workspace.openDelegatedChild(child.id)     // 右栏详情
      └─ delegation://updated  ──► upsert(record) + 状态投影 ──► 列表卡片实时刷新

RightPanel(subtasks)
   ├─ DelegationTaskList(parentSessionId) ── 过滤 delegation.records
   └─ DelegatedChildView(childSession)   ── ConversationView + passive composer
```

## 4. 兼容与回滚

- 仅前端组件/状态调整 + 一处 Rust 字符串；wire 契约、持久化结构、既有入口均不变。
- 回滚：还原 `RightPanel.vue`、`CodingWorkspace.vue`、`workspace.ts`，删除两个新组件与状态工具；
  工具描述回退为原文即可。
- `DelegationCard` 的状态工具抽取属于纯重构，回滚时可原样内联。

## 5. 风险

| 风险 | 缓解 |
|---|---|
| 页签状态提升后其它入口行为变化（`openDelegatedChild` 会自动切页签） | 现有调用点只有 `main.ts` 与 `DelegationCard`，均为「打开子会话」语义，符合新行为 |
| `ConversationView` 在窄栏内的滚动 / 高度 | 沿用当前抽屉的 `flex h-full min-h-0 flex-col` 布局，抽组件时保持结构 |
| 列表与详情共用右栏导致 `MessageComposer passive` 初始化差异 | 详情组件完全照搬现抽屉的 composer 用法，不改 props |
| 工具描述引导不生效 | 概率性措施；PRD 已注明后续可评估引擎侧禁用 |
