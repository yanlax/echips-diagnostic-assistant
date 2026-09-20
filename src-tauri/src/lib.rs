mod commands;
mod powershell;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_info,
            commands::battery::get_battery_info,
            commands::report::save_report,
            commands::usb::list_usb_devices,
            commands::network::get_network_adapters,
            commands::fingerprint::get_biometric_devices,
            commands::stress::run_cpu_stress,
            commands::stress::stop_cpu_stress,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
