// Блокировка клавиши Win во время теста клавиатуры — по прямому запросу:
// обычный preventDefault() в JS её не останавливает, Windows перехватывает
// Win (открывает меню «Пуск») до того, как событие вообще доходит до
// webview. Единственный способ — низкоуровневый системный хук клавиатуры
// (WH_KEYBOARD_LL), впервые в этом проекте — до сих пор весь доступ к
// Windows шёл через PowerShell (см. CLAUDE.md), здесь без него не обойтись.
//
// Включается только вручную тумблером на экране теста (не сама по себе
// при входе в тест) — риск того, что хук не снимется корректно, слишком
// высок, чтобы включать его без ведома техника. На случай зависания/сбоя
// снимается: (1) при выходе с экрана теста клавиатуры (app.js, A.go),
// (2) при закрытии приложения (RunEvent::Exit в lib.rs). Обычный
// Alt+Tab/Ctrl+Alt+Del и другие системные комбинации не трогаются —
// перехватывается только сама клавиша Win (VK_LWIN/VK_RWIN).

#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::mpsc;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_LWIN, VK_RWIN};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
        WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    /// ID потока, который держит хук и качает сообщения — 0, если хука нет.
    /// Нужен, чтобы разбудить GetMessageW сообщением WM_QUIT при остановке
    /// (низкоуровневые хуки требуют цикла сообщений именно на своём потоке).
    static HOOK_THREAD_ID: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && ACTIVE.load(Ordering::Relaxed) {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let is_win = kb.vkCode == VK_LWIN as u32 || kb.vkCode == VK_RWIN as u32;
            let msg = wparam as u32;
            let is_key_msg = msg == WM_KEYDOWN || msg == WM_KEYUP || msg == WM_SYSKEYDOWN || msg == WM_SYSKEYUP;
            if is_win && is_key_msg {
                return 1; // «съедаем» нажатие — дальше по системе не идёт
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    pub fn start() -> Result<(), String> {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return Ok(()); // уже включено
        }
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || unsafe {
            let hook: HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0);
            if hook.is_null() {
                ACTIVE.store(false, Ordering::SeqCst);
                let _ = tx.send(Err("Не удалось установить перехват клавиатуры".to_string()));
                return;
            }
            HOOK_THREAD_ID.store(GetCurrentThreadId() as isize, Ordering::SeqCst);
            let _ = tx.send(Ok(()));
            let mut msg: MSG = std::mem::zeroed();
            loop {
                let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                if r <= 0 {
                    break; // 0 = WM_QUIT, -1 = ошибка — в обоих случаях выходим
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            UnhookWindowsHookEx(hook);
            HOOK_THREAD_ID.store(0, Ordering::SeqCst);
            ACTIVE.store(false, Ordering::SeqCst);
        });
        rx.recv().map_err(|_| "Поток перехвата клавиатуры не ответил".to_string())?
    }

    pub fn stop() {
        if !ACTIVE.load(Ordering::SeqCst) {
            return;
        }
        let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                PostThreadMessageW(tid as u32, WM_QUIT, 0, 0);
            }
        }
    }
}

#[tauri::command(async)]
pub fn start_win_key_block() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win::start()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[tauri::command(async)]
pub fn stop_win_key_block() {
    #[cfg(target_os = "windows")]
    {
        win::stop();
    }
}
