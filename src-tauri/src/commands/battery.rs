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
    #[serde(default)]
    pub cycle_count: Option<u32>,
}

#[tauri::command]
pub fn get_battery_info() -> Result<BatteryInfo, String> {
    #[cfg(target_os = "windows")]
    {
        // Win32_Battery даёт текущий % и статус заряда; design/full capacity и
        // число циклов берём из XML-отчёта powercfg /batteryreport. Если отчёт
        // не построился (нет прав и т.п.) — отдаём только данные WMI.
        let script = r#"
            $b = Get-CimInstance Win32_Battery | Select-Object -First 1
            if ($null -eq $b) {
                [PSCustomObject]@{ present = $false } | ConvertTo-Json -Compress
            } else {
                $design = $null; $full = $null; $health = $null; $cycles = $null
                $path = Join-Path $env:TEMP 'echips-battery-report.xml'
                try {
                    Remove-Item $path -ErrorAction SilentlyContinue
                    powercfg /batteryreport /xml /output $path | Out-Null
                    [xml]$x = Get-Content -LiteralPath $path -Encoding UTF8
                    $bat = @($x.BatteryReport.Batteries.Battery) | Select-Object -First 1
                    if ($bat) {
                        $design = [int]$bat.DesignCapacity
                        $full = [int]$bat.FullChargeCapacity
                        if ($bat.CycleCount) { $cycles = [int]$bat.CycleCount }
                        if ($design -gt 0) { $health = [math]::Round($full * 100.0 / $design, 1) }
                    }
                } catch {}
                Remove-Item $path -ErrorAction SilentlyContinue
                [PSCustomObject]@{
                    present = $true
                    charge_percent = $b.EstimatedChargeRemaining
                    charging = ($b.BatteryStatus -eq 2)
                    design_capacity_mwh = $design
                    full_charge_capacity_mwh = $full
                    health_percent = $health
                    cycle_count = $cycles
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
