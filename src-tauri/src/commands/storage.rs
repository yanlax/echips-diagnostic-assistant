// Диск (аналог Victoria): здоровье и износ через Get-PhysicalDisk /
// Get-StorageReliabilityCounter, плюс тест чтения физического диска с замером
// скорости по разным зонам, поиском медленных блоков и ошибок чтения.
// Чтение \\.\PhysicalDriveN требует прав администратора (они есть — см. build.rs).

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Window;

static STOP: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DiskHealth {
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub media: String,
    #[serde(default)]
    pub bus: String,
    #[serde(default)]
    pub size_gb: f64,
    #[serde(default)]
    pub health: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_system: bool,
    #[serde(default)]
    pub temp_c: Option<f64>,
    #[serde(default)]
    pub wear_pct: Option<f64>,
    #[serde(default)]
    pub power_on_hours: Option<u64>,
    #[serde(default)]
    pub read_errors: Option<u64>,
    #[serde(default)]
    pub write_errors: Option<u64>,
}

#[tauri::command]
pub fn get_disk_health() -> Result<Vec<DiskHealth>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $sys = $null
            try { $sys = (Get-Partition -DriveLetter C -ErrorAction Stop).DiskNumber } catch {}
            $items = @(Get-PhysicalDisk | ForEach-Object {
                $d = $_
                $r = $null
                try { $r = $d | Get-StorageReliabilityCounter -ErrorAction Stop } catch {}
                [PSCustomObject]@{
                    number = [int]$d.DeviceId
                    name = [string]$d.FriendlyName
                    media = [string]$d.MediaType
                    bus = [string]$d.BusType
                    size_gb = [math]::Round($d.Size / 1GB, 0)
                    health = [string]$d.HealthStatus
                    status = (@($d.OperationalStatus) -join ',')
                    is_system = ($null -ne $sys -and [int]$d.DeviceId -eq [int]$sys)
                    temp_c = if ($r -and $r.Temperature) { $r.Temperature } else { $null }
                    wear_pct = if ($r) { $r.Wear } else { $null }
                    power_on_hours = if ($r) { $r.PowerOnHours } else { $null }
                    read_errors = if ($r) { $r.ReadErrorsUncorrected } else { $null }
                    write_errors = if ($r) { $r.WriteErrorsUncorrected } else { $null }
                }
            })
            ConvertTo-Json -InputObject $items -Compress -Depth 3
        "#;
        run_ps_json::<Vec<DiskHealth>>(script)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Состояние дисков доступно только в Windows-сборке".to_string())
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct DiskProgress {
    pub pct: u32,
    pub mbps: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DiskReadResult {
    pub avg_mbps: f64,
    pub min_mbps: f64,
    pub max_mbps: f64,
    /// Блоки 1 МБ, прочитанные медленнее 250 мс
    pub slow_blocks: u32,
    pub errors: u32,
    pub error_offsets_mb: Vec<u64>,
    /// Скорость в каждой точке замера (по диску от начала к концу), МБ/с
    pub samples: Vec<f64>,
    pub read_mb: u64,
    pub stopped: bool,
}

#[tauri::command]
pub async fn run_disk_read_test(
    window: Window,
    disk_number: u32,
    size_gb: f64,
    sample_mb: u64,
) -> Result<DiskReadResult, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Тест чтения диска уже выполняется".to_string());
    }
    STOP.store(false, Ordering::SeqCst);
    let res = tauri::async_runtime::spawn_blocking(move || read_test(window, disk_number, size_gb, sample_mb))
        .await
        .map_err(|e| format!("Тест чтения завершился аварийно: {e}"))
        .and_then(|r| r);
    RUNNING.store(false, Ordering::SeqCst);
    res
}

#[tauri::command]
pub fn stop_disk_read_test() {
    STOP.store(true, Ordering::SeqCst);
}

#[cfg(target_os = "windows")]
fn read_test(window: Window, disk_number: u32, size_gb: f64, sample_mb: u64) -> Result<DiskReadResult, String> {
    use std::io::{Read, Seek, SeekFrom};
    use std::time::Instant;
    use tauri::Emitter;

    const MB: u64 = 1024 * 1024;
    const POINTS: u64 = 24;
    let sample = sample_mb.clamp(16, 512) * MB;
    let total = (size_gb * 1024.0 * 1024.0 * 1024.0 * 0.99) as u64;
    if total < sample * 2 {
        return Err("Слишком маленький диск для теста чтения".to_string());
    }

    let path = format!(r"\\.\PhysicalDrive{disk_number}");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|e| format!("Не удалось открыть {path} (нужны права администратора): {e}"))?;

    let mut buf = vec![0u8; MB as usize];
    let mut result = DiskReadResult::default();
    let span = total - sample;

    for i in 0..POINTS {
        if STOP.load(Ordering::SeqCst) {
            result.stopped = true;
            break;
        }
        let pos = (span * i / (POINTS - 1)) / MB * MB;
        if file.seek(SeekFrom::Start(pos)).is_err() {
            result.errors += 1;
            result.error_offsets_mb.push(pos / MB);
            continue;
        }
        let started = Instant::now();
        let mut read_bytes = 0u64;
        let mut off = 0u64;
        while off < sample {
            let t = Instant::now();
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => read_bytes += n as u64,
                Err(_) => {
                    result.errors += 1;
                    if result.error_offsets_mb.len() < 20 {
                        result.error_offsets_mb.push((pos + off) / MB);
                    }
                    // после ошибки перепозиционируемся за проблемный блок
                    let _ = file.seek(SeekFrom::Start(pos + off + MB));
                }
            }
            if t.elapsed().as_millis() > 250 {
                result.slow_blocks += 1;
            }
            off += MB;
        }
        let secs = started.elapsed().as_secs_f64().max(0.001);
        let mbps = read_bytes as f64 / MB as f64 / secs;
        result.samples.push(mbps);
        result.read_mb += read_bytes / MB;
        let _ = window.emit("disk-progress", DiskProgress { pct: ((i + 1) * 100 / POINTS) as u32, mbps });
    }

    if !result.samples.is_empty() {
        result.avg_mbps = result.samples.iter().sum::<f64>() / result.samples.len() as f64;
        result.min_mbps = result.samples.iter().cloned().fold(f64::INFINITY, f64::min);
        result.max_mbps = result.samples.iter().cloned().fold(0.0, f64::max);
    }
    Ok(result)
}

#[cfg(not(target_os = "windows"))]
fn read_test(_window: Window, _disk_number: u32, _size_gb: f64, _sample_mb: u64) -> Result<DiskReadResult, String> {
    Err("Тест чтения диска доступен только в Windows-сборке".to_string())
}
