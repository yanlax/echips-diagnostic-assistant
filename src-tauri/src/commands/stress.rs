// Реальный стресс-тест CPU: busy-loop поток на каждое логическое ядро на
// заданное время. GPU-нагрузка (compute shader) и мониторинг throttling из
// дизайн-прототипа — не реализованы (throttling нельзя достоверно увидеть
// без чтения частот/температур, которых у нас нет — см. sensors.rs).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Emitter, Window};

#[derive(Debug, Serialize, Clone)]
pub struct StressProgress {
    pub elapsed_secs: u64,
    pub total_secs: u64,
    pub running: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StressResult {
    pub completed: bool,
    pub elapsed_secs: u64,
    pub threads: usize,
}

#[tauri::command]
pub async fn run_cpu_stress(window: Window, duration_secs: u64) -> Result<StressResult, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let threads_count = num_cpus::get();

    let mut handles = Vec::with_capacity(threads_count);
    for _ in 0..threads_count {
        let stop = Arc::clone(&stop);
        handles.push(thread::spawn(move || {
            // Простой busy-loop с плавающей точкой — нагружает ALU/FPU ядра.
            let mut x: f64 = 1.0000001;
            while !stop.load(Ordering::Relaxed) {
                for _ in 0..100_000 {
                    x = (x * 1.0000001).sin().abs() + 1.0;
                }
                std::hint::black_box(x);
            }
        }));
    }

    let start = Instant::now();
    let total = Duration::from_secs(duration_secs);
    loop {
        let elapsed = start.elapsed();
        let _ = window.emit(
            "stress-progress",
            StressProgress {
                elapsed_secs: elapsed.as_secs(),
                total_secs: duration_secs,
                running: true,
            },
        );
        if elapsed >= total {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }

    let _ = window.emit(
        "stress-progress",
        StressProgress { elapsed_secs: duration_secs, total_secs: duration_secs, running: false },
    );

    Ok(StressResult { completed: true, elapsed_secs: duration_secs, threads: threads_count })
}
