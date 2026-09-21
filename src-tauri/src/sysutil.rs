// Мелкие системные утилиты.

/// Понижает приоритет текущего потока (BELOW_NORMAL), чтобы фоновые нагрузочные
/// тесты не отнимали время у интерфейса на слабых процессорах.
#[cfg(target_os = "windows")]
pub fn lower_thread_priority() {
    extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn SetThreadPriority(h: *mut std::ffi::c_void, priority: i32) -> i32;
    }
    const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn lower_thread_priority() {}

/// Повышает приоритет текущего потока (ABOVE_NORMAL) — для потока-монитора
/// стресс-теста: секундный тик должен идти вовремя, пока рабочие потоки заняты.
#[cfg(target_os = "windows")]
pub fn raise_thread_priority() {
    extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn SetThreadPriority(h: *mut std::ffi::c_void, priority: i32) -> i32;
    }
    const THREAD_PRIORITY_ABOVE_NORMAL: i32 = 1;
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn raise_thread_priority() {}
