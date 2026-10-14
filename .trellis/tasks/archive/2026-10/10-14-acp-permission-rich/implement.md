# Implement: 权限授权富化（changes / diff / 队列）

需求 `prd.md`，设计 `design.md`，事实 `research/codeg-permission.md`。
执行顺序：Rust 小改（透传 meta + 行号注入）→ 前端（类型→解析→组件→mock）→ 检查。
每步可独立验证。

## 验证命令（本项目基准）

```powershell
cargo check            # workdir: src-tauri
cargo test --lib       # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 1 · Rust：请求级 `_meta` 透传（`src-tauri/src/acp.rs`）

- [ ] `handle_client_request` 的 `acp://permission` 事件增加
      `"meta": params.and_then(|p| p.get("_meta")).cloned().unwrap_or(Value::Null)`。
- [ ] 注释说明：请求级 `_meta`（title/description/defaultToNo）原样透传，前端解析。

**验证**：`cargo check`。
**回滚点**：`acp.rs`（一行）。

## Step 2 · Rust：`rawInput` 行号注入（best-effort，`acp.rs`）

- [ ] 抽纯函数 `inject_raw_input_start_line(raw: &Value) -> Value`：
      object 且有补丁类字段 → 注入 `_start_line`；string 且含 `@@` → 尝试 parse 为对象再注入，
      失败保持原值；其他 → 原值。
- [ ] 在 emit 前对 `toolCall` 的 `rawInput` 应用（若 `toolCall` 是 object）。
- [ ] 单测：object 注入、string patch 解析、非 patch 原样、null 原样。

**验证**：`cargo test --lib`。
**回滚点**：`acp.rs`（纯函数 + 调用点）。

## Step 3 · 前端：类型（`src/types/acp.ts`）

- [ ] `AcpPermissionOption.meta?: unknown`；`AcpPermissionEvent.meta?: unknown`。
- [ ] `AcpPermissionOption.kind` 放宽为 `... | string`（容纳 reject_always 等）。

**验证**：`pnpm vue-tsc --noEmit`。
**回滚点**：`types/acp.ts`。

## Step 4 · 前端：解析工具（新 `src/utils/permissionMeta.ts`）

- [ ] `parsePermissionTitle(event)`、`parsePermissionDescription(event)`、
      `isDefaultToNo(event)`、`parseOptionChanges(option)`、`extractDiff(rawInput)`。
- [ ] 上限常量（changes 条数/字符）仿 codeg。
- [ ] 纯函数，便于单测/手测。

**验证**：`pnpm vue-tsc --noEmit`。
**回滚点**：新文件。

## Step 5 · 前端：卡片改造（`PermissionCard.vue`）

- [ ] 头部改用 `parsePermissionTitle` + `parsePermissionDescription`（回退旧 toolName）。
- [ ] 新增区块：命令 / diff / 选项授予内容（按存在性）。
- [ ] 按钮改为遍历 `options` 动态渲染（`option.name ?? kind 文案`）；`defaultToNo` 强调 reject。
- [ ] 保留无 `options`/无 `meta` 时的旧三态回退（AC7）。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。
**回滚点**：`PermissionCard.vue`。

## Step 6 · 前端：diff 视图 + 队列计数 + mock

- [ ] diff 视图：优先复用既有统一 diff 组件；无则最小 `DiffPreview.vue`（不引新依赖）。
- [ ] `ConversationView.vue` 给卡传 `:queued-count`；卡上显示「还有 N 个待审批」。
- [ ] `acpMock.ts`：样例请求带 `meta` + 补丁 `rawInput`。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。
**回滚点**：视图/组件/mock。

## Step 7 · 全量质量检查（trellis-check）

- [ ] 四门禁全绿。
- [ ] PAC1：`rg -n "codex|claude|Codex|Claude" src-tauri/src/acp.rs` 仅 `#[cfg(test)]`；
      协议路由无引擎名。
- [ ] PAC4：opencode 零回归（无 `_meta` 时富区块不出现、按钮回退旧三态）。
- [ ] AC2/AC5：`parseOptionChanges` 两形态；动态按钮回应正确 `optionId`。
- [ ] AC3：title 回退链（claudeCode.title → permission.title → toolCall.title）；defaultToNo 反转。

## Step 8 · 实机验收（Tauri，人工）

- [ ] codex `Bash` 审批：卡显示命令 + changes（含 scope）；操作后 agent 继续。
- [ ] codex edit 审批：卡显示 diff。
- [ ] claude 审批：卡正常（无 changes 时不报错）。
- [ ] opencode 审批：与改造前一致。

---

## 关键契约（不可破坏）

- **协议路由不含引擎名**（PAC1）。
- **opencode 零回归**（PAC4）：无 `_meta` ⇒ 旧卡等价。
- **不 host fs/terminal**（D2）：行号注入纯本地，best-effort。
- **队列计数用前端既有队列**（D3）：不动 Rust `pending_perm`。
