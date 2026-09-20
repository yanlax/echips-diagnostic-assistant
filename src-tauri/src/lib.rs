mod commands;
mod powershell;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_info,
            commands::summary::get_hardware_summary,
            commands::storage::get_disk_health,
            commands::smart::get_smart_report,
            commands::peripherals::list_lan_adapters,
            commands::peripherals::scan_wifi_detailed,
            commands::peripherals::list_monitors,
            commands::peripherals::get_brightness,
            commands::peripherals::set_brightness,
            commands::peripherals::list_removable_drives,
            commands::peripherals::test_removable_drive,
            commands::storage::run_disk_read_test,
            commands::storage::stop_disk_read_test,
            commands::storage::list_fixed_volumes,
            commands::storage::run_disk_write_test,
            commands::storage::stop_disk_write_test,
            commands::storage::run_surface_scan,
            commands::storage::stop_surface_scan,
            commands::crash::get_crash_history,
            commands::memtest::run_memory_test,
            commands::memtest::stop_memory_test,
            commands::system::get_problem_devices,
            commands::system::list_problem_devices,
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
            commands::drivers::yandex_list_folder,
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
