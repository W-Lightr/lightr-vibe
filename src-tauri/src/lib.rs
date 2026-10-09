// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod agent;
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
            agent::agent_detect_opencode,
            agent::agent_read_opencode_config,
            agent::agent_write_opencode_config,
            agent::agent_read_env_file,
            agent::agent_write_env_file,
            fs::fs_list_dir,
            fs::fs_read_file,
            fs::fs_write_file,
            fs::fs_read_text,
            fs::fs_write_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
