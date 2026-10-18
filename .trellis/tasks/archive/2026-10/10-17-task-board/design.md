# Design: 任务看板

需求见 `prd.md`，codeg 盘点见 `research/codeg-task-board.md`，差距清单见 `research/codeg-gap.md`。
桥接规范：`.trellis/spec/frontend/tauri-bridge.md`。前端分层规范：`.trellis/spec/frontend/directory-structure.md`。

## 1. 分层

```
src/types/task.ts                 ← 唯一类型来源（WorkTask / WorkTaskEvent / TaskStatus）
src/services/taskService.ts       ← 唯一 invoke 入口，isTauri() 双通道
src/services/taskMock.ts          ← 浏览器内存实现（同形契约）
src/stores/task.ts                ← Pinia setup store：列表、筛选、派生分组
src/views/tasks/TasksView.vue     ← 看板页
src/views/tasks/boardColumns.ts   ← 状态 → 列映射（纯函数，可单测）
src/views/tasks/components/*      ← TaskCard / TaskColumn / TaskDetailSheet / TaskEditorDialog

src-tauri/src/task.rs             ← 命令 + 状态机 + JSON 读写
```

分层理由：`taskService` 与 `fsService`（`src/services/fsService.ts:35`）同构——组件与 store
永远不 `invoke`。列映射放 `views/tasks/boardColumns.ts` 而不是 `utils/`，因为它只服务这一个
feature（`directory-structure.md` §Module Organization）。

## 2. 数据模型

### 2.1 `WorkTask`（`src/types/task.ts`）

```ts
export type TaskStatus =
  | 'todo' | 'queued' | 'preparing' | 'running'
  | 'awaiting_input' | 'review' | 'merging'
  | 'done' | 'failed' | 'canceled'

export interface WorkTask {
  id: string                  // 'task-xxxxxxxx'
  projectId: string           // Project.id（'proj-xxxx'）；项目被移除后仍保留
  title: string
  prompt: string              // 发给 Agent 的正文（首版纯文本）
  status: TaskStatus
  sortOrder: number           // 列内排序，仅同 projectId 内有意义
  runSeq: number              // 每次启动 +1；用于丢弃迟到的 turn 回调
  sessionId: string | null    // 一对一 ACP 会话
  engineId: EngineWire | null // 创建时固化，启动时用它建会话
  verdict: 'success' | 'needs_review' | 'blocked' | null
  resultSummary: string | null
  lastError: string | null
  failureReason: 'agent_error' | 'interrupted' | 'empty_turn' | null
  archivedAt: number | null
  createdAt: number
  updatedAt: number
  startedAt: number | null
  settledAt: number | null    // 进入 review / failed
  finishedAt: number | null   // 进入 done / canceled
}

export interface WorkTaskEvent {
  id: string
  taskId: string
  kind: 'created' | 'status_changed' | 'user_action'
       | 'agent_progress' | 'agent_verdict'
  actor: 'user' | 'engine' | 'agent'
  payload: Record<string, unknown> | null
  createdAt: number
}
```

与 codeg 的对应（`models/work_task.rs:9`）：
- `folder_id: i32` → `projectId: string`（本项目项目 id 是字符串，`types/project.ts:7`）
- `config: JSON` → `prompt: string`（首版无 prompt_blocks / per-task override）
- 新增 `engineId`：codeg 的 agent 类型在 `config.agent_type` 与文件夹设置里解析，本项目
  没有文件夹设置表，直接固化在任务上
- 其余业务列（worktree / merge / preflight / source / scheduled）见 `research/codeg-gap.md`

### 2.2 文件格式（`~/.lightr-vibe/lightr-vibe/tasks.json`）

```json
{
  "version": 1,
  "tasks": [ /* WorkTask[] */ ],
  "events": [ /* WorkTaskEvent[]，按时间升序，每任务上限 200 条 */ ]
}
```

- `version` 用于将来迁移；读到更高版本时只读降级（保留未知字段，写回时原样带上）。
- 单文件、整体读写。写入走 `fs_write_text`（Rust 侧原子写 + 自动建目录，
  `fsService.ts:79`）。
- 事件截断：每任务保留最近 200 条，写入时裁剪，防止无限增长。

## 3. 状态机

### 3.1 转换表（唯一权威，Rust 侧 `src-tauri/src/task.rs`）

| from | 允许到 |
|---|---|
| `todo` | `queued`, `canceled` |
| `queued` | `preparing`, `todo`, `canceled` |
| `preparing` | `running`, `failed`, `canceled` |
| `running` | `awaiting_input`, `review`, `failed`, `canceled` |
| `awaiting_input` | `running`, `canceled` |
| `review` | `done`, `todo`, `canceled` |
| `failed` | `queued`, `canceled` |
| `canceled` | `todo` |
| `done` | （仅归档，不改状态） |
| `merging` | 本任务不产生该状态，保留枚举以兼容 codeg 列映射 |

与 codeg 的差异：去掉 `merging`（无合并）、`review → merging`（无合并落地）、
`failed → canceled` 保留但 UI 不提供。**`review → todo`（打回）是本项目新增的显式转换**——
codeg 用 `return`（追加上下文并重跑），本任务简化为「打回待办，保留原有提示词」。

### 3.2 校验方式

`task_transition(id, to, expectedFrom)`：

```
读 → 取内存锁 → 校验 row.status === expectedFrom（不匹配返回 'stale'）
              → 校验 TRANSITIONS[row.status] 含 to（不含返回 'illegal-transition: <from>-><to>'）
              → 按 to 写时间戳（startedAt / settledAt / finishedAt）
              → runSeq + 1（仅进入 queued 时）
              → 写 status_changed 事件
              → 落盘 → 返回新行
```

约定错误串（前端按前缀分类，同 `classifyFsError` 模式，`types/project.ts:54`）：
`not-found` / `stale` / `illegal-transition: ...` / `io: ...`。

## 4. 四列映射

`src/views/tasks/boardColumns.ts` 直接移植 `codeg/board-columns.ts:22`：

```ts
export type BoardColumnId = 'todo' | 'inProgress' | 'attention' | 'done'
export const STATUSES_BY_COLUMN = {
  todo: ['todo', 'queued'],
  inProgress: ['preparing', 'running'],
  attention: ['awaiting_input', 'review', 'merging', 'failed'],
  done: ['done', 'canceled'],
}
```

- `columnForStatus` 用 `switch` 穷举，TS 保证新增状态必须归列（否则编译失败）——比
  codeg 的「靠测试断言」更强。
- `groupTasksByColumn(tasks, { showCanceled, showArchived })`：过滤 + 按 `updatedAt` 降序。
- 拖拽落点用 `allowedDrop(fromStatus, columnId)` 判定：只有
  `(todo → inProgress)`、`(review → done)`、`(review → todo)` 返回目标状态，其余返回 `null`
  → 拖拽视觉上拒绝（列不亮），指针释放时不发请求。

## 5. 拖拽

手写 Pointer Events，参考本项目既有模式 `CodingWorkspace.vue:65-86`（pointerdown +
window pointermove/pointerup + 阈值），加上 codeg 的两个关键技术点：

1. **Teleport 预览**（`tasks-page.tsx:1036`）：被拖卡片复制一份到 `<Teleport to="body">`，
   `position: fixed; pointer-events: none`，跟随指针。原因：看板列是滚动容器，`overflow`
   会把卡片裁掉。原卡片降到 `opacity-40`。
2. **落列判定用指针坐标与列矩形求交**，不用 HTML5 drag 的 `dragover`：前者不受
   子元素 `pointer-events` 影响，且能精确判断「斜着拖到右列」。

交互细节：
- `pointerdown` 后移动超过 4px 才算拖拽（区分点击）。
- 拖拽中同列内用指针 y 与各卡片中线比较，实时重排本地数组（乐观）。
- `pointerup`：
  - 同列 → `taskService.reorder(projectId, orderedIds)`（只发 `sortOrder`）
  - 目标列合法 → `taskService.transition(id, target, 'todo'|'review')`
  - 目标列非法 → 不请求，回弹，卡片抖动一次（`animate-shake`，CSS 已有动画则复用）
- 拖拽结束后 300ms 内抑制卡片的 `click`（`draggedRef`），避免误开详情。

`projectId` 为 `null`（全部项目视图）时禁止跨列拖拽与排序，只允许「点击卡片操作」——
codeg 同款取舍（`tasks-page.tsx:300 dragEnabled = folderFilter != null`）。

## 6. UI 结构

```
TasksView.vue
├── 顶部工具栏：项目筛选（Project[] + 「全部项目」）、隐藏已取消 / 已归档、新建任务
├── 四列 flex（每列 min-w-[280px]，横向 overflow-auto）
│   └── TaskColumn.vue   ← 标题 + 计数 + 卡片列表 + 拖拽高亮
│       └── TaskCard.vue ← 状态 chip + 标题 + 项目名 + 相对时间 + 主/次动作
├── TaskDetailDialog.vue ← 标题、提示词、时间线、会话入口、审核动作
└── TaskEditorDialog.vue ← 新建/编辑：项目选择、标题、提示词、引擎
```

- 路由 `/tasks`（`router/index.ts`），顶栏加一个入口按钮（`TopBar.vue`）。
- 视觉沿用项目现有风格：`rounded-md border border-neutral-200`、`text-xs`、无阴影。
- 状态 chip 照 codeg `task-card.tsx:70` 的分色语义，但用本项目色板
  （`neutral-*` + 主色，不引 amber/emerald 之外的语义色）。

## 7. 会话联动（子任务 3）

- **启动**：`transition(id,'queued','todo')` → `sessionStore.createDraft(model, { kind:'folder',
  projectId, cwd, engineId })` → `taskService.attachSession(id, sessionId)` →
  `transition(id,'preparing','queued')` → `workspace.openSession(sessionId)` →
  `transition(id,'running','preparing')` → 发送 `prompt`。
- **回流**：在 `useConversation` 的 turn 结束处（`send()` 的 `finally` 区）挂一个钩子
  `onTurnSettled(sessionId, { ok, errorKind, runSeq })`，由 task store 反查 `sessionId`
  对应的任务：
  - `ok` → `running → review`
  - `errorKind === 'empty'` → `running → failed` + `failureReason='empty_turn'`
  - 其他错误 → `running → failed` + `failureReason='agent_error'`
  - 用户主动 stop → 保持 `running`（不判失败），由用户再点「打回」或「通过」
- `runSeq` 防串：任务行记录启动时的 `runSeq`，回调携带同一个值，不匹配则丢弃
  （codeg 同机制，`entities/work_task.rs:66` 注释）。

## 8. 兼容 / 回滚

- 新增文件为主，对现有代码的侵入只有：`router/index.ts` 加一条路由、`TopBar.vue` 加一个按钮、
  `lib.rs` 注册命令、`appData.ts` 加一个路径函数、`useConversation` 加一个可选钩子。
- 回滚 = 删除 `src/views/tasks/`、`src/types/task.ts`、`src/services/taskService.ts`、
  `src/stores/task.ts`、`src-tauri/src/task.rs`，还原上述 5 处小改动。
- `tasks.json` 不参与启动关键路径：读失败按空任务处理并记 toast，不影响应用启动。

## 9. 风险

| 风险 | 缓解 |
|---|---|
| JSON 整体读写并发丢更新 | Rust 侧单 `Mutex` 串行化所有读写；前端所有变更都 await |
| 拖拽与后台状态推送冲突 | 拖拽只改本地顺序，drop 时重读；不做跨列乐观更新 |
| `review → todo` 打回后原会话是否复用 | 打回**保留** `sessionId`，下次启动走 `acp_session_load` 续上原会话；用户可在详情里「断开会话」清空 |
| 事件无限增长 | 每任务截断 200 条 |
| 项目被移除后的孤儿任务 | 按 `projectId` 保留，卡片标记「项目已移除」，详情只允许删除/归档 |
