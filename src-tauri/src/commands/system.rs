// Определение модели/серийника устройства — тот же WMI-каскад, что и в
// echips-driver-assistant (detect_system_info), плюс базовые данные для
// шапки диагностики (ОС, CPU, ОЗУ).

use crate::powershell::{run_ps, run_ps_json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SystemInfo {
    pub manufacturer: String,
    pub model: String,
    pub serial_number: String,
    pub bios_version: String,
    pub os_version: String,
    pub cpu: String,
    pub ram_total_gb: f64,
}

#[tauri::command]
pub fn get_system_info() -> Result<SystemInfo, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $cs = Get-CimInstance Win32_ComputerSystem
            $bios = Get-CimInstance Win32_BIOS
            $os = Get-CimInstance Win32_OperatingSystem
            $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
            [PSCustomObject]@{
                manufacturer = $cs.Manufacturer
                model = $cs.Model
                serial_number = $bios.SerialNumber
                bios_version = $bios.SMBIOSBIOSVersion
                os_version = $os.Caption
                cpu = $cpu.Name
                ram_total_gb = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
            } | ConvertTo-Json -Compress
        "#;
        run_ps_json::<SystemInfo>(script)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Определение системной информации доступно только в Windows-сборке".to_string())
    }
}

/// Список устройств с ошибкой в диспетчере устройств (Status = 'Error') —
/// быстрый индикатор явных проблем при старте диагностики.
#[tauri::command]
pub fn get_problem_devices() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let raw = run_ps(
            "Get-PnpDevice | Where-Object { $_.Status -eq 'Error' } | \
             Select-Object -ExpandProperty FriendlyName",
        )?;
        Ok(raw.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}
