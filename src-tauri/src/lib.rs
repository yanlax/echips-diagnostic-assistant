mod commands;
mod powershell;
mod sysutil;
mod winpe;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    winpe::prepare();
    std::panic::set_hook(Box::new(|info| {
        winpe::log(&format!("паника: {info}"));
    }));
    let built = tauri::Builder::default()
        .on_page_load(|_webview, payload| {
            winpe::PAGE_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
            winpe::log(&format!("страница: {:?} {}", payload.event(), payload.url()));
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_info,
            commands::summary::get_hardware_summary,
            commands::hwmon::hwmon_status,
            commands::hwmon::hwmon_start,
            commands::hwmon::hwmon_stop,
            commands::hwmon::hwmon_snapshot,
            commands::hwmon::hwmon_fan_set,
            commands::hwmon::hwmon_fan_default,
            commands::hwmon::hwmon_fan_default_all,
            commands::hwmon::hwmon_install_driver,
            commands::hwmon::hwmon_uninstall_driver,
            commands::activation::get_activation_status,
            commands::activation::run_activation_step,
            commands::activation::open_activation_settings,
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
            commands::stress::start_stress,
            commands::stress::stop_stress,
            commands::stress::get_stress_marker,
            commands::stress::clear_stress_marker,
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
            commands::report::save_report_pdf,
            commands::report::open_containing_folder,
            commands::keyhook::start_win_key_block,
            commands::keyhook::stop_win_key_block,
            commands::update::check_for_update,
            commands::update::download_update,
            commands::techs::fetch_techs,
            commands::techs::techs_token_status,
            commands::techs::techs_save_token,
            commands::techs::techs_clear_token,
            commands::techs::techs_upsert,
            commands::techs::techs_remove,
            commands::upload::submit_report,
            commands::upload::flush_report_queue,
        ])
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            let msg = format!("Не удалось создать окно приложения: {e}");
            winpe::log(&msg);
            winpe::message_box(
                "Echips Hardware Check",
                &format!("{msg}\n\nЕсли это WinPE или Windows без Edge WebView2 — положите рядом с exe папку переносимого WebView2 (см. README, «Работа в WinPE»).\nПодробности: echips-startup.log рядом с exe или во временной папке."),
            );
            return;
        }
    };
    winpe::log("окно создано, приложение запущено");
    winpe::start_watchdog();
    app.run(|_app, event| {
            // Жизненный цикл в лог: если процесс закрывается сам, здесь будет видно кто и с каким кодом.
            match &event {
                tauri::RunEvent::Ready => winpe::log("событие: Ready"),
                tauri::RunEvent::ExitRequested { code, .. } => winpe::log(&format!("событие: ExitRequested code={code:?}")),
                tauri::RunEvent::WindowEvent { label, event: we, .. } => {
                    if matches!(we, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
                        winpe::log(&format!("событие: окно {label}: {we:?}"));
                    }
                }
                _ => {}
            }
            if let tauri::RunEvent::Exit = event {
                winpe::log("событие: Exit");
                commands::hwmon::hwmon_stop();
                commands::keyhook::stop_win_key_block();
                powershell::shutdown();
            }
        });
}
