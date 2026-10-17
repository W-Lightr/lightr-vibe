# Research: codeg 待办任务实现盘点

参考仓库：`H:\AI\Light-vibe\codeg`（Tauri 2 + Rust + Next.js 16，SeaORM/SQLite）。

## 1. 分层

| 层 | 文件 | 规模 |
|---|---|---|
| DB 实体 | `src-tauri/src/db/entities/work_task.rs` | 10 状态枚举 + 49 列 |
| DB 实体 | `src-tauri/src/db/entities/work_task_event.rs` | append-only 时间线 |
| DB 实体 | `src-tauri/src/db/entities/work_task_settings.rs` | 每文件夹一份默认值 |
| 迁移 | `src-tauri/src/db/migration/m20260801_000001_work_task.rs` 等 6 个 | 渐进加列 |
| wire DTO | `src-tauri/src/models/work_task.rs` | `WorkTaskInfo` / `WorkTaskDraft` / `WorkTaskConfig` |
| 业务逻辑 | `src-tauri/src/db/service/work_task_service.rs` | 5039 行，约 60 个 pub fn |
| 命令层 | `src-tauri/src/commands/work_task.rs` | 1498 行，`_core` + `#[tauri::command]` 双层 |
| Web 镜像 | `src-tauri/src/web/handlers/work_task.rs` | 531 行（服务器模式） |
| 前端页面 | `src/components/tasks/tasks-page.tsx` | 1074 行 |
| 前端卡片 | `src/components/tasks/task-card.tsx` | 660 行 |
| 前端动作矩阵 | `src/components/tasks/task-actions.ts` | 216 行 |
| 前端列映射 | `src/components/tasks/board-columns.ts` | 128 行 |
| 前端详情 | `src/components/tasks/task-detail-sheet.tsx` | 2074 行 |
| 前端编辑器 | `src/components/tasks/task-editor-dialog.tsx` | 26.6 KB |

## 2. 状态机（`db/entities/work_task.rs:4-47`）

```
todo → queued → preparing → running ⇄ awaiting_input → review → merging → done
                                                            ↘ failed
                                            任意非 merging → canceled
```

两条硬不变量：
1. `done` ⟺ **已接受的最终成功**，永不回滚。三条进入路径（merge 落地 / 交付 PR / 无改动直接接受）
   都**先过 `review`**，`completion_kind` 记录是哪一条。
2. 每个转换都是 **CAS**（`UPDATE ... WHERE status = expected AND run_seq = expected_seq`）。

## 3. 4 列看板映射（`src/components/tasks/board-columns.ts:22`）

```ts
todo       = ["todo", "queued"]
inProgress = ["preparing", "running"]
attention  = ["awaiting_input", "review", "merging", "failed"]
done       = ["done", "canceled"]
```

- `columnForStatus` 是唯一真源；`STATUSES_BY_COLUMN` 是它的规格副本，测试断言两者一致
  （`board-columns.test.ts`）——新增状态若未归列会测试失败而不是从看板静默消失。
- `canceled` 落在 Done 列但默认隐藏，由 `showCanceled` 开关过滤。
- 归档任务由 `showArchived` 过滤。
- 列内排序：`updated_at DESC`（最新动作置顶，`:98 byFreshest`）。`sort_order` 只在时间戳相同时
  做 tie-break——**这是避免拖拽排序与引擎状态更新互相打架的关键**。

## 4. 拖拽（`src/components/tasks/tasks-page.tsx`）

- 库：`motion/react` 的 `Reorder.Group` / `Reorder.Item`（`:899-935`）。
- **只允许 todo 列内纵向排序**；拖动时指针位置的矩形判定决定是否落到 In-progress：
  `inProgressColRef.current.getBoundingClientRect()` 含点即 `workTaskStart(id)`（`:560-573`）。
- 拖动预览：portal 到 body 的 fixed 副本（`:1036`），因为列是滚动容器会裁剪。读 `grabRef`
  的抓取偏移让预览与卡片 1:1 跟随。
- 排序落库：`work_task_service.rs:973 reorder()` 在一个事务内按 index 写 `sort_order`，
  作用域 `folder_id`；跨 folder 混合视图不排序（`dragEnabled = folderFilter != null`）。
- 拖完抑制随后的 click（`draggedRef`），否则会误开详情抽屉。
- 拖拽中任务被引擎推进（自动认领）时 `abortDrag()`（`:540`），drop 作用于**实时行**不是快照。

## 5. 卡片动作矩阵（`src/components/tasks/task-actions.ts:80-216`）

| 状态 | primary | secondaries |
|---|---|---|
| archived | 取消归档 | 查看会话 |
| todo | 开始 | 编辑、计划时间 |
| queued/preparing/running/awaiting_input | 取消 | 查看会话 |
| review（已排队合并） | — | 取消排队、查看会话 |
| review（无改动） | 直接接受 | 查看会话 |
| review（来自 PR） | 交付回 PR | 查看会话 |
| review（普通） | 合并 | 查看会话 |
| merging | —（不可取消） | 查看会话 |
| failed | 重试 | 编辑、归档 |
| done | 归档 | 查看会话 |
| canceled | 重新入队 | 归档 |

设计原则：**每个状态恰好一个填充主按钮**；「查看会话」永远排最后锚定右侧。

## 6. 状态 chip 视觉（`src/components/tasks/task-card.tsx:70-140`）

- 运行中（queued/preparing/running/merging）：主色文字 + 旋转 `Loader2`
- 等待输入 / 审核：琥珀色描边胶囊
- done：绿色底 + 勾
- failed：红色底（`interrupted` 单独文案）
- 其余：muted 胶囊

## 7. 创建路径

`work_task_service::create` → `insert_todo_row`（`work_task_service.rs:488`）。`WorkTaskDraft`
只有 `folder_id` / `title` / `config`（JSON 快照）；`config` 里是 `prompt_blocks` +
可选 per-task override。**source（forge 来源）故意不在 DTO 里**，只能由 forge 触发命令写入，
防止客户端伪造来源。

## 8. 前端取数

`TasksViewProvider`（`src/contexts/task-context.tsx`）+ `task-context` 常驻，所有变更
fire-and-refetch；引擎侧 `task://changed` 事件推送。不做前端乐观状态机。

## 9. 本项目对照

| 能力 | codeg | lightr-vibe 现状 |
|---|---|---|
| 持久化 | SQLite + SeaORM 迁移 | `projects.json` / `sessions-index.json`（`utils/appData.ts:37`） |
| 后端 | 60 个 service fn + 30 个命令 | `src-tauri/src` 仅 `acp/agent/engine/fs`，**无 DB** |
| 路由 | `/tasks` | `/` `/settings` `/coding`（`router/index.ts:6`） |
| 拖拽 | motion/react | 无；但有手写 pointer 拖拽先例（`CodingWorkspace.vue:65-86`） |
| 会话联动 | `conversation_id` | `sessionStore.createDraft` + `workspace.openSession` |
| 项目模型 | `folder` 表（数字 id） | `Project`（`id` 为 `proj-xxxx` 字符串，`src/types/project.ts:6`） |

关键不适用项：codeg 的 `folder_id: i32` 对应到本项目必须换成 `projectId: string`。
