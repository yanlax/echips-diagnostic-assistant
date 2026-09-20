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
    /// Видеокарта NVIDIA через nvidia-smi (поставляется с драйвером, сторонние
    /// библиотеки не нужны). Для AMD/Intel-видео данных нет.
    pub gpu: Option<GpuSensor>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GpuSensor {
    pub name: String,
    pub temp_c: f64,
    pub fan_pct: Option<f64>,
    pub power_w: Option<f64>,
    pub util_pct: Option<f64>,
}

#[cfg(target_os = "windows")]
fn read_nvidia() -> Option<GpuSensor> {
    let raw = run_ps(
        "nvidia-smi --query-gpu=name,temperature.gpu,fan.speed,power.draw,utilization.gpu --format=csv,noheader,nounits 2>$null; exit 0",
    )
    .ok()?;
    let line = raw.lines().next()?.trim().to_string();
    let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
    if parts.len() < 5 {
        return None;
    }
    let num = |s: &str| s.parse::<f64>().ok();
    Some(GpuSensor {
        name: parts[0].to_string(),
        temp_c: num(parts[1])?,
        fan_pct: num(parts[2]),
        power_w: num(parts[3]),
        util_pct: num(parts[4]),
    })
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
        let gpu = read_nvidia();
        match trimmed.parse::<f64>() {
            Ok(celsius) if celsius > -50.0 && celsius < 150.0 => Ok(SensorReading {
                available: true,
                cpu_temp_c: Some(celsius),
                note: "ACPI thermal zone (WMI) — может не отражать реальную температуру CPU/GPU на всех платах".into(),
                gpu,
            }),
            _ => Ok(SensorReading {
                available: false,
                cpu_temp_c: None,
                note: "Плата не публикует ACPI-датчики через WMI. Для точных данных нужен \
                       LibreHardwareMonitor/HWInfo (не интегрирован в это приложение)."
                    .into(),
                gpu,
            }),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}
