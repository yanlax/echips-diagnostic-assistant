// Диагностика батареи: текущий заряд из WMI + design/full-charge capacity и
// health% из XML-отчёта powercfg (единственный надёжный источник этих цифр
// на большинстве ноутбуков).

use crate::powershell::{run_ps, run_ps_json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BatteryInfo {
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub charge_percent: Option<u32>,
    #[serde(default)]
    pub charging: Option<bool>,
    #[serde(default)]
    pub design_capacity_mwh: Option<u64>,
    #[serde(default)]
    pub full_charge_capacity_mwh: Option<u64>,
    #[serde(default)]
    pub health_percent: Option<f64>,
    #[serde(default)]
    pub cycle_count: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct WmiBattery {
    #[serde(default)]
    present: bool,
    #[serde(default)]
    charge_percent: Option<u32>,
    #[serde(default)]
    charging: Option<bool>,
}

#[tauri::command(async)]
pub fn get_battery_info() -> Result<BatteryInfo, String> {
    #[cfg(target_os = "windows")]
    {
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
        let wmi: WmiBattery = run_ps_json(script)?;
        if !wmi.present {
            return Ok(BatteryInfo { present: false, ..Default::default() });
        }

        let mut info = BatteryInfo {
            present: true,
            charge_percent: wmi.charge_percent,
            charging: wmi.charging,
            ..Default::default()
        };

        // powercfg /batteryreport пишет файл на диск — читаем и разбираем его
        // как XML. Если по какой-то причине это не сработало (батарея
        // виртуальная/ноутбук без поддержки отчёта), просто не заполняем
        // health% — это не критическая ошибка.
        if let Ok((design, full, cycles)) = read_battery_report() {
            info.design_capacity_mwh = design;
            info.full_charge_capacity_mwh = full;
            info.cycle_count = cycles;
            if let (Some(d), Some(f)) = (design, full) {
                if d > 0 {
                    info.health_percent = Some((f as f64 / d as f64 * 100.0 * 10.0).round() / 10.0);
                }
            }
        }

        Ok(info)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Диагностика батареи доступна только в Windows-сборке".to_string())
    }
}

#[cfg(target_os = "windows")]
fn read_battery_report() -> Result<(Option<u64>, Option<u64>, Option<u32>), String> {
    let dir = std::env::temp_dir();
    let path = dir.join("echips_battery_report.xml");
    let path_str = path.to_string_lossy().replace('\'', "''");

    let script = format!("powercfg /batteryreport /xml /output '{path_str}' | Out-Null");
    run_ps(&script)?;

    let xml = std::fs::read_to_string(&path).map_err(|e| format!("Не удалось прочитать отчёт powercfg: {e}"))?;
    let _ = std::fs::remove_file(&path);

    let design = extract_xml_tag_u64(&xml, "DesignCapacity");
    let full = extract_xml_tag_u64(&xml, "FullChargeCapacity");
    let cycles = extract_xml_tag_u64(&xml, "CycleCount").map(|v| v as u32);

    if design.is_none() && full.is_none() {
        return Err("Отчёт powercfg не содержит данных о ёмкости".to_string());
    }
    Ok((design, full, cycles))
}

/// Простой парсер одного числового тега без внешних XML-зависимостей —
/// формат отчёта powercfg стабилен и достаточно прост для этого.
#[cfg(target_os = "windows")]
fn extract_xml_tag_u64(xml: &str, tag: &str) -> Option<u64> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    xml[start..end].trim().parse::<u64>().ok()
}
