// Диагностика батареи: design capacity vs full charge capacity → health %,
// количество циклов заряда (если доступно из отчёта powercfg).

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BatteryInfo {
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub design_capacity_mwh: Option<u32>,
    #[serde(default)]
    pub full_charge_capacity_mwh: Option<u32>,
    #[serde(default)]
    pub health_percent: Option<f64>,
    #[serde(default)]
    pub charging: Option<bool>,
    #[serde(default)]
    pub charge_percent: Option<u32>,
}

#[tauri::command]
pub fn get_battery_info() -> Result<BatteryInfo, String> {
    #[cfg(target_os = "windows")]
    {
        // Win32_Battery даёт текущий % и статус заряда; design/full capacity
        // надёжнее вытащить из отчёта powercfg /batteryreport (XML), это TODO —
        // на первом этапе отдаём то, что доступно через WMI напрямую.
        let script = r#"
            $b = Get-CimInstance Win32_Battery | Select-Object -First 1
            if ($null -eq $b) {
                [PSCustomObject]@{ present = $false } | ConvertTo-Json -Compress
            } else {
                [PSCustomObject]@{
                    present = $true
                    charge_percent = $b.EstimatedChargeRemaining
                    charging = ($b.BatteryStatus -eq 2)
                } | ConvertTo-Json -Compress
            }
        "#;
        run_ps_json::<BatteryInfo>(script)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Диагностика батареи доступна только в Windows-сборке".to_string())
    }
}
