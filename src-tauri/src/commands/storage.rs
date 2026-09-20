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

// ------------------------------------------------ Диск: тест записи
//
// Запись на сам физический диск уничтожила бы данные, поэтому пишем
// проверочный файл на выбранный том, читаем обратно с проверкой и удаляем.
// Кэш Windows обходим (NO_BUFFERING + WRITE_THROUGH) — иначе скорость
// показала бы память, а не диск.

static W_STOP: AtomicBool = AtomicBool::new(false);
static W_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct VolumeInfo {
    #[serde(default)]
    pub letter: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub size_gb: f64,
    #[serde(default)]
    pub free_gb: f64,
    #[serde(default)]
    pub fs: String,
    #[serde(default)]
    pub is_system: bool,
}

#[tauri::command]
pub fn list_fixed_volumes() -> Result<Vec<VolumeInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $sys = ($env:SystemDrive).TrimEnd(':')
            $items = @(Get-Volume -ErrorAction SilentlyContinue |
                Where-Object { $_.DriveType -eq 'Fixed' -and $_.DriveLetter } |
                ForEach-Object {
                    [PSCustomObject]@{
                        letter = [string]$_.DriveLetter
                        label = [string]$_.FileSystemLabel
                        size_gb = [math]::Round($_.Size / 1GB, 1)
                        free_gb = [math]::Round($_.SizeRemaining / 1GB, 1)
                        fs = [string]$_.FileSystem
                        is_system = ([string]$_.DriveLetter -eq $sys)
                    }
                })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<VolumeInfo>>(script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct DiskWriteProgress {
    pub pct: u32,
    /// "write" | "read"
    pub phase: String,
    pub mbps: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DiskWriteResult {
    pub letter: String,
    pub size_mb: u64,
    pub write_avg_mbps: f64,
    pub write_min_mbps: f64,
    pub write_max_mbps: f64,
    /// Скорость записи по участкам файла, МБ/с
    pub write_samples: Vec<f64>,
    pub read_mbps: f64,
    /// Блоки 1 МБ, записанные/прочитанные медленнее 250 мс
    pub slow_blocks: u32,
    /// Несовпадения данных при чтении обратно
    pub errors: u64,
    pub stopped: bool,
}

#[tauri::command]
pub async fn run_disk_write_test(window: Window, letter: String, size_mb: u64) -> Result<DiskWriteResult, String> {
    if W_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Тест записи диска уже выполняется".to_string());
    }
    W_STOP.store(false, Ordering::SeqCst);
    let res = tauri::async_runtime::spawn_blocking(move || write_test(window, letter, size_mb))
        .await
        .map_err(|e| format!("Тест записи завершился аварийно: {e}"))
        .and_then(|r| r);
    W_RUNNING.store(false, Ordering::SeqCst);
    res
}

#[tauri::command]
pub fn stop_disk_write_test() {
    W_STOP.store(true, Ordering::SeqCst);
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(target_os = "windows")]
fn write_test(window: Window, letter: String, size_mb: u64) -> Result<DiskWriteResult, String> {
    use crate::powershell::run_ps;
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::Instant;
    use tauri::Emitter;

    const NO_BUFFERING: u32 = 0x2000_0000;
    const WRITE_THROUGH: u32 = 0x8000_0000;
    const BLK: usize = 1 << 20;
    const CHUNK: u64 = 32;

    let letter = letter.trim().trim_end_matches(':').to_uppercase();
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("Некорректная буква тома".to_string());
    }
    let size_mb = size_mb.clamp(128, 512 * 1024);

    // Свободного места должно хватить с запасом — иначе можно забить системный диск.
    let free_mb = run_ps(&format!("(Get-Volume -DriveLetter {letter}).SizeRemaining"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|b| b / (1024 * 1024))
        .ok_or("Не удалось узнать свободное место на томе")?;
    // запас: 512 МБ или 5% тома — чтобы не забить диск под ноль
    let reserve = 512u64.max(free_mb / 20);
    if free_mb < size_mb + reserve {
        return Err(format!("Мало свободного места на {letter}: (доступно {free_mb} МБ, нужно не менее {} МБ)", size_mb + reserve));
    }

    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).trim_end_matches(':').to_uppercase();
    let dir = if letter == sys { std::env::temp_dir() } else { std::path::PathBuf::from(format!(r"{letter}:\")) };
    let path = dir.join(format!("echips_write_test_{}.tmp", std::process::id()));

    let mut raw = vec![0u8; BLK + 4096];
    let off = raw.as_ptr().align_offset(4096);
    let fill = |buf: &mut [u8], block: u64| {
        for (i, c) in buf.chunks_exact_mut(8).enumerate() {
            c.copy_from_slice(&splitmix((block << 32) | i as u64).to_le_bytes());
        }
    };

    let mut run = || -> Result<DiskWriteResult, String> {
        let mut res = DiskWriteResult { letter: letter.clone(), size_mb, ..Default::default() };
        let total_ms = size_mb as f64;

        // --- запись
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .custom_flags(NO_BUFFERING | WRITE_THROUGH)
                .open(&path)
                .map_err(|e| format!("Не удалось создать файл на {letter}: {e}"))?;
            let mut chunk_start = Instant::now();
            let started = Instant::now();
            for b in 0..size_mb {
                if W_STOP.load(Ordering::SeqCst) {
                    res.stopped = true;
                    break;
                }
                let buf = &mut raw[off..off + BLK];
                fill(buf, b);
                let t = Instant::now();
                f.write_all(buf).map_err(|e| format!("Ошибка записи на {letter}: {e}"))?;
                if t.elapsed().as_millis() > 250 {
                    res.slow_blocks += 1;
                }
                if (b + 1) % CHUNK == 0 || b + 1 == size_mb {
                    let n = if (b + 1) % CHUNK == 0 { CHUNK } else { (b + 1) % CHUNK };
                    let mbps = n as f64 / chunk_start.elapsed().as_secs_f64().max(0.001);
                    res.write_samples.push(mbps);
                    chunk_start = Instant::now();
                    let _ = window.emit(
                        "diskw-progress",
                        DiskWriteProgress { pct: ((b + 1) as f64 / total_ms * 50.0) as u32, phase: "write".into(), mbps },
                    );
                }
            }
            let _ = started;
        }
        if !res.write_samples.is_empty() {
            res.write_avg_mbps = res.write_samples.iter().sum::<f64>() / res.write_samples.len() as f64;
            res.write_min_mbps = res.write_samples.iter().cloned().fold(f64::INFINITY, f64::min);
            res.write_max_mbps = res.write_samples.iter().cloned().fold(0.0, f64::max);
        }
        if res.stopped {
            return Ok(res);
        }

        // --- чтение обратно с проверкой
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(NO_BUFFERING)
            .open(&path)
            .map_err(|e| format!("Не удалось открыть проверочный файл: {e}"))?;
        let mut expect = vec![0u8; BLK];
        let read_started = Instant::now();
        for b in 0..size_mb {
            if W_STOP.load(Ordering::SeqCst) {
                res.stopped = true;
                break;
            }
            let buf = &mut raw[off..off + BLK];
            let t = Instant::now();
            f.read_exact(buf).map_err(|e| format!("Ошибка чтения с {letter}: {e}"))?;
            if t.elapsed().as_millis() > 250 {
                res.slow_blocks += 1;
            }
            fill(&mut expect, b);
            if buf != &expect[..] {
                res.errors += 1;
            }
            if (b + 1) % CHUNK == 0 || b + 1 == size_mb {
                let mbps = (b + 1) as f64 / read_started.elapsed().as_secs_f64().max(0.001);
                let _ = window.emit(
                    "diskw-progress",
                    DiskWriteProgress { pct: 50 + ((b + 1) as f64 / total_ms * 50.0) as u32, phase: "read".into(), mbps },
                );
            }
        }
        res.read_mbps = size_mb as f64 / read_started.elapsed().as_secs_f64().max(0.001);
        Ok(res)
    };
    let result = run();
    let _ = std::fs::remove_file(&path);
    result
}

#[cfg(not(target_os = "windows"))]
fn write_test(_window: Window, _letter: String, _size_mb: u64) -> Result<DiskWriteResult, String> {
    Err("Тест записи диска доступен только в Windows-сборке".to_string())
}

// ------------------------------------- Диск: сканирование поверхности
//
// Как «Read» в Victoria: последовательное чтение всего диска (или диапазона)
// блоками с замером времени каждого блока. Данные не меняются и диск не
// изнашивается. Время блока раскладывается по классам задержки, скорость
// уходит в интерфейс в реальном времени для графика.

static S_STOP: AtomicBool = AtomicBool::new(false);
static S_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Clone)]
pub struct SurfaceProgress {
    pub pos_mb: u64,
    pub total_mb: u64,
    /// Скорость за последний интервал, МБ/с
    pub mbps: f64,
    /// Блоки по классам задержки: <5, <20, <50, <150, <500, >=500 мс, ошибка
    pub classes: [u64; 7],
    /// Новые смещения нечитаемых блоков (МБ) с прошлого события
    pub new_bad_mb: Vec<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SurfaceResult {
    pub scanned_mb: u64,
    pub avg_mbps: f64,
    pub min_mbps: f64,
    pub max_mbps: f64,
    pub classes: [u64; 7],
    pub bad_offsets_mb: Vec<u64>,
    pub elapsed_secs: u64,
    pub stopped: bool,
}

#[tauri::command]
pub async fn run_surface_scan(
    window: Window,
    disk_number: u32,
    size_gb: f64,
    start_pct: f64,
    end_pct: f64,
    block_kb: u64,
) -> Result<SurfaceResult, String> {
    if S_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Сканирование поверхности уже выполняется".to_string());
    }
    S_STOP.store(false, Ordering::SeqCst);
    let res = tauri::async_runtime::spawn_blocking(move || surface_scan(window, disk_number, size_gb, start_pct, end_pct, block_kb))
        .await
        .map_err(|e| format!("Сканирование завершилось аварийно: {e}"))
        .and_then(|r| r);
    S_RUNNING.store(false, Ordering::SeqCst);
    res
}

#[tauri::command]
pub fn stop_surface_scan() {
    S_STOP.store(true, Ordering::SeqCst);
}

#[cfg(target_os = "windows")]
fn surface_scan(
    window: Window,
    disk_number: u32,
    size_gb: f64,
    start_pct: f64,
    end_pct: f64,
    block_kb: u64,
) -> Result<SurfaceResult, String> {
    use std::io::{Read, Seek, SeekFrom};
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    const NO_BUFFERING: u32 = 0x2000_0000;
    const MB: u64 = 1024 * 1024;

    let block = (block_kb.clamp(64, 4096) * 1024) / 4096 * 4096;
    let total_bytes = (size_gb * 1024.0 * 1024.0 * 1024.0 * 0.995) as u64;
    let start = ((total_bytes as f64 * start_pct.clamp(0.0, 100.0) / 100.0) as u64) / block * block;
    let end = ((total_bytes as f64 * end_pct.clamp(0.0, 100.0) / 100.0) as u64) / block * block;
    if end <= start + block {
        return Err("Пустой диапазон сканирования".to_string());
    }

    let path = format!(r"\\.\PhysicalDrive{disk_number}");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(NO_BUFFERING)
        .open(&path)
        .map_err(|e| format!("Не удалось открыть {path} (нужны права администратора): {e}"))?;
    file.seek(SeekFrom::Start(start)).map_err(|e| format!("Не удалось встать на начало диапазона: {e}"))?;

    let mut raw = vec![0u8; block as usize + 4096];
    let off = raw.as_ptr().align_offset(4096);
    let total_mb = (end - start) / MB;

    let mut res = SurfaceResult::default();
    let mut classes = [0u64; 7];
    let mut new_bad: Vec<u64> = Vec::new();
    let t0 = Instant::now();
    let mut win_start = Instant::now();
    let mut win_bytes = 0u64;
    let mut pos = start;
    let mut min_mbps = f64::INFINITY;
    let mut max_mbps = 0.0f64;
    let mut samples = 0u64;
    let mut sum_mbps = 0.0f64;

    while pos < end {
        if S_STOP.load(Ordering::SeqCst) {
            res.stopped = true;
            break;
        }
        let buf = &mut raw[off..off + block as usize];
        let t = Instant::now();
        let r = file.read(buf);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let mut step = block;
        match r {
            Ok(0) => break,
            Ok(n) => {
                step = n as u64;
                win_bytes += n as u64;
                let c = if ms < 5.0 { 0 } else if ms < 20.0 { 1 } else if ms < 50.0 { 2 } else if ms < 150.0 { 3 } else if ms < 500.0 { 4 } else { 5 };
                classes[c] += 1;
            }
            Err(_) => {
                classes[6] += 1;
                let mb = pos / MB;
                if res.bad_offsets_mb.len() < 500 {
                    res.bad_offsets_mb.push(mb);
                }
                new_bad.push(mb);
                // перепозиционируемся за проблемный блок
                let _ = file.seek(SeekFrom::Start(pos + block));
            }
        }
        pos += step;

        if win_start.elapsed() >= Duration::from_millis(250) {
            let secs = win_start.elapsed().as_secs_f64().max(0.001);
            let mbps = win_bytes as f64 / MB as f64 / secs;
            if win_bytes > 0 {
                min_mbps = min_mbps.min(mbps);
                max_mbps = max_mbps.max(mbps);
                sum_mbps += mbps;
                samples += 1;
            }
            let _ = window.emit(
                "surface-progress",
                SurfaceProgress { pos_mb: (pos - start) / MB, total_mb, mbps, classes, new_bad_mb: std::mem::take(&mut new_bad) },
            );
            win_start = Instant::now();
            win_bytes = 0;
        }
    }

    // хвост: последнее событие с итоговым положением
    let _ = window.emit(
        "surface-progress",
        SurfaceProgress { pos_mb: (pos.min(end) - start) / MB, total_mb, mbps: 0.0, classes, new_bad_mb: std::mem::take(&mut new_bad) },
    );
    res.scanned_mb = (pos.min(end) - start) / MB;
    res.classes = classes;
    res.elapsed_secs = t0.elapsed().as_secs();
    res.avg_mbps = if samples > 0 { sum_mbps / samples as f64 } else { 0.0 };
    res.min_mbps = if min_mbps.is_finite() { min_mbps } else { 0.0 };
    res.max_mbps = max_mbps;
    Ok(res)
}

#[cfg(not(target_os = "windows"))]
fn surface_scan(_w: Window, _d: u32, _s: f64, _a: f64, _b: f64, _k: u64) -> Result<SurfaceResult, String> {
    Err("Сканирование поверхности доступно только в Windows-сборке".to_string())
}
