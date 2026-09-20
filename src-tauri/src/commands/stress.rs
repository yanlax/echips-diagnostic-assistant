// Стресс-тест CPU: нагрузка на все логические ядра на заданное время.
// Каждую секунду шлёт событие "stress-progress" с прогрессом и скоростью
// вычислений — просадка скорости во время теста указывает на троттлинг.

use serde::Serialize;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

static STOP: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Clone)]
pub struct StressProgress {
    pub elapsed_secs: u64,
    pub total_secs: u64,
    /// Миллионов операций за последнюю секунду
    pub mops: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct StressResult {
    pub threads: usize,
    pub elapsed_secs: u64,
    pub cancelled: bool,
    pub avg_mops: f64,
    pub min_mops: f64,
    pub max_mops: f64,
}

#[tauri::command]
pub async fn run_cpu_stress(app: AppHandle, duration_secs: u64) -> Result<StressResult, String> {
    let total = duration_secs.clamp(5, 3600);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Стресс-тест уже выполняется".to_string());
    }
    STOP.store(false, Ordering::SeqCst);

    let result = tauri::async_runtime::spawn_blocking(move || {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let counter = Arc::new(AtomicU64::new(0));

        let handles: Vec<_> = (0..threads)
            .map(|i| {
                let counter = Arc::clone(&counter);
                std::thread::spawn(move || {
                    let mut x = 1.000_1_f64 + i as f64 * 1e-6;
                    while !STOP.load(Ordering::Relaxed) {
                        for _ in 0..100_000 {
                            x = black_box((x * 1.000_000_1 + 0.5).sqrt().sin().abs() + 1.0);
                        }
                        counter.fetch_add(100_000, Ordering::Relaxed);
                    }
                    black_box(x);
                })
            })
            .collect();

        let start = Instant::now();
        let mut last_count = 0u64;
        let mut rates: Vec<f64> = Vec::new();
        for sec in 1..=total {
            // Ждём секунду небольшими шагами, чтобы быстро реагировать на отмену.
            let tick_end = start + Duration::from_secs(sec);
            while Instant::now() < tick_end && !STOP.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if STOP.load(Ordering::Relaxed) {
                break;
            }
            let count = counter.load(Ordering::Relaxed);
            let mops = (count - last_count) as f64 / 1e6;
            last_count = count;
            rates.push(mops);
            let _ = app.emit(
                "stress-progress",
                StressProgress { elapsed_secs: sec, total_secs: total, mops },
            );
        }

        let cancelled = STOP.load(Ordering::SeqCst);
        STOP.store(true, Ordering::SeqCst);
        for h in handles {
            let _ = h.join();
        }

        let avg = if rates.is_empty() { 0.0 } else { rates.iter().sum::<f64>() / rates.len() as f64 };
        StressResult {
            threads,
            elapsed_secs: rates.len() as u64,
            cancelled,
            avg_mops: avg,
            min_mops: if rates.is_empty() { 0.0 } else { rates.iter().cloned().fold(f64::INFINITY, f64::min) },
            max_mops: rates.iter().cloned().fold(0.0, f64::max),
        }
    })
    .await
    .map_err(|e| format!("Стресс-тест завершился аварийно: {e}"));

    RUNNING.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub fn stop_cpu_stress() {
    STOP.store(true, Ordering::SeqCst);
}
