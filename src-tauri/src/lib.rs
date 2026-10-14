// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod acp;
mod acp_question;
mod agent;
mod agent_type;
mod engine;
mod fs;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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
            acp::acp_answer_elicitation
        ])
        // ACP 长驻子进程清理：App.vue onBeforeUnmount 不可靠 → Rust 侧退出钩子
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                acp::shutdown();
            }
        });
}
