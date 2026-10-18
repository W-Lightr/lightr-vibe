# 与 codeg 的差距清单（本任务不实现）

本文件是每次 codeg 对齐讨论的收口表。左列 = codeg 已有；中列 = 本任务是否覆盖；
右列 = 若不覆盖的原因与后续可选任务。

## A. 本任务覆盖

| codeg 能力 | 本任务 |
|---|---|
| `work_task` 表（title / status / sort_order / conversation_id / 时间戳） | ✅ JSON 行同构 |
| 10 状态枚举与 CAS 转换 | ✅ 但转换表精简（见 D3） |
| 4 列看板聚合 `columnForStatus` | ✅ 原样移植 |
| 列内排序 + `sort_order` 持久化 | ✅ |
| 任务时间线 `work_task_event` | ✅ 精简 kind 集合 |
| 卡片动作矩阵 | ✅ 精简为开始/停止/通过/打回/重试/归档 |
| 项目（文件夹）绑定 | ✅ `projectId: string` |
| 详情抽屉 + 会话跳转 | ✅ |
| `cancelled` 在 Done 列可隐藏 | ✅ |

## B. 明确不实现

| codeg 能力 | 位置 | 为什么砍 |
|---|---|---|
| git worktree 隔离 | `work_task.base_branch/base_sha/work_branch/worktree_folder_id`，`work_task_service.rs:1312` | 首版一个任务=一个会话，直接在项目目录跑；worktree 需要完整的 git 生命周期管理（创建/复用/清理/失败恢复），复杂度约占 codeg service 的 30% |
| 合并落地 `merging` | `begin_merge`（`:1804`）、`merge_landed`（`:2167`）、`WorkTaskMergeState` | 没有 worktree 就没有「落地到 base 分支」这件事；本任务 `review → done` 只记录审核结论 |
| merge pump 串行队列 | `queue_merge`（`:1915`）、`pending_merge` 列、`WorkTaskQueuedMerge` | 同上一行，无并发落地就没有排队需求 |
| PR 交付 | `deliver_pr`（`:2020`）、`begin_delivery`、`mustDeliverToPr` | 需要 forge（GitHub/GitLab）集成与凭据管理 |
| forge issue/PR 导入 | `create_from_forge`（`:547`）、`source_kind/source_key/source_meta` | 需要 forge API 与去重键设计 |
| preflight 验收红绿灯 | `set_preflight`（`:2348`）、`WorkTaskPreflight` | 需要在 worktree 里跑命令并采集输出；依赖 worktree |
| 并发预算 `max_concurrent` | `auto_claim_next`（`:1011`）、`folders_with_pending` | 本任务不自动认领，任务是用户手动点开始 |
| 自动认领 `auto_process` | `claim_unplanned_for_run`（`:846`）、`claim_due_scheduled`（`:1143`） | 需要常驻调度器；首版人工驱动 |
| 定时启动 `scheduled_at` | `set_schedule`（`:1104`）、`task-schedule-dialog.tsx` | 需要 cron 调度器 |
| 自动合并 `auto_merge` | `WorkTaskFolderSettings.auto_merge` | 依赖 merge（已砍） |
| 崩溃恢复 | `boot_reconcile_interrupted`（`:2629`）、`abandon_setup`（`:1393`） | 没有 `preparing/merging` 这类不可中断态，进程重启只需把 `running` 标记为 `interrupted` |
| 任务模板 | `work_task_template` 表、`task_editor-dialog` 模板区 | 首版编辑器只有标题 + 提示词 |
| 文件夹级设置表 | `work_task_settings` + `WorkTaskFolderSettings` 20 个字段 | 首版没有可配置项，默认值硬编码 |
| `requeue` 来源去重守卫 | `resurrection_guard`、`other_active_with_same_source`（`:624`） | 没有 forge 来源，不存在同一工作项双活 |
| 失败原因细分 | `failure_reason`: agent_error / setup_error / verdict_blocked / interrupted | 精简为 agent_error / interrupted / empty_turn |
| Agent 自报 verdict（MCP） | `set_verdict`（`:1630`）、`codeg-mcp` 伴生进程 | 本项目 MCP 伴生仅做提问（`lightr_ask_mcp.rs`），不扩工具集 |
| 上下文压缩 | `context_compact` 事件、`auto_compact_percent` | 属 `10-14` 父任务 Out of Scope 的 AIR 重型机制 |
| 关注度角标 | `work_task_attention_count` | 无状态栏 UI |
| 会话改名联动 | `work_task_update` 内 rename 逻辑 | 无「会话标题锁定」概念 |
| 服务器模式 HTTP API | `web/handlers/work_task.rs` | 本项目只做桌面 |
| 归档全部已完成 | `archiveAllDone`（`tasks-page.tsx:593`） | 可延后；单条归档已覆盖 |
| 列表视图 | `task-list.tsx` + `filterTasksForList` | 首版只有看板视图 |

## C. 差异化设计（不是缺失，是有意换法）

| 点 | codeg | 本项目 |
|---|---|---|
| 存储 | SQLite + SeaORM | `tasks.json` 单文件原子写（`fs_write_text`）+ Rust 内存态串行 |
| 并发安全 | SQL CAS UPDATE | Rust 侧 `Mutex` 保护的读改写 + 内存 `expectedFrom` 校验 |
| 任务 id | 自增 i32 | `task-<nanoid>`（`utils/id.ts`） |
| 拖拽库 | motion/react | 手写 Pointer Events + `<Teleport>` 预览 |
| 状态推送 | `task://changed` 事件 | 前端 store 本地更新 + 变更后重读（首版单窗口够用） |
