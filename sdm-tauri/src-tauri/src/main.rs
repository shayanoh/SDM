// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Chrome's native-messaging host is this same binary: answer one message
    // and exit before Tauri (or its single-instance check) starts.
    let args: Vec<String> = std::env::args().collect();
    if sdm_tauri_lib::is_native_host_launch(&args) {
        std::process::exit(sdm_tauri_lib::run_native_host());
    }
    sdm_tauri_lib::run()
}
