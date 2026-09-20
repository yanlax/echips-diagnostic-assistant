// Проверка наличия сенсора отпечатков пальца (PnP-устройства класса Biometric).

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BiometricDevice {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error_code: u32,
}

#[tauri::command]
pub fn get_biometric_devices() -> Result<Vec<BiometricDevice>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $items = @(Get-CimInstance Win32_PnPEntity |
                Where-Object { $_.PNPClass -eq 'Biometric' } |
                ForEach-Object {
                    [PSCustomObject]@{
                        name = [string]$_.Name
                        manufacturer = [string]$_.Manufacturer
                        status = [string]$_.Status
                        error_code = [int]$_.ConfigManagerErrorCode
                    }
                })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<BiometricDevice>>(script)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Проверка биометрии доступна только в Windows-сборке".to_string())
    }
}
