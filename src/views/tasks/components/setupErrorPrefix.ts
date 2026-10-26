// 「启动准备失败」（setup_error）的文案前缀：桥接层把它写在 `lastError` 开头，
// chip 与详情抽屉据此把这次失败说成「启动失败」，而不是 Agent 报错。
//
// 为什么单独一个文件：写方是 taskSessionBridge、读方是 taskChip / TaskDetailDialog。
// 前缀若定义在桥接层，chip 会为了一个常量把「桥接到主进程 + 会话链路」的整条依赖链
// 拉进纯展示模块（循环导入：bridge → session store → … → chip）。常量没有依赖，
// 单独成文件后写方与读方都只依赖它。

/** `failed` + 此前缀的 `lastError` = 启动准备失败（setup_error，见 taskSessionBridge `failSetup`） */
export const SETUP_ERROR_PREFIX = '准备失败：'

/**
 * 工作树清理失败的附注文案前缀（lifecycle R3 / AC9）：写方是桥接层的
 * `cleanupWorktreeOnFinish` / `retryWorktreeCleanup` / `rollbackStart`，
 * 读方是详情抽屉——据此显示「清理失败 + 重试」，而不是笼统的失败原因。
 * 用 `includes` 判定而不是 `startsWith`：这条附注可能被追加在启动失败原因之后。
 */
export const WORKTREE_CLEANUP_FAILED_PREFIX = '工作树清理失败：'
