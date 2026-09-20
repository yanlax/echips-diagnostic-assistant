// Датчики температур/оборотов. В отличие от дизайн-прототипа (там график
// рисуется по случайным числам для наглядности макета), здесь — попытка
// реального чтения через WMI (MSAcpi_ThermalZoneTemperature), и честный
// "недоступно", если плата эти данные не отдаёт — на большинстве
// потребительских ноутбуков ACPI-датчики через WMI либо не публикуются
// вендором, либо дают одно фиксированное число. Живой мониторинг уровня
// HWInfo/LibreHardwareMonitor требует либо интеграции с их библиотекой,
// либо доступа к SMBus/EC напрямую — это отдельная задача, отмечена в
// README как TODO.

use crate::powershell::run_ps;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SensorReading {
    pub available: bool,
    pub cpu_temp_c: Option<f64>,
    pub note: String,
}

#[tauri::command]
pub fn get_thermal_reading() -> Result<SensorReading, String> {
    #[cfg(target_os = "windows")]
    {
        // Значение в MSAcpi_ThermalZoneTemperature — в десятых долях кельвина.
        let raw = run_ps(
            "try { \
               $t = Get-CimInstance -Namespace 'root/wmi' -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop | Select-Object -First 1; \
               if ($t) { [math]::Round(($t.CurrentTemperature / 10) - 273.15, 1) } \
             } catch { }",
        )?;
        let trimmed = raw.trim();
        match trimmed.parse::<f64>() {
            Ok(celsius) if celsius > -50.0 && celsius < 150.0 => Ok(SensorReading {
                available: true,
                cpu_temp_c: Some(celsius),
                note: "ACPI thermal zone (WMI) — может не отражать реальную температуру CPU/GPU на всех платах".into(),
            }),
            _ => Ok(SensorReading {
                available: false,
                cpu_temp_c: None,
                note: "Плата не публикует ACPI-датчики через WMI. Для точных данных нужен \
                       LibreHardwareMonitor/HWInfo (не интегрирован в это приложение)."
                    .into(),
            }),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}
