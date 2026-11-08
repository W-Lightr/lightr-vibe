fn main() {
    println!("cargo:rerun-if-changed=src/bin/lightr_ask_mcp.rs");
    tauri_build::build()
}
