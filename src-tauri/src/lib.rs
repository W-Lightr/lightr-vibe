// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod agent;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            agent::agent_detect_opencode,
            agent::agent_read_opencode_config,
            agent::agent_write_opencode_config,
            agent::agent_read_env_file,
            agent::agent_write_env_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
