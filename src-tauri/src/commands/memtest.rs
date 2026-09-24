// Тест памяти (аналог TestMem5, упрощённый): выделяем блок ОЗУ, многопоточно
// пишем и проверяем набор паттернов (нули/единицы, шахматка, бегущая единица,
// адрес-как-данные, псевдослучайные). Блок не больше 75% свободной памяти —
// иначе система начнёт свопиться и тест ничего не покажет. Из пользовательского
// процесса недоступна вся физическая память, поэтому это быстрая проверка на
// явный брак, а не полноценная замена аппаратным тестерам.

use crate::powershell::run_ps;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{Emitter, Window};

static STOP: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

const PATTERNS: usize = 9;

#[derive(Debug, Serialize, Clone)]
pub struct MemProgress {
    pub pct: u32,
    pub pass: u32,
    pub pattern: String,
    pub errors: u64,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct MemResult {
    pub tested_mb: u64,
    pub passes: u32,
    pub errors: u64,
    pub first_errors: Vec<String>,
    pub stopped: bool,
    pub elapsed_secs: u64,
    pub capped: bool,
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn pattern_name(p: usize) -> &'static str {
    ["нули", "единицы", "0xAA…", "0x55…", "0x0F…", "0xF0…", "бегущая единица", "адрес как данные", "случайные"][p]
}

/// Ожидаемое значение элемента `idx` для паттерна `p` на проходе `pass`.
fn expected(p: usize, idx: u64, pass: u32) -> u64 {
    match p {
        0 => 0,
        1 => u64::MAX,
        2 => 0xAAAA_AAAA_AAAA_AAAA,
        3 => 0x5555_5555_5555_5555,
        4 => 0x0F0F_0F0F_0F0F_0F0F,
        5 => 0xF0F0_F0F0_F0F0_F0F0,
        6 => 1u64 << ((idx + pass as u64) % 64),
        7 => idx.wrapping_mul(0x0000_0001_0000_0001) ^ (pass as u64),
        _ => splitmix(idx ^ ((pass as u64) << 40)),
    }
}

#[tauri::command]
pub async fn run_memory_test(window: Window, size_mb: u64, passes: u32) -> Result<MemResult, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Тест памяти уже выполняется".to_string());
    }
    STOP.store(false, Ordering::SeqCst);
    let res = tauri::async_runtime::spawn_blocking(move || run(window, size_mb, passes))
        .await
        .map_err(|e| format!("Тест памяти завершился аварийно: {e}"))
        .and_then(|r| r);
    RUNNING.store(false, Ordering::SeqCst);
    res
}

#[tauri::command(async)]
pub fn stop_memory_test() {
    STOP.store(true, Ordering::SeqCst);
}

fn free_mb() -> Option<u64> {
    run_ps("(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kb| kb / 1024)
}

fn run(window: Window, size_mb: u64, passes: u32) -> Result<MemResult, String> {
    let passes = passes.clamp(1, 20);
    // size_mb == 0 — проверять всю свободную память (как memtest-утилиты): берём
    // свободное за вычетом запаса под систему и интерфейс, иначе начнётся подкачка.
    let mut want = if size_mb == 0 { u64::MAX } else { size_mb.clamp(64, 1024 * 1024) };
    let mut capped = false;
    if let Some(free) = free_mb() {
        let reserve = (free / 10).max(768).min(free / 2);
        let cap = free.saturating_sub(reserve).max(64);
        if want > cap {
            want = cap;
            capped = size_mb != 0;
        }
    } else if size_mb == 0 {
        want = 1024;
    }
    let len = (want * 1024 * 1024 / 8) as usize;
    let mut mem: Vec<u64> = Vec::new();
    mem.try_reserve_exact(len)
        .map_err(|_| format!("Не удалось выделить {want} МБ — недостаточно свободной памяти"))?;
    mem.resize(len, 0);

    // одно ядро оставляем интерфейсу — иначе на слабых процессорах окно замирает
    let threads = num_cpus::get().saturating_sub(1).max(1);
    let chunk = (len + threads - 1) / threads;
    let errors = AtomicU64::new(0);
    let first: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let steps_total = passes as usize * PATTERNS * 2;
    let started = Instant::now();
    let mut step = 0usize;
    let mut stopped = false;
    let mut done_passes = 0u32;

    'outer: for pass in 0..passes {
        for p in 0..PATTERNS {
            for verify in [false, true] {
                if STOP.load(Ordering::SeqCst) {
                    stopped = true;
                    break 'outer;
                }
                std::thread::scope(|s| {
                    for (ci, part) in mem.chunks_mut(chunk).enumerate() {
                        let base = (ci * chunk) as u64;
                        let errors = &errors;
                        let first = &first;
                        s.spawn(move || {
                            crate::sysutil::lower_thread_priority();
                            if !verify {
                                for (i, v) in part.iter_mut().enumerate() {
                                    unsafe { std::ptr::write_volatile(v, expected(p, base + i as u64, pass)) };
                                }
                            } else {
                                for (i, v) in part.iter().enumerate() {
                                    let got = unsafe { std::ptr::read_volatile(v) };
                                    let want = expected(p, base + i as u64, pass);
                                    if got != want {
                                        let n = errors.fetch_add(1, Ordering::Relaxed);
                                        if n < 10 {
                                            if let Ok(mut f) = first.lock() {
                                                f.push(format!(
                                                    "смещение {:#x}: записано {:#018x}, прочитано {:#018x} ({})",
                                                    (base + i as u64) * 8, want, got, pattern_name(p)
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                });
                step += 1;
                let _ = window.emit(
                    "mem-progress",
                    MemProgress {
                        pct: (step * 100 / steps_total) as u32,
                        pass: pass + 1,
                        pattern: pattern_name(p).to_string(),
                        errors: errors.load(Ordering::Relaxed),
                    },
                );
            }
        }
        done_passes += 1;
    }

    Ok(MemResult {
        tested_mb: want,
        passes: done_passes,
        errors: errors.load(Ordering::Relaxed),
        first_errors: first.into_inner().unwrap_or_default(),
        stopped,
        elapsed_secs: started.elapsed().as_secs(),
        capped,
    })
}
