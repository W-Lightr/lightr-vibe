// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod acp;
mod acp_question;
mod agent;
mod agent_type;
mod automation;
mod engine;
mod fs;
mod task;
mod worktree;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 用系统文件管理器打开目录（右键「打开文件资源」）。
/// 直接走 Rust 调 opener，避开插件 open_path 对路径 scope 的要求。
#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|err| err.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        // 目录选择对话框（design D1：前端 @tauri-apps/plugin-dialog open() 调用，
        // capabilities 仅授权 dialog:allow-open）
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            open_folder,
            agent::agent_detect,
            agent::agent_read_config,
            agent::agent_write_config,
            agent::agent_read_env_file,
            agent::agent_write_env_file,
            engine::acp_list_agents,
            engine::engine_default_get,
            engine::engine_default_set,
            fs::fs_list_dir,
            fs::fs_read_file,
            fs::fs_write_file,
            fs::fs_read_text,
            fs::fs_write_text,
            task::task_list,
            task::task_create,
            task::task_update,
            task::task_delete,
            task::task_transition,
            task::task_reorder,
            task::task_attach_session,
            task::task_archive,
            task::task_events,
            task::task_settle_turn,
            task::task_detach_session,
            task::task_duplicate,
            task::task_attach_worktree,
            task::task_clear_worktree,
            task::task_worktree_issue,
            // git worktree 隔离（task 10-26-worktree-backend）：全仓唯一调用 git 的模块
            worktree::worktree_repo_info,
            worktree::worktree_create,
            worktree::worktree_commit_all,
            worktree::worktree_remove,
            acp::acp_connect,
            acp::acp_disconnect,
            acp::acp_status,
            acp::acp_probe_options,
            acp::agent_model_context_window,
            acp::acp_session_new,
            acp::acp_session_load,
            acp::acp_prompt,
            acp::acp_cancel,
            acp::acp_session_close,
            acp::acp_set_config,
            acp::acp_respond_permission,
            acp::acp_answer_elicitation,
            // 自动化（task 10-26-automation-scheduler）：CRUD + 调度引擎
            automation::automation_list,
            automation::automation_get,
            automation::automation_create,
            automation::automation_update,
            automation::automation_set_enabled,
            automation::automation_delete,
            automation::automation_mark_seen,
            automation::automation_compute_next_run,
            automation::automation_runs,
            automation::automation_run_status,
            automation::automation_run_now,
            automation::automation_cancel_run,
            automation::automation_run_status_values
        ])
        .setup(|app| {
            // 自动化调度引擎：拿独占锁 -> boot 恢复 -> spawn 主循环。
            // 拿不到锁时不驱动（CRUD 仍可用），见 design 5.1/5.2。
            automation::engine::boot_engine(app.handle().clone());
            Ok(())
        })
        // ACP 长驻子进程清理：App.vue onBeforeUnmount 不可靠 → Rust 侧退出钩子
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                acp::shutdown();
            }
        });
}
