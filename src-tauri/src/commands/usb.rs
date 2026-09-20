// Список подключённых USB-устройств (Win32_PnPEntity с PNPDeviceID USB\*).
// Инженер втыкает флешку в порт и обновляет список — устройство должно
// появиться; ручную отметку по портам ведёт фронтенд.

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UsbDevice {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub is_hub: bool,
}

#[tauri::command]
pub fn list_usb_devices() -> Result<Vec<UsbDevice>, String> {
    #[cfg(target_os = "windows")]
    {
        // @() + -InputObject гарантируют JSON-массив даже для 0/1 устройства.
        let script = r#"
            $items = @(Get-CimInstance Win32_PnPEntity |
                Where-Object { $_.PNPDeviceID -like 'USB\*' } |
                ForEach-Object {
                    [PSCustomObject]@{
                        name = [string]$_.Name
                        manufacturer = [string]$_.Manufacturer
                        device_id = [string]$_.PNPDeviceID
                        status = [string]$_.Status
                        is_hub = ($_.Name -match 'hub|концентратор')
                    }
                })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<UsbDevice>>(script)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Список USB-устройств доступен только в Windows-сборке".to_string())
    }
}
