# Design: 新建会话流程（绑定文件夹 + 延迟入列 + 引擎/文件夹选择弹窗）

需求见 `prd.md`。桥接规范：`.trellis/spec/frontend/tauri-bridge.md`（无 Rust 改动）。
状态规范：`.trellis/spec/frontend/state-management.md`、`hook-guidelines.md`。

## 1. 核心思路：草稿区（draft staging）

现状 `sessionStore.get(id)` 与侧栏列表都只读 `sessions.value`。延迟入列要求「草稿不
在列表里，但 UI 能 `get` 到它」。最小改动 = **在 session store 内维护独立的草稿集合**：

```
sessions: ref<Session[]>       ← 「已入库」列表（侧栏渲染 + 索引落盘来源，不变）
drafts:   Map<string, Session> ← 新建未发送的会话（不进 sessions、不落盘）
```

- `get(id)` = `byId`(sessions) → 未命中再查 `drafts`。**所有既有消费点无需改**。
- 侧栏 `folderGroups`/`tmpSessions`/计数仍读 `sessions` → 草稿天然不显示。
- `persistIndex` 仍只序列化 `sessions` → 草稿天然不落盘。

`drafts` 用普通 `Map`（非 reactive）即可：会话对象本身不变，标签/Composer 按 id 取；
若需响应式（如 Composer 展示标题），可改 `ref<Session[]>`，二者等价。**选 `Map`**
（与 `messageLoads` 同风格，草稿数量极小）。

## 2. Session store 改造（`src/stores/session.ts`）

新增/改动：

```ts
const drafts = new Map<string, Session>()

function get(id): Session | null {
  if (!id) return null
  return byId.value.get(id) ?? drafts.get(id) ?? null
}

/** 新建草稿：不入列、不落盘、不写索引 */
function createDraft(model, opts): Session {
  const session = conversationService.createSession('新会话', model, {
    ...opts, engineId: opts.engineId ?? DEFAULT_ENGINE_WIRE,
  })
  session.messagesLoaded = true
  session.cwd = opts.cwd ?? tmpSessionDirSync()
  if (session.kind === 'tmp') session.storagePath = `${tmpSessionDirSync()}/${session.id}`
  drafts.set(session.id, session)
  return session
}

/** 首次发送时正式入库（草稿 → sessions + 落盘） */
function promoteDraft(id): void {
  const s = drafts.get(id)
  if (!s) return
  drafts.delete(id)
  sessions.value.unshift(s)
  schedulePersistIndex()
  // 标题：沿用 applyAutoTitle（发送流随后调用），或此处直接按首条消息设置
}

/** 丢弃草稿：仅当未入库时生效 */
function discardDraft(id): void {
  drafts.delete(id)
}

function isDraft(id): boolean { return drafts.has(id) }
```

- **标题编号**：原 `新会话 ${sessions.value.length + 1}` 依赖列表长度，草稿不计入会让
  多个草稿同名。改为**草稿创建时用统一占位「新会话」**，`applyAutoTitle` 在发送时按首条
  消息截断 20 字（现有逻辑 `startsWith('新会话')` 仍匹配）。避免编号错位。
- `deleteSession` 增加兜底：若 `id` 在 `drafts`，`drafts.delete(id)`（防御）。
- `forkSession`/`seedFolderSession` 不变（直接入 `sessions`，非草稿）。
- 导出 `createDraft / promoteDraft / discardDraft / isDraft`。

## 3. 发送流改造（`src/composables/useConversation.ts`）

`send(sessionId, content)` 在现有 `ensureMessages`（`:558`）**之前**插入提升：

```ts
// 首次发送：草稿正式入库（顺序：提升 → pushMessage → applyAutoTitle）
if (sessionStore.isDraft(sessionId)) sessionStore.promoteDraft(sessionId)
await sessionStore.ensureMessages(sessionId)
... 原有流程不变（pushMessage/applyAutoTitle/setStatus/ensureAcpSession/prompt）
```

- `promoteDraft` 在 `pushMessage` 前，保证「入列表」先于「消息写入」，且随后 turn 结束
  `persistMessages`（`:598`）会把索引 + 消息都落盘。
- `applyAutoTitle`（`:562`）在 promote 之后调用 → 此时会话已在 `sessions`，标题替换正常。

## 4. 丢弃时机

### 4.1 关标签（`SessionTabs.vue`）
现有 `closeTab`（`:15-20`）已 `workspace.closeTab` + `closeConnection(sessionId)`。新增：
```
if (sessionStore.isDraft(sessionId)) sessionStore.discardDraft(sessionId)
```
（放在 `closeConnection` 前后皆可；discard 只删 map 项。）无条件丢弃。

### 4.2 切走（`MessageComposer.vue`）
Composer 已 `watch(sessionId)`（`:333`）处理切换。新增：**切换前**判断旧会话是否草稿
**且输入框为空** → 丢弃。

```ts
watch(() => props.sessionId, (newId, oldId) => {
  if (oldId && sessionStore.isDraft(oldId) && !draft.value.trim()) {
    const prevCwd = ...; // 无需
    sessionStore.discardDraft(oldId)
    void closeConnection(oldId)   // 杀已建 ACP 进程（若 ensureConnected 已建）
  }
  // 原有：重置 draft、ensureConnected(newId)
})
```

- 「输入框为空」= `draft.value.trim() === ''`（Composer 的本地 ref）。
- 因丢弃发生在 Composer（唯一持有 `draft` 的地方），避免跨组件传「是否非空」。
- 有未发送文字 → 不丢弃，草稿留在 `drafts`；切回时 `openSession(id)` + `get` 仍可解析，
  标签仍在（`workspace` 不 removeSession），Composer 重新挂载后 `draft` 从会话状态恢复？
  ⚠️ **草稿文字不存在会话里**，Composer 的 `draft` 是组件本地 state，切换会重置。
  → 见 §7 风险：需决定「保留的草稿文字」是否随会话保存。

## 5. NewSessionDialog 改造

emits 变更：
```ts
emit('confirm', { engineId: EngineWire, target: { kind: 'folder'; projectId: string } | { kind: 'tmp' } })
```
- 新增文件夹下拉（来源 `projectStore.projects`），选项含「临时会话（不选文件夹）」。
- 默认选中：`projectStore.recentProjects[0]`（无则 tmp）。
- 引擎下拉维持现状（`10-13-acp-multi-engine` 已实现）。
- props 可加 `defaultProjectId?: string | null`（文件夹内新建直接构造 target，不经弹窗）。

调用方：
- `TopBar.vue` / `CodingWorkspace.vue` / `SidebarPanel.vue` 通用新建 → 弹窗 → 按 target 建草稿。
- 文件夹内新建（`SidebarPanel.vue:113` `createSessionInProject`）→ **不走弹窗**，直接
  `createDraft`（kind folder + projectId + rootPath cwd + 默认引擎）。

## 6. 数据流小结

```
新建（文件夹内）
  → sessionStore.createDraft(model, {kind:'folder', projectId, cwd, engineId})
  → workspace.openSession(id)              （标签 + 活动）
  → Composer 挂载 → ensureConnected → ACP 进程（可能已 spawn）

发送
  → useConversation.send: isDraft? promoteDraft（入列 + 落盘索引）
  → pushMessage / applyAutoTitle / prompt → turn 结束 persistMessages

关标签（草稿）
  → SessionTabs: isDraft → discardDraft + closeConnection

切走（草稿空）
  → Composer watch: isDraft(old) && !draft.trim() → discardDraft + closeConnection
切走（草稿有字）
  → 保留（drafts 仍有；输入文字存于模块级 Map，切回 Composer 恢复）
```

## 7. 风险 / 未决

- **草稿输入文字归属（已决策）**：Composer 的 `draft` 是本地 state，切换即重置。
  采用**模块级 `Map<sessionId, string>`** 保存未发送输入（照 `useConversationScroll`
  的 cross-mount 模式）：
  - Composer `onMounted`/`watch(sessionId)`：从 map 恢复 `draft`；
  - `draft` 变化时写入 map（仅草稿需要；正式会话无需，可统一写不区分）；
  - `discardDraft`/关标签丢弃时清除该 id 的 map 项。
  - 「输入是否非空」判定同时用于切走是否丢弃（§4.2）。
- **ensureConnected 早建连**：打开草稿即 spawn 进程；丢弃必须 `closeConnection`（已纳入）。
  若草稿从未被发送，agent 侧会话已建但随后被杀，可接受（`session_new` 成本已付）。
- **编号/标题**：草稿统一占位「新会话」，`applyAutoTitle` 首发时定标题（避免 length 编号错位）。
- **fork 不受影响**：fork 直接入 `sessions`，非草稿。
- **回滚**：改动集中在 `stores/session.ts`、`useConversation.ts`、`MessageComposer.vue`、
  `SessionTabs.vue`、`NewSessionDialog.vue`、三处调用方；git 单任务 diff 可整体 revert。

## 8. 测试要点（无自动化测试框架 → 手测）

- 文件夹内新建 → 无弹窗、草稿打开、列表无该会话。
- 通用新建 → 弹窗选文件夹 + 引擎 → 草稿打开、列表无。
- 首次发送 → 列表出现、索引含该 id、消息落盘。
- 草稿空输入切走 → 消失（列表/索引/进程）。
- 草稿有输入切走 → 停在该处，切回恢复文字与草稿。
- 草稿关标签 → 消失。
- 已入库会话关标签 → 记录保留、列表仍在。
- 浏览器 dev mock 全流程。
- 静态核查：新会话未发送时 `sessions-index.json` 无其 id。
