// 自动化（定时任务）模块：把一条保存好的提示词按 cron 或手动触发，每次触发产出一条运行记录。
//
// 分层（design.md §1）：
// - `store`   — automations.json 落盘（与 task.rs 的 TaskStore 同构）
// - `cron`    — 纯函数 cron 求值（自实现，零新依赖）
// - `service` — 纯业务：校验 / CAS 抢占 / start / settle / prune（不感知 tokio）
// - `engine`  — 调度循环 + fire 锁 + 两条 launch 分支 + 取消 + 对账
// - `commands`— #[tauri::command] 薄壳
//
// 三个保留自 codeg 的不变量（design §2）：
// 1. CAS 抢占时间片：多进程 / 重启都不会漏触发或重触发
// 2. fire 锁串行化「查重 + 插入 + 启动」：手动与定时撞车不会起两条
// 3. 独占文件锁证明自己是唯一引擎：才敢在启动时把 running 一律判死

pub mod commands;
pub mod cron;
pub mod engine;
pub mod service;
pub mod store;

pub use commands::*;
pub use engine::engine;
