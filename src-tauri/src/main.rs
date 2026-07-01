// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|a| a == "--dump-usage") {
        tauri_app_lib::dump_usage();
        return;
    }
    tauri_app_lib::run()
}
