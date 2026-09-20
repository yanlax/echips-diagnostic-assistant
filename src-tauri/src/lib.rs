mod commands;
mod powershell;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_info,
            commands::system::get_problem_devices,
            commands::battery::get_battery_info,
            commands::hardware::list_usb_devices,
            commands::hardware::list_bluetooth_devices,
            commands::hardware::list_wifi_adapters,
            commands::hardware::scan_wifi_networks,
            commands::hardware::get_fingerprint_sensor,
            commands::sensors::get_thermal_reading,
            commands::stress::run_cpu_stress,
            commands::drivers::find_by_name,
            commands::drivers::find_by_serial_prefix,
            commands::drivers::fetch_public_json,
            commands::drivers::cache_manifest,
            commands::drivers::load_cached_manifest,
            commands::drivers::create_restore_point,
            commands::drivers::download_and_install,
            commands::drivers::restart_system,
            commands::drivers::open_log_folder,
            commands::motherboard::read_board_identity,
            commands::motherboard::write_smbios_identity,
            commands::motherboard::read_audit_log,
            commands::report::save_report_txt,
            commands::report::save_report_json,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
