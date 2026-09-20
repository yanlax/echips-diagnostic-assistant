// Температуры и кулер без сторонних DLL: ACPI-термозоны через WMI.
// На многих ноутбуках эти источники пусты или отдают константу, поэтому
// нереалистичные значения отбрасываем, а экран честно пишет "нет данных".

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TempReading {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub celsius: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FanReading {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub rpm: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SensorsInfo {
    #[serde(default)]
    pub temperatures: Vec<TempReading>,
    #[serde(default)]
    pub fans: Vec<FanReading>,
}

#[tauri::command]
pub fn get_sensors_info() -> Result<SensorsInfo, String> {
    #[cfg(target_os = "windows")]
    {
        // Каждый источник в своём try — недоступность одного (например, root/wmi
        // без прав администратора) не должна ронять остальные.
        let script = r#"
            $temps = @()
            try {
                $temps += @(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop |
                    ForEach-Object {
                        [PSCustomObject]@{ source = 'MSAcpi_ThermalZoneTemperature'; name = [string]$_.InstanceName; celsius = [math]::Round($_.CurrentTemperature / 10.0 - 273.15, 1) }
                    })
            } catch {}
            try {
                $temps += @(Get-CimInstance Win32_PerfFormattedData_Counters_ThermalZoneInformation -ErrorAction Stop |
                    ForEach-Object {
                        [PSCustomObject]@{ source = 'ThermalZoneInformation'; name = [string]$_.Name; celsius = [math]::Round([double]$_.Temperature - 273.15, 1) }
                    })
            } catch {}
            $temps = @($temps | Where-Object { $_.celsius -gt 0 -and $_.celsius -lt 130 })

            $fans = @()
            try {
                $fans = @(Get-CimInstance Win32_Fan -ErrorAction Stop |
                    ForEach-Object {
                        [PSCustomObject]@{ name = [string]$_.Name; status = [string]$_.Status; rpm = $_.DesiredSpeed }
                    })
            } catch {}

            [PSCustomObject]@{ temperatures = $temps; fans = $fans } | ConvertTo-Json -Depth 4 -Compress
        "#;
        // ConvertTo-Json схлопывает массивы из одного элемента в объект — нормализуем.
        let mut v: serde_json::Value = run_ps_json(script)?;
        for key in ["temperatures", "fans"] {
            if let Some(f) = v.get_mut(key) {
                if f.is_object() {
                    *f = serde_json::Value::Array(vec![f.take()]);
                } else if f.is_null() {
                    *f = serde_json::Value::Array(vec![]);
                }
            }
        }
        serde_json::from_value(v).map_err(|e| format!("Не удалось разобрать данные сенсоров: {e}"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Опрос сенсоров доступен только в Windows-сборке".to_string())
    }
}
