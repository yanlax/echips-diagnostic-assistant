// Стресс-тест в духе AIDA64 System Stability Test: выбираемые виды нагрузки
// (CPU целочисленная, FPU — с AVX/FMA где есть, кэш, память с проверкой, диск с
// проверкой), живые метрики раз в секунду (событие "stress-tick"), остановка в
// любой момент, температурная защита и маркер обрыва: если Windows перезагрузилась
// или зависла посреди теста, при следующем запуске приложение это покажет.
// GPU-нагрузка делается на стороне интерфейса (WebGL), здесь только температура
// видеокарты (nvidia-smi через sensors.rs).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Window};

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct StressConfig {
    /// 0 — до остановки вручную
    pub duration_secs: u64,
    pub cpu: bool,
    pub fpu: bool,
    pub cache: bool,
    pub memory: bool,
    pub disk: bool,
    /// Нагрузку на GPU создаёт интерфейс (WebGL); ядро только собирает метрики
    pub gpu: bool,
    /// Сколько потоков отдать процессорным нагрузкам (0 — все логические)
    pub threads: u32,
    /// Доля свободной ОЗУ под тест памяти, %
    pub memory_percent: u32,
    /// Буква тома для дисковой нагрузки (пусто — системный, файл в %TEMP%)
    pub disk_letter: String,
    pub disk_mb: u64,
    /// Порог температурной защиты, °C (0 — выключена)
    pub max_temp_c: f64,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StressTick {
    pub elapsed: u64,
    pub duration: u64,
    pub load: f64,
    pub clock_mhz: f64,
    pub clock_max_mhz: f64,
    pub temp_c: Option<f64>,
    pub gpu_temp_c: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub power_w: Option<f64>,
    /// Скорость каждой нагрузки за последнюю секунду (см. unit в итоге)
    pub scores: BTreeMap<String, f64>,
    pub mem_errors: u64,
    pub disk_errors: u64,
    pub events: Vec<String>,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StressorStat {
    pub name: String,
    pub unit: String,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    /// Базовая скорость (медиана первых секунд) и худшее отношение к ней
    pub baseline: f64,
    pub min_ratio: f64,
    pub throttled_secs: u32,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StressResult {
    /// "completed" | "stopped" | "thermal" | "error"
    pub reason: String,
    pub elapsed_secs: u64,
    pub threads: usize,
    pub stressors: Vec<StressorStat>,
    pub max_temp_c: Option<f64>,
    pub max_gpu_temp_c: Option<f64>,
    pub avg_load: f64,
    pub clock_avg_mhz: f64,
    pub clock_min_mhz: f64,
    pub clock_max_mhz: f64,
    pub mem_errors: u64,
    pub disk_errors: u64,
    pub log: Vec<String>,
}

// ------------------------------------------------------------------
//  Нагрузки
// ------------------------------------------------------------------

fn work_cpu(stop: &AtomicBool, counter: &AtomicU64) {
    crate::sysutil::lower_thread_priority();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut y: u64 = 1;
    while !stop.load(Ordering::Relaxed) {
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            y = y.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(x).rotate_left(23);
        }
        black_box((x, y));
        counter.fetch_add(200_000 * 8, Ordering::Relaxed);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn fpu_avx(stop: &AtomicBool, counter: &AtomicU64) {
    use std::arch::x86_64::*;
    let m = _mm256_set1_pd(0.999_999_9);
    let c = _mm256_set1_pd(0.001);
    let (mut v0, mut v1, mut v2, mut v3) = (_mm256_set1_pd(1.0), _mm256_set1_pd(1.1), _mm256_set1_pd(1.2), _mm256_set1_pd(1.3));
    let (mut v4, mut v5, mut v6, mut v7) = (_mm256_set1_pd(1.4), _mm256_set1_pd(1.5), _mm256_set1_pd(1.6), _mm256_set1_pd(1.7));
    while !stop.load(Ordering::Relaxed) {
        for _ in 0..50_000 {
            v0 = _mm256_fmadd_pd(v0, m, c);
            v1 = _mm256_fmadd_pd(v1, m, c);
            v2 = _mm256_fmadd_pd(v2, m, c);
            v3 = _mm256_fmadd_pd(v3, m, c);
            v4 = _mm256_fmadd_pd(v4, m, c);
            v5 = _mm256_fmadd_pd(v5, m, c);
            v6 = _mm256_fmadd_pd(v6, m, c);
            v7 = _mm256_fmadd_pd(v7, m, c);
        }
        // 8 цепочек × 4 числа × 2 операции (умножение + сложение) × повторов
        counter.fetch_add(50_000 * 8 * 4 * 2, Ordering::Relaxed);
    }
    let mut sink = [0f64; 4];
    let sum = _mm256_add_pd(_mm256_add_pd(_mm256_add_pd(v0, v1), _mm256_add_pd(v2, v3)), _mm256_add_pd(_mm256_add_pd(v4, v5), _mm256_add_pd(v6, v7)));
    _mm256_storeu_pd(sink.as_mut_ptr(), sum);
    black_box(sink);
}

fn work_fpu(stop: &AtomicBool, counter: &AtomicU64) {
    crate::sysutil::lower_thread_priority();
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            unsafe { fpu_avx(stop, counter) };
            return;
        }
    }
    let mut x: f64 = 1.000_000_1;
    while !stop.load(Ordering::Relaxed) {
        for _ in 0..100_000 {
            x = (x * 1.000_000_1 + 0.5).sqrt().sin().abs() + 1.0;
        }
        black_box(x);
        counter.fetch_add(100_000 * 4, Ordering::Relaxed);
    }
}

fn work_cache(stop: &AtomicBool, counter: &AtomicU64) {
    crate::sysutil::lower_thread_priority();
    // рабочие наборы под L1/L2/L3 и чуть больше
    let sizes = [64 * 1024usize, 512 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024];
    let mut bufs: Vec<Vec<u64>> = sizes.iter().map(|s| vec![1u64; s / 8]).collect();
    while !stop.load(Ordering::Relaxed) {
        for b in bufs.iter_mut() {
            let mut acc = 0u64;
            let mut i = 0;
            while i < b.len() {
                acc = acc.wrapping_add(b[i]);
                b[i] = acc ^ (i as u64);
                i += 8; // шаг в одну строку кэша (64 байта)
            }
            black_box(acc);
            counter.fetch_add((b.len() * 8) as u64, Ordering::Relaxed);
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
    }
}

fn work_memory(stop: &AtomicBool, counter: &AtomicU64, errors: &AtomicU64, log: &Mutex<Vec<String>>, size_mb: u64) {
    crate::sysutil::lower_thread_priority();
    let len = (size_mb * 1024 * 1024 / 8) as usize;
    let mut mem: Vec<u64> = Vec::new();
    if mem.try_reserve_exact(len).is_err() {
        if let Ok(mut l) = log.lock() {
            l.push(format!("Память: не удалось выделить {size_mb} МБ"));
        }
        return;
    }
    mem.resize(len, 0);
    let patterns = [0xAAAA_AAAA_AAAA_AAAAu64, 0x5555_5555_5555_5555, u64::MAX, 0, 0x0F0F_0F0F_0F0F_0F0F, 0xF0F0_F0F0_F0F0_F0F0];
    const CHUNK: usize = 1 << 17;
    let mut round = 0usize;
    while !stop.load(Ordering::Relaxed) {
        let pat = patterns[round % patterns.len()] ^ (round as u64 / patterns.len() as u64);
        round += 1;
        for chunk in mem.chunks_mut(CHUNK) {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            for v in chunk.iter_mut() {
                unsafe { std::ptr::write_volatile(v, pat) };
            }
            counter.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
        }
        for (ci, chunk) in mem.chunks(CHUNK).enumerate() {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            for (i, v) in chunk.iter().enumerate() {
                let got = unsafe { std::ptr::read_volatile(v) };
                if got != pat {
                    let n = errors.fetch_add(1, Ordering::Relaxed);
                    if n < 5 {
                        if let Ok(mut l) = log.lock() {
                            l.push(format!("ОШИБКА ПАМЯТИ: смещение {:#x}, записано {:#018x}, прочитано {:#018x}", (ci * CHUNK + i) * 8, pat, got));
                        }
                    }
                }
            }
            counter.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
        }
    }
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(target_os = "windows")]
fn work_disk(stop: &AtomicBool, counter: &AtomicU64, errors: &AtomicU64, log: &Mutex<Vec<String>>, letter: &str, size_mb: u64) {
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    crate::sysutil::lower_thread_priority();
    const NO_BUFFERING: u32 = 0x2000_0000;
    const WRITE_THROUGH: u32 = 0x8000_0000;
    const BLK: usize = 1 << 20;
    let size_mb = size_mb.clamp(64, 4096);
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).trim_end_matches(':').to_uppercase();
    let l = letter.trim().trim_end_matches(':').to_uppercase();
    let dir = if l.is_empty() || l == sys { std::env::temp_dir() } else { std::path::PathBuf::from(format!(r"{l}:\")) };
    let path = dir.join(format!("echips_stress_{}.tmp", std::process::id()));
    let mut raw = vec![0u8; BLK + 4096];
    let off = raw.as_ptr().align_offset(4096);
    let mut expect = vec![0u8; BLK];
    let fill = |buf: &mut [u8], block: u64, salt: u64| {
        for (i, c) in buf.chunks_exact_mut(8).enumerate() {
            c.copy_from_slice(&splitmix((block << 32) ^ i as u64 ^ salt).to_le_bytes());
        }
    };
    let mut salt = 0u64;
    'outer: while !stop.load(Ordering::Relaxed) {
        salt += 1;
        {
            let mut f = match std::fs::OpenOptions::new().write(true).create(true).truncate(true).custom_flags(NO_BUFFERING | WRITE_THROUGH).open(&path) {
                Ok(f) => f,
                Err(e) => {
                    if let Ok(mut lg) = log.lock() {
                        lg.push(format!("Диск: не удалось создать файл нагрузки: {e}"));
                    }
                    break 'outer;
                }
            };
            for b in 0..size_mb {
                if stop.load(Ordering::Relaxed) {
                    break 'outer;
                }
                let buf = &mut raw[off..off + BLK];
                fill(buf, b, salt);
                if f.write_all(buf).is_err() {
                    errors.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut lg) = log.lock() {
                        lg.push("ОШИБКА ДИСКА: сбой записи".to_string());
                    }
                    break 'outer;
                }
                counter.fetch_add(BLK as u64, Ordering::Relaxed);
            }
        }
        let mut f = match std::fs::OpenOptions::new().read(true).custom_flags(NO_BUFFERING).open(&path) {
            Ok(f) => f,
            Err(_) => break,
        };
        for b in 0..size_mb {
            if stop.load(Ordering::Relaxed) {
                break 'outer;
            }
            let buf = &mut raw[off..off + BLK];
            if f.read_exact(buf).is_err() {
                errors.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut lg) = log.lock() {
                    lg.push("ОШИБКА ДИСКА: сбой чтения".to_string());
                }
                break 'outer;
            }
            fill(&mut expect, b, salt);
            if buf != &expect[..] {
                let n = errors.fetch_add(1, Ordering::Relaxed);
                if n < 5 {
                    if let Ok(mut lg) = log.lock() {
                        lg.push(format!("ОШИБКА ДИСКА: данные не совпали в блоке {b}"));
                    }
                }
            }
            counter.fetch_add(BLK as u64, Ordering::Relaxed);
        }
    }
    let _ = std::fs::remove_file(&path);
}

#[cfg(not(target_os = "windows"))]
fn work_disk(_s: &AtomicBool, _c: &AtomicU64, _e: &AtomicU64, log: &Mutex<Vec<String>>, _l: &str, _m: u64) {
    if let Ok(mut lg) = log.lock() {
        lg.push("Диск: нагрузка доступна только в Windows-сборке".to_string());
    }
}

// ------------------------------------------------------------------
//  Системные метрики
// ------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn cpu_times() -> Option<(u64, u64, u64)> {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Ft {
        lo: u32,
        hi: u32,
    }
    extern "system" {
        fn GetSystemTimes(idle: *mut Ft, kernel: *mut Ft, user: *mut Ft) -> i32;
    }
    let (mut i, mut k, mut u) = (Ft::default(), Ft::default(), Ft::default());
    if unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } == 0 {
        return None;
    }
    let f = |t: Ft| ((t.hi as u64) << 32) | t.lo as u64;
    Some((f(i), f(k), f(u)))
}

#[cfg(not(target_os = "windows"))]
fn cpu_times() -> Option<(u64, u64, u64)> {
    None
}

/// (средняя текущая частота, максимальная), МГц — CallNtPowerInformation(ProcessorInformation).
#[cfg(target_os = "windows")]
fn cpu_clock() -> Option<(f64, f64)> {
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Ppi {
        number: u32,
        max_mhz: u32,
        cur_mhz: u32,
        limit_mhz: u32,
        max_idle: u32,
        cur_idle: u32,
    }
    #[link(name = "powrprof")]
    extern "system" {
        fn CallNtPowerInformation(level: i32, inbuf: *mut std::ffi::c_void, insize: u32, outbuf: *mut std::ffi::c_void, outsize: u32) -> i32;
    }
    const PROCESSOR_INFORMATION: i32 = 11;
    let n = num_cpus::get();
    let mut v = vec![Ppi::default(); n];
    let st = unsafe {
        CallNtPowerInformation(PROCESSOR_INFORMATION, std::ptr::null_mut(), 0, v.as_mut_ptr() as *mut std::ffi::c_void, (n * std::mem::size_of::<Ppi>()) as u32)
    };
    if st != 0 {
        return None;
    }
    let cur = v.iter().map(|p| p.cur_mhz as f64).sum::<f64>() / n as f64;
    let max = v.iter().map(|p| p.max_mhz as f64).fold(0.0, f64::max);
    Some((cur, max))
}

#[cfg(not(target_os = "windows"))]
fn cpu_clock() -> Option<(f64, f64)> {
    None
}

// ------------------------------------------------------------------
//  Маркер обрыва
// ------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StressMarker {
    pub started_at: String,
    pub duration_secs: u64,
    pub stressors: Vec<String>,
    pub last_elapsed: u64,
    pub last_temp_c: Option<f64>,
    pub last_gpu_temp_c: Option<f64>,
    pub last_load: f64,
}

fn marker_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck").join("stress_running.json")
}

fn write_marker(m: &StressMarker) {
    let p = marker_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string(m) {
        let _ = std::fs::write(p, s);
    }
}

fn remove_marker() {
    let _ = std::fs::remove_file(marker_path());
}

/// Возвращает маркер, если прошлый стресс-тест не завершился штатно
/// (перезагрузка, зависание, выключение питания посреди теста).
#[tauri::command(async)]
pub fn get_stress_marker() -> Option<StressMarker> {
    let s = std::fs::read_to_string(marker_path()).ok()?;
    serde_json::from_str(&s).ok()
}

#[tauri::command(async)]
pub fn clear_stress_marker() {
    remove_marker();
}

// ------------------------------------------------------------------
//  Управление
// ------------------------------------------------------------------

#[tauri::command(async)]
pub fn start_stress(window: Window, cfg: StressConfig) -> Result<(), String> {
    if !(cfg.cpu || cfg.fpu || cfg.cache || cfg.memory || cfg.disk || cfg.gpu) {
        return Err("Не выбрано ни одной нагрузки (CPU, FPU, кэш, память, диск, GPU)".to_string());
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Стресс-тест уже выполняется".to_string());
    }
    STOP.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        run_session(window, cfg);
        RUNNING.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
pub fn stop_stress() {
    STOP.store(true, Ordering::SeqCst);
}

fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    s[s.len() / 2]
}

struct Load {
    name: &'static str,
    unit: &'static str,
    /// делитель счётчика в единицы за секунду
    div: f64,
    counter: Arc<AtomicU64>,
    prev: u64,
    hist: Vec<f64>,
    throttled: u32,
    min_ratio: f64,
}

fn run_session(window: Window, cfg: StressConfig) {
    crate::sysutil::raise_thread_priority();
    let stop = Arc::new(AtomicBool::new(false));
    let mem_errors = Arc::new(AtomicU64::new(0));
    let disk_errors = Arc::new(AtomicU64::new(0));
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
    let mut loads: Vec<Load> = Vec::new();

    let cores = num_cpus::get().max(1);
    let cpu_threads = if cfg.threads == 0 { cores } else { (cfg.threads as usize).min(cores).max(1) };
    let mk = |name: &'static str, unit: &'static str, div: f64| Load { name, unit, div, counter: Arc::new(AtomicU64::new(0)), prev: 0, hist: vec![], throttled: 0, min_ratio: 1.0 };

    // процессорные нагрузки делят потоки по кругу
    let mut cpu_kinds: Vec<usize> = Vec::new(); // индексы в loads
    if cfg.cpu {
        loads.push(mk("cpu", "Мопс/с", 1e6));
        cpu_kinds.push(loads.len() - 1);
    }
    if cfg.fpu {
        loads.push(mk("fpu", "ГФлопс", 1e9));
        cpu_kinds.push(loads.len() - 1);
    }
    if cfg.cache {
        loads.push(mk("cache", "МБ/с", 1e6));
        cpu_kinds.push(loads.len() - 1);
    }
    for t in 0..(if cpu_kinds.is_empty() { 0 } else { cpu_threads }) {
        let idx = cpu_kinds[t % cpu_kinds.len()];
        let name = loads[idx].name;
        let counter = Arc::clone(&loads[idx].counter);
        let stop = Arc::clone(&stop);
        handles.push(std::thread::spawn(move || match name {
            "cpu" => work_cpu(&stop, &counter),
            "fpu" => work_fpu(&stop, &counter),
            _ => work_cache(&stop, &counter),
        }));
    }
    if cfg.memory {
        loads.push(mk("memory", "МБ/с", 1e6));
        let counter = Arc::clone(&loads.last().unwrap().counter);
        let free_mb = crate::powershell::run_ps("(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|kb| kb / 1024)
            .unwrap_or(2048);
        let pct = cfg.memory_percent.clamp(5, 75) as u64;
        let total = (free_mb * pct / 100).clamp(64, 16 * 1024);
        let parts = if cores >= 4 { 2 } else { 1 };
        for _ in 0..parts {
            let (stop, counter, errors, log) = (Arc::clone(&stop), Arc::clone(&counter), Arc::clone(&mem_errors), Arc::clone(&log));
            let mb = total / parts;
            handles.push(std::thread::spawn(move || work_memory(&stop, &counter, &errors, &log, mb)));
        }
    }
    if cfg.disk {
        loads.push(mk("disk", "МБ/с", 1e6));
        let counter = Arc::clone(&loads.last().unwrap().counter);
        let (stop, errors, log) = (Arc::clone(&stop), Arc::clone(&disk_errors), Arc::clone(&log));
        let (letter, mb) = (cfg.disk_letter.clone(), if cfg.disk_mb == 0 { 1024 } else { cfg.disk_mb });
        handles.push(std::thread::spawn(move || work_disk(&stop, &counter, &errors, &log, &letter, mb)));
    }

    // температуры читаем отдельным потоком (запросы медленные и не должны тормозить секундный тик)
    let temps: Arc<Mutex<(Option<f64>, Option<f64>, Option<f64>, Option<f64>)>> = Arc::new(Mutex::new((None, None, None, None)));
    {
        let (temps, stop) = (Arc::clone(&temps), Arc::clone(&stop));
        handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok(r) = crate::commands::sensors::get_thermal_reading() {
                    if let Ok(mut t) = temps.lock() {
                        *t = (r.cpu_temp_c, r.gpu.map(|g| g.temp_c), r.fan_rpm, r.cpu_power_w);
                    }
                }
                for _ in 0..25 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }));
    }

    let names: Vec<String> = loads.iter().map(|l| l.name.to_string()).collect();
    let mut marker = StressMarker { started_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), duration_secs: cfg.duration_secs, stressors: names, ..Default::default() };
    write_marker(&marker);

    let started = Instant::now();
    let mut tick_no = 0u64;
    let mut reason = "completed".to_string();
    let (mut max_temp, mut max_gpu): (Option<f64>, Option<f64>) = (None, None);
    let (mut load_sum, mut clock_sum, mut clock_n) = (0.0f64, 0.0f64, 0u64);
    let (mut clock_min, mut clock_max) = (f64::INFINITY, 0.0f64);
    let mut prev_times = cpu_times();
    let mut hot_ticks = 0u32;
    let mut events_log: Vec<String> = Vec::new();
    let mut logged_errors = 0usize;

    loop {
        // ждём секунду шагами по 100 мс, чтобы быстро реагировать на «Стоп»
        let target = started + Duration::from_secs(tick_no + 1);
        while Instant::now() < target {
            if STOP.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if STOP.load(Ordering::Relaxed) {
            reason = "stopped".into();
            break;
        }
        tick_no += 1;
        let elapsed = started.elapsed().as_secs();

        let mut tick = StressTick { elapsed, duration: cfg.duration_secs, ..Default::default() };
        // загрузка CPU
        let now_times = cpu_times();
        if let (Some(a), Some(b)) = (prev_times, now_times) {
            let (di, dk, du) = (b.0.saturating_sub(a.0), b.1.saturating_sub(a.1), b.2.saturating_sub(a.2));
            let total = dk + du;
            if total > 0 {
                tick.load = (100.0 * (1.0 - di as f64 / total as f64)).clamp(0.0, 100.0);
            }
        }
        prev_times = now_times;
        if let Some((cur, max)) = cpu_clock() {
            tick.clock_mhz = cur;
            tick.clock_max_mhz = max;
            clock_sum += cur;
            clock_n += 1;
            clock_min = clock_min.min(cur);
            clock_max = clock_max.max(cur);
        }
        load_sum += tick.load;
        if let Ok(t) = temps.lock() {
            tick.temp_c = t.0;
            tick.gpu_temp_c = t.1;
            tick.fan_rpm = t.2;
            tick.power_w = t.3;
        }
        if let Some(t) = tick.temp_c {
            max_temp = Some(max_temp.map_or(t, |m: f64| m.max(t)));
        }
        if let Some(t) = tick.gpu_temp_c {
            max_gpu = Some(max_gpu.map_or(t, |m: f64| m.max(t)));
        }
        // скорости нагрузок и признаки падения (throttling)
        for l in loads.iter_mut() {
            let now = l.counter.load(Ordering::Relaxed);
            let score = (now - l.prev) as f64 / l.div;
            l.prev = now;
            tick.scores.insert(l.name.to_string(), score);
            if tick_no > 2 {
                l.hist.push(score);
            }
            // скорость памяти и диска зависит от фазы записи/чтения и кэша SSD, а не от троттлинга —
            // просадку ищем только у процессорных нагрузок
            if matches!(l.name, "cpu" | "fpu" | "cache") && l.hist.len() >= 10 {
                let base = median(&l.hist[..10]);
                if base > 0.0 {
                    let ratio = score / base;
                    l.min_ratio = l.min_ratio.min(ratio);
                    if ratio < 0.8 {
                        l.throttled += 1;
                        if l.throttled == 5 {
                            tick.events.push(format!("Падение скорости «{}» до {:.0}% от базовой — возможен троттлинг", l.name, ratio * 100.0));
                        }
                    }
                }
            }
        }
        tick.mem_errors = mem_errors.load(Ordering::Relaxed);
        tick.disk_errors = disk_errors.load(Ordering::Relaxed);
        // новые записи журнала нагрузок
        if let Ok(lg) = log.lock() {
            for line in lg.iter().skip(logged_errors) {
                tick.events.push(line.clone());
            }
            logged_errors = lg.len();
        }
        events_log.extend(tick.events.iter().cloned());

        // температурная защита
        let hot = tick.temp_c.unwrap_or(0.0).max(tick.gpu_temp_c.unwrap_or(0.0));
        if cfg.max_temp_c > 0.0 && hot >= cfg.max_temp_c {
            hot_ticks += 1;
        } else {
            hot_ticks = 0;
        }

        let _ = window.emit("stress-tick", &tick);
        if tick_no % 5 == 0 {
            marker.last_elapsed = elapsed;
            marker.last_temp_c = tick.temp_c;
            marker.last_gpu_temp_c = tick.gpu_temp_c;
            marker.last_load = tick.load;
            write_marker(&marker);
        }
        if hot_ticks >= 3 {
            reason = "thermal".into();
            events_log.push(format!("Температурная защита: {:.0} °C ≥ порога {:.0} °C — тест остановлен", hot, cfg.max_temp_c));
            break;
        }
        if tick.mem_errors > 0 || tick.disk_errors > 0 {
            reason = "error".into();
            events_log.push("Обнаружены ошибки при проверке данных — тест остановлен".to_string());
            break;
        }
        if cfg.duration_secs > 0 && elapsed >= cfg.duration_secs {
            break;
        }
    }

    stop.store(true, Ordering::SeqCst);
    for h in handles {
        let _ = h.join();
    }
    let stressors: Vec<StressorStat> = loads
        .iter()
        .map(|l| {
            let n = l.hist.len().max(1) as f64;
            StressorStat {
                name: l.name.to_string(),
                unit: l.unit.to_string(),
                avg: l.hist.iter().sum::<f64>() / n,
                min: l.hist.iter().cloned().fold(f64::INFINITY, f64::min).min(if l.hist.is_empty() { 0.0 } else { f64::INFINITY }),
                max: l.hist.iter().cloned().fold(0.0, f64::max),
                baseline: if l.hist.len() >= 10 { median(&l.hist[..10]) } else { 0.0 },
                min_ratio: l.min_ratio,
                throttled_secs: l.throttled,
            }
        })
        .collect();
    let result = StressResult {
        reason,
        elapsed_secs: started.elapsed().as_secs(),
        threads: cpu_threads,
        stressors,
        max_temp_c: max_temp,
        max_gpu_temp_c: max_gpu,
        avg_load: if tick_no > 0 { load_sum / tick_no as f64 } else { 0.0 },
        clock_avg_mhz: if clock_n > 0 { clock_sum / clock_n as f64 } else { 0.0 },
        clock_min_mhz: if clock_min.is_finite() { clock_min } else { 0.0 },
        clock_max_mhz: clock_max,
        mem_errors: mem_errors.load(Ordering::Relaxed),
        disk_errors: disk_errors.load(Ordering::Relaxed),
        log: events_log,
    };
    remove_marker();
    let _ = window.emit("stress-done", &result);
}
