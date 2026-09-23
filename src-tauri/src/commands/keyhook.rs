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
// перехватывается только сама клавиша Win (VK_LWIN/VK_RWIN) и «медиа»-
// клавиши F-ряда (громкость/яркость-заглушка/медиа/браузер — см. ниже),
// которые на части ноутбуков всплывают системным OSD-оверлеем и мешают
// технику видеть, что клавиша вообще сработала.
//
// ВАЖНО (по итогам реального теста на железе, v0.10.x): один только
// WH_KEYBOARD_LL не гарантированно останавливает открытие «Пуска» —
// возврат ненулевого значения из hook_proc должен не давать сообщению
// уйти дальше по системе, но на практике меню один раз успело открыться
// раньше, чем сработала блокировка. Чтобы не полагаться на один
// механизм, добавлена подстраховка через SetWinEventHook
// (EVENT_SYSTEM_FOREGROUND): если, несмотря на перехват клавиши, «Пуск»
// всё же стал активным окном — тут же шлём ему Escape, пока блок включён.
// Это тоже не железобетонная гарантия (см. HANDOFF.md/CLAUDE.md — нужен
// ещё один реальный прогон на Windows для проверки), но должно закрывать
// подавляющее большинство случаев мгновенно, без заметного мигания.

#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::mpsc;
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::{
        GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_BROWSER_BACK,
        VK_BROWSER_FAVORITES, VK_BROWSER_FORWARD, VK_BROWSER_HOME, VK_BROWSER_REFRESH, VK_BROWSER_SEARCH,
        VK_BROWSER_STOP, VK_ESCAPE, VK_LAUNCH_APP1, VK_LAUNCH_APP2, VK_LAUNCH_MAIL, VK_LAUNCH_MEDIA_SELECT,
        VK_LWIN, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_RWIN,
        VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetClassNameW, GetMessageW, GetWindowThreadProcessId,
        PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, EVENT_SYSTEM_FOREGROUND,
        HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WINEVENT_OUTOFCONTEXT, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    /// ID потока, который держит хуки и качает сообщения — 0, если хука нет.
    /// Нужен, чтобы разбудить GetMessageW сообщением WM_QUIT при остановке
    /// (низкоуровневые хуки требуют цикла сообщений именно на своём потоке).
    static HOOK_THREAD_ID: AtomicIsize = AtomicIsize::new(0);

    /// «Съедаемые» коды клавиш: сама Win + медиа-клавиши F-ряда, которые
    /// на многих ноутбуках всплывают системным OSD (громкость/медиа/
    /// браузер) — яркость сюда не входит: у неё нет отдельного VK-кода,
    /// её обрабатывает встроенный контроллер/BIOS ещё до ОС (как Fn).
    fn is_blocked_vk(vk: u32) -> bool {
        matches!(
            vk as u16,
            VK_LWIN
                | VK_RWIN
                | VK_VOLUME_MUTE
                | VK_VOLUME_DOWN
                | VK_VOLUME_UP
                | VK_MEDIA_NEXT_TRACK
                | VK_MEDIA_PREV_TRACK
                | VK_MEDIA_STOP
                | VK_MEDIA_PLAY_PAUSE
                | VK_LAUNCH_MAIL
                | VK_LAUNCH_MEDIA_SELECT
                | VK_LAUNCH_APP1
                | VK_LAUNCH_APP2
                | VK_BROWSER_BACK
                | VK_BROWSER_FORWARD
                | VK_BROWSER_REFRESH
                | VK_BROWSER_STOP
                | VK_BROWSER_SEARCH
                | VK_BROWSER_FAVORITES
                | VK_BROWSER_HOME
        )
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && ACTIVE.load(Ordering::Relaxed) {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let msg = wparam as u32;
            let is_key_msg = msg == WM_KEYDOWN || msg == WM_KEYUP || msg == WM_SYSKEYDOWN || msg == WM_SYSKEYUP;
            if is_key_msg && is_blocked_vk(kb.vkCode) {
                return 1; // «съедаем» нажатие — дальше по системе не идёт
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    /// Подстраховка: если «Пуск» всё же стал активным окном, пока блок
    /// включён — сразу шлём ему Escape. У Start UI (Windows 10/11) класс
    /// окна "Windows.UI.Core.CoreWindow", но так называются и другие
    /// системные оверлеи (Поиск, Центр уведомлений) — поэтому дополнительно
    /// проверяем, что процесс окна называется именно StartMenuExperienceHost
    /// или (старый Windows 10) ShellExperienceHost, чтобы случайно не
    /// закрыть что-то ещё нажатием Escape.
    unsafe extern "system" fn win_event_proc(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _id_thread: u32,
        _event_time: u32,
    ) {
        if event != EVENT_SYSTEM_FOREGROUND || !ACTIVE.load(Ordering::Relaxed) || hwnd.is_null() {
            return;
        }
        let mut class_buf = [0u16; 256];
        let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
        if len <= 0 {
            return;
        }
        let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
        if class_name != "Windows.UI.Core.CoreWindow" {
            return;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return;
        }
        let mut name_buf = [0u16; 260];
        let mut name_len = name_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, name_buf.as_mut_ptr(), &mut name_len);
        CloseHandle(handle);
        if ok == 0 {
            return;
        }
        let path = String::from_utf16_lossy(&name_buf[..name_len as usize]).to_lowercase();
        let is_start = path.ends_with("startmenuexperiencehost.exe") || path.ends_with("shellexperiencehost.exe");
        if !is_start {
            return;
        }
        send_escape();
    }

    unsafe fn send_escape() {
        let mut inputs: [INPUT; 2] = std::mem::zeroed();
        for inp in inputs.iter_mut() {
            inp.r#type = INPUT_KEYBOARD;
        }
        inputs[0].Anonymous.ki = KEYBDINPUT { wVk: VK_ESCAPE, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0 };
        inputs[1].Anonymous.ki =
            KEYBDINPUT { wVk: VK_ESCAPE, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 };
        SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
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
            let win_event_hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
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
            if !win_event_hook.is_null() {
                UnhookWinEvent(win_event_hook);
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
