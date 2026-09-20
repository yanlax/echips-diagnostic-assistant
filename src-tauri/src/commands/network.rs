// Статус Wi-Fi и Bluetooth адаптеров (Win32_NetworkAdapter).

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct NetworkAdapter {
    #[serde(default)]
    pub name: String,
    /// "wifi" или "bluetooth"
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    /// NetEnabled: включён ли адаптер (для части Bluetooth-устройств null)
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Код ошибки диспетчера устройств, 0 — без ошибок
    #[serde(default)]
    pub error_code: u32,
    /// Строка Get-NetAdapter-подобного статуса: NetConnectionStatus
    #[serde(default)]
    pub connection_status: Option<u32>,
}

#[tauri::command]
pub fn get_network_adapters() -> Result<Vec<NetworkAdapter>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $items = @(Get-CimInstance Win32_NetworkAdapter |
                Where-Object { $_.Name -match 'Bluetooth|Wi-?Fi|Wireless|802\.11|WLAN' } |
                ForEach-Object {
                    $kind = if ($_.Name -match 'Bluetooth') { 'bluetooth' } else { 'wifi' }
                    [PSCustomObject]@{
                        name = [string]$_.Name
                        kind = $kind
                        manufacturer = [string]$_.Manufacturer
                        enabled = $_.NetEnabled
                        error_code = [int]$_.ConfigManagerErrorCode
                        connection_status = $_.NetConnectionStatus
                    }
                })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<NetworkAdapter>>(script)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Список сетевых адаптеров доступен только в Windows-сборке".to_string())
    }
}
