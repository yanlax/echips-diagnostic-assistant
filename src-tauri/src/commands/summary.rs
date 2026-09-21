// Сводка железа "как в CPU-Z": процессор, модули ОЗУ, диски, видеокарты,
// плата, BIOS и тип корпуса. Данные из WMI, драйверы не нужны. По ним экран
// "Системная информация" сверяет железо с профилем модели.

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CpuInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cores: u32,
    #[serde(default)]
    pub threads: u32,
    #[serde(default)]
    pub max_mhz: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RamModule {
    #[serde(default)]
    pub slot: String,
    #[serde(default)]
    pub capacity_gb: f64,
    #[serde(default)]
    pub speed_mhz: u32,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub part_number: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DiskInfo {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub size_gb: f64,
    #[serde(default)]
    pub media: String,
    #[serde(default)]
    pub health: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GpuInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub driver_version: String,
    #[serde(default)]
    pub vram_mb: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct HardwareSummary {
    #[serde(default)]
    pub cpu: CpuInfo,
    #[serde(default)]
    pub ram_modules: Vec<RamModule>,
    #[serde(default)]
    pub ram_total_gb: f64,
    #[serde(default)]
    pub disks: Vec<DiskInfo>,
    #[serde(default)]
    pub gpus: Vec<GpuInfo>,
    #[serde(default)]
    pub board: String,
    #[serde(default)]
    pub bios_version: String,
    #[serde(default)]
    pub bios_date: String,
    /// Ноутбук/моноблок по типу корпуса (SMBIOS ChassisTypes) — на десктопе
    /// отсутствие батареи и Wi-Fi не считается неисправностью.
    #[serde(default)]
    pub is_laptop: bool,
}

#[tauri::command(async)]
pub fn get_hardware_summary() -> Result<HardwareSummary, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
            $ram = @(Get-CimInstance Win32_PhysicalMemory | ForEach-Object {
                $speed = if ($_.ConfiguredClockSpeed) { $_.ConfiguredClockSpeed } else { $_.Speed }
                [PSCustomObject]@{
                    slot = [string]$_.DeviceLocator
                    capacity_gb = [math]::Round($_.Capacity / 1GB, 1)
                    speed_mhz = [int]$speed
                    manufacturer = ([string]$_.Manufacturer).Trim()
                    part_number = ([string]$_.PartNumber).Trim()
                }
            })
            $media = @{}
            try { Get-PhysicalDisk -ErrorAction Stop | ForEach-Object { $media[[string]$_.FriendlyName] = @([string]$_.MediaType, [string]$_.HealthStatus) } } catch {}
            $disks = @(Get-CimInstance Win32_DiskDrive | ForEach-Object {
                $m = $media[[string]$_.Model]
                [PSCustomObject]@{
                    model = [string]$_.Model
                    size_gb = [math]::Round($_.Size / 1GB, 0)
                    media = if ($m) { $m[0] } else { '' }
                    health = if ($m) { $m[1] } else { '' }
                }
            })
            $gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
                [PSCustomObject]@{
                    name = [string]$_.Name
                    driver_version = [string]$_.DriverVersion
                    vram_mb = [math]::Round([double]$_.AdapterRAM / 1MB, 0)
                }
            })
            $bb = Get-CimInstance Win32_BaseBoard | Select-Object -First 1
            $bios = Get-CimInstance Win32_BIOS | Select-Object -First 1
            $chassis = @((Get-CimInstance Win32_SystemEnclosure | Select-Object -First 1).ChassisTypes)
            $laptopTypes = 8, 9, 10, 11, 12, 14, 18, 21, 30, 31, 32
            $isLaptop = $false
            foreach ($t in $chassis) { if ($laptopTypes -contains [int]$t) { $isLaptop = $true } }
            $total = 0
            foreach ($m in $ram) { $total += $m.capacity_gb }
            [PSCustomObject]@{
                cpu = [PSCustomObject]@{
                    name = ([string]$cpu.Name).Trim()
                    cores = [int]$cpu.NumberOfCores
                    threads = [int]$cpu.NumberOfLogicalProcessors
                    max_mhz = [int]$cpu.MaxClockSpeed
                }
                ram_modules = $ram
                ram_total_gb = $total
                disks = $disks
                gpus = $gpus
                board = (([string]$bb.Manufacturer) + ' ' + ([string]$bb.Product)).Trim()
                bios_version = [string]$bios.SMBIOSBIOSVersion
                bios_date = if ($bios.ReleaseDate) { $bios.ReleaseDate.ToString('yyyy-MM-dd') } else { '' }
                is_laptop = $isLaptop
            } | ConvertTo-Json -Depth 5 -Compress
        "#;
        // ConvertTo-Json схлопывает массив из одного элемента в объект — нормализуем.
        let mut v: serde_json::Value = run_ps_json(script)?;
        for key in ["ram_modules", "disks", "gpus"] {
            if let Some(f) = v.get_mut(key) {
                if f.is_object() {
                    *f = serde_json::Value::Array(vec![f.take()]);
                } else if f.is_null() {
                    *f = serde_json::Value::Array(vec![]);
                }
            }
        }
        serde_json::from_value(v).map_err(|e| format!("Не удалось разобрать сводку железа: {e}"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Сводка железа доступна только в Windows-сборке".to_string())
    }
}
