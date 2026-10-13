# 新建会话流程：绑定文件夹 + 延迟入列 + 引擎/文件夹选择弹窗

父任务：`10-08-real-agent-workspace`。前置：`10-13-acp-multi-engine`（引擎注册表已就绪，
`NewSessionDialog` 已有引擎下拉 + `Session.engineId`）。

## Goal / 用户价值

重构「新建会话」的入口语义与生命周期：

1. **文件夹内新建** = 该会话直接绑定当前文件夹（无弹窗，不选任何东西）。
2. **标签栏 / 左上角菜单新建** = 弹窗里同时选**引擎** + **当前已有的文件夹**（保留
   「不选文件夹」的临时会话选项）。
3. **延迟入列** = 新建的会话**不立即**出现在会话列表、不落盘；只有**首次发送消息后**
   才正式入库（进入列表 + 持久化）。未发消息就切走 / 关标签 → **直接丢弃**。

## Background / Confirmed facts（证据：代码勘察）

**创建入口（4 处 UI + 2 处内部）**
- `CodingWorkspace.vue:28-31` `createSession()` → 弹窗 `newDialogOpen`；确认
  `:35-44` 用 `recentProjects[0]` 作 projectId。
- `TopBar.vue:36-39` `newSession()` → 弹窗；确认 `:43-52` 同上。
- `SidebarPanel.vue`：`:99-104` 通用新建、`:106-110` tmp、`:113-117` 文件夹内新建；
  共享确认 `:125-141`。
- 内部：`seedFolderSession`（浏览器 mock，`session.ts:19-51`）、`forkSession`
  （`session.ts:276-316`）。

**会话存储与持久化（`src/stores/session.ts`）**
- `createSession`（`:227-250`）：`conversationService.createSession` →
  `sessions.value.unshift(session)`（`:247`）**立即入列** + `schedulePersistIndex()`
  （`:248`）**约 1s 内落盘**，即使没发过消息。标题占位 `新会话 ${length+1}`（`:238`）。
- `persistIndex`（`:159-166`）/ `schedulePersistIndex`（`:168-174`，1s debounce）。
- `persistMessages`（`:177-186`）：turn 结束整会话消息全量覆写，并 `schedulePersistIndex`。
- `load`（`:189-201`）：读索引 → `parseIndex().map(sessionFromEntry)`（不含消息）。
- `ensureMessages`（`:207-225`）：打开会话时懒加载消息。
- `get(id)`（`:153-155`）：**只查 `sessions.value`**（`byId` computed 基于该数组）。
- `applyAutoTitle`（`:261-267`）：仅当标题 `startsWith('新会话')` 时用首条消息前 20 字替换。
- `SessionStatus = 'idle'|'generating'|'error'`（`types/session.ts:9`）——**无 draft/active 概念**；
  `SessionIndexEntry`（`types/acp.ts:164-177`）**无 status/draft 字段**。

**发送流（`src/composables/useConversation.ts`）**
- `send(sessionId, content)`（`:552-638`）：门禁 → `ensureMessages`（`:558`）→
  `pushMessage`（`:561`）+ `applyAutoTitle`（`:562`）+ `setStatus('generating')` →
  `ensureAcpSession`（`:568`）→ `acpService.prompt` → turn 结束 `persistMessages`（`:598`）。
- `ensureAcpSession`（`:438-522`）：`connect` + `sessionNew`/`sessionLoad`；
  **首次建 ACP 会话即设 `session.acpSessionId`**（`:496`）。
- `ensureConnected`（`:770-786`）：`MessageComposer` 挂载 / 会话切换时**自动建连**
  （`MessageComposer.vue:333,347`）——即打开会话就会 spawn 进程，早于任何消息。

**弹窗（`src/views/coding/components/NewSessionDialog.vue`）**
- props `{ open }`，emits `{ close, confirm(engineId) }`；**只选引擎，无文件夹选择**。

**项目（`src/stores/project.ts`）**
- `Project { id, name, rootPath, addedAt, lastOpenedAt }`（`types/project.ts:6-16`）；
  `projects`（`:16`）为已有文件夹列表；`recentProjects`（`:26-28`）前 5；`get`（`:30-32`）。

**工作区 / 标签（`src/stores/workspace.ts`）**
- `openSession(sessionId)`（`:42-49`）建 `tab-ses-${sessionId}`；`activeSessionId`（`:37-39`）。
- `closeTab(tabId)`（`:62-70`）只移标签，**不删会话**；`SessionTabs.vue:15-20` 额外
  `closeConnection(sessionId)`（杀 ACP 进程）但**从不 `deleteSession`**。
- `forgetSession(sessionId)`（`:73-78`）：删该会话所有标签，用于删除会话流。

**延迟入列的关键摩擦（设计必须处理）**
1. `get(id)` 只读 `sessions.value`；侧栏 `folderGroups`/`tmpSessions` 也读同一数组
   （`SidebarPanel.vue:159-173`）——草稿不能进 `sessions`，但 UI 又必须能 `get` 到它。
2. Tab / Composer / ConversationView 全部按 `sessionId` 取会话对象
   （`CodingWorkspace.vue:23`、`MessageComposer.vue:57`）——草稿需可解析。
3. 打开会话即 `ensureConnected` → `sessionNew`，会产生 agent 侧会话 + 进程；
   丢弃草稿需一并 `closeConnection`。
4. 标题占位与编号基于 `sessions.value.length`；草稿若不计入会与列表编号错位（需重定义）。
5. `closeTab` 目前不删会话；丢弃草稿是**新行为**，触发点在 `SessionTabs`（关标签）与
   会话切换（`MessageComposer` watch `sessionId`）两处。

## Resolved Decisions（用户已确认）

- **D1 文件夹行内新建**：绑定当前文件夹，**直接创建，不弹窗**。
- **D2 标签栏 / 左上角菜单新建**：弹窗选**引擎 + 当前已有文件夹**（下拉）；保留
  「临时会话（不选文件夹）」选项。**不支持在弹窗内新建文件夹**（文件夹来源 = 已打开的
  项目列表）。
- **D3 延迟入列**：新建后**不立即**进入 `sessions` 列表、不落盘；**首次发送消息时**才
  正式 `createSession`（入列表 + 落盘）。实现方式 = **首次发送时正式创建**。
- **D4 丢弃（分场景）**：未发任何消息的草稿——
  - **关标签** → 丢弃（不落盘、不列表、杀已建 ACP 进程）。不做二次确认。
  - **切走** → **仅当该草稿输入框为空**时丢弃；若输入框有未发送文字则**保留草稿**
    （仍在草稿区，可切回继续）。
- **D5 保留 tmp**：无文件夹的「临时会话」入口保留，且** tmp 同样走延迟入列**（首次
  发送才入列表）。

## Requirements

### R1 文件夹内新建（无弹窗）
- 文件夹行内「新建会话」按钮 → 直接以该 `projectId`（+ 其 `rootPath` 作 cwd）创建一个
  **草稿会话**并打开；不弹窗。
- 引擎取当前默认引擎（`engineStore.defaultEngineId`）。

### R2 外部新建弹窗（引擎 + 文件夹）
- `NewSessionDialog` 增加**文件夹选择**（下拉，来源 `projectStore.projects`）+ 保留引擎下拉，
  外加「临时会话（不选文件夹）」选项。
- 标签栏（`TopBar`）与左上角菜单 / 侧栏通用新建：确认后按所选文件夹 + 引擎创建草稿。
- 无已有文件夹时：文件夹下拉为空 → 只能选「临时会话」，或提示先打开文件夹。
- emits 从 `confirm(engineId)` 扩展为携带目标（engineId + projectId/tmp）。

### R3 延迟入列（草稿区）
- 新增**草稿暂存**：`sessionStore` 维护 `drafts`（不在 `sessions`/索引中的会话对象），
  `get(id)` 同时查 `sessions` 与 `drafts`。
- 新建只写草稿：不进 `sessions.value`、不 `schedulePersistIndex`、不落盘。
- 侧栏列表（`folderGroups`/`tmpSessions`/计数）**不显示草稿**（仍只读 `sessions`）。
- 草稿打开为标签（`openSession(id)` + `get(id)` 可解析），Composer/ConversationView 正常渲染。

### R4 首次发送 → 正式创建
- `send(sessionId, content)` 首先判断该会话是否仍在草稿区；若是：**先 `promoteDraft(id)`**
  （移入 `sessions` + 写标题 + `schedulePersistIndex`），再走原发送流程。
- 提升时机在 `pushMessage`/`applyAutoTitle` 之前，保证「发送 = 入库」。
- 标题：沿用首条消息截断 20 字规则（`applyAutoTitle`）。

### R5 丢弃（切走 / 关标签）
- **关标签**（`SessionTabs` 关闭会话标签）：若该会话仍是草稿 → `discardDraft(id)`
  （从 `drafts` 移除 + `closeConnection(id)` 杀进程 + `forgetSession`）。**无条件丢弃**。
- **切走**（活动会话变更）：若之前的活动会话是草稿**且其输入框当前为空** → 丢弃；
  **若输入框有未发送文字则保留草稿**（不丢，可切回继续）。
  - 「输入框内容」由 `MessageComposer` 的 `draft` 维护，需暴露一个查询/回调：
    切走前由 Composer 上报「当前草稿是否非空」，或由 composer 自身在 `watch(sessionId)`
    时决定是否调用 `discardDraft`。
  - 定义「未发送」= 仍在 `drafts` 集合（发送即 promotes，离开草稿区）。
- 正式会话（已入库）关标签保持现状（不删记录，仅杀进程）。

### R6 不影响既有路径
- `forkSession` 仍即时入库（分叉本身携带历史，不算草稿）。
- 启动 `load`、`ensureMessages`、持久化格式（`SessionIndexEntry`）不变——草稿从不落盘，
  故无需 schema 变更。
- 浏览器 mock（`seedFolderSession`）不变。

## Acceptance Criteria

- [ ] AC1 `vue-tsc --noEmit` 通过（无 Rust 改动）。
- [ ] AC2 文件夹行内「新建会话」→ 直接创建并打开草稿，**无弹窗**，绑定该文件夹。
- [ ] AC3 标签栏 / 侧栏通用新建 → 弹窗可选**引擎 + 已有文件夹**（或临时会话），确认后打开草稿。
- [ ] AC4 新建后**会话列表不出现**该会话；关标签/切走后不留任何记录（索引文件无该 id）。
- [ ] AC5 草稿打开时 Composer 可正常输入；**首次发送后**会话出现在列表，且 `sessions-index.json`
      出现该 id。
- [ ] AC6 未发送草稿关标签 → 无残留：不在列表、不在索引、ACP 进程被杀。
- [ ] AC7 已入库会话行为不变（关标签不删记录、列表保留、重启恢复）。
- [ ] AC8 无已有文件夹时弹窗表现合理（只能选临时会话 / 明确提示）。
- [ ] AC9 浏览器 dev mock 全流程不崩溃。

## Out of Scope

- 在弹窗内新建文件夹（只选已打开的项目）。
- 持久化草稿（草稿不跨重启；重启即消失，符合「未发送即丢弃」语义）。
- 会话列表草稿的显式标识（灰显/「草稿」标签）——草稿不进列表，故不需要。
- 修改 `SessionIndexEntry` schema / 迁移。
- Rust / ACP 命令面改动。
