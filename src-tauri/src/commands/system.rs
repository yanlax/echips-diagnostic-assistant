// Определение модели/серийника устройства — тот же каскадный подход
// (WMI Win32_ComputerSystem / Win32_BIOS), что и в echips-driver-assistant,
// нужен здесь для привязки отчёта диагностики к конкретному устройству.

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
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
