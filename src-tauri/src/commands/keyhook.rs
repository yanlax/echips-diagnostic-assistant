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
//
// ПОВТОРНЫЙ реальный тест (v0.11.0) показал: «Пуск» всё так же открывается,
// подстраховка не помогла. Вслепую гадать дальше третий раз подряд не имеет
// смысла — вместо этого добавлено логирование (см. `log()` ниже,
// %LOCALAPPDATA%\Echips\HardwareCheck\keyhook.log, как и perf.log в
// powershell.rs): пишется, установились ли оба хука, каждое «съеденное»
// нажатие Win/медиа-клавиши и каждое срабатывание EVENT_SYSTEM_FOREGROUND
// (класс+процесс окна, совпало ли с «Пуском», отправлен ли Escape) — пока
// блок включён. Следующий реальный прогон с этим логом (v0.12.0) подтвердил:
// работает — Escape уходит сразу, фокус возвращается в наше окно.
//
// PrtScr тоже добавлена в список блокируемых (Windows 11 по умолчанию
// открывает Ножницы прямо на неё) — но здесь другая проблема: раз хук
// глушит клавишу ДЛЯ ВСЕЙ СИСТЕМЫ, включая наш собственный webview, простое
// добавление в is_blocked_vk означало бы, что тест клавиатуры вообще
// перестал бы видеть нажатие PrtScr (или F-клавиш, если они на конкретном
// ноутбуке уходят медиа-кодами, а не VK_F1..F12 — см. is_blocked_vk).
// Поэтому теперь при перехвате хук сам ретранслирует нажатие обратно в наш
// JS отдельным Tauri-событием "hook-relay-key" (см. relay_key/WINDOW ниже),
// а не просто глушит его — тест по-прежнему видит и засчитывает нажатие.

#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::{mpsc, Mutex};
    use serde::Serialize;
    use tauri::{Emitter, Window};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::{
        GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_BROWSER_BACK,
        VK_BROWSER_FAVORITES, VK_BROWSER_FORWARD, VK_BROWSER_HOME, VK_BROWSER_REFRESH, VK_BROWSER_SEARCH,
        VK_BROWSER_STOP, VK_ESCAPE, VK_LAUNCH_APP1, VK_LAUNCH_APP2, VK_LAUNCH_MAIL, VK_LAUNCH_MEDIA_SELECT,
        VK_LWIN, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_RWIN,
        VK_SNAPSHOT, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
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
    /// Окно, которому шлём событие "hook-relay-key" — установлено при
    /// start(). Нужно, потому что WH_KEYBOARD_LL глушит клавишу для ВСЕЙ
    /// системы, включая наш же webview: просто добавить, скажем, PrtScr в
    /// is_blocked_vk означало бы, что тест клавиатуры перестал бы видеть
    /// нажатие PrtScr вообще (см. relay_key ниже).
    static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

    #[derive(Serialize, Clone)]
    struct RelayKey {
        vk: u32,
        down: bool,
    }
    /// Раз хук глушит нажатие для всей системы (иначе не подавить системное
    /// действие — OSD громкости, Snipping Tool на PrtScr и т.п.), сами же
    /// отправляем его обратно в JS отдельным Tauri-событием, чтобы тест
    /// клавиатуры всё равно засчитал нажатие. Win/меню «Пуск» сюда не
    /// входит — эта клавиша в раскладке теста не отображается, ретранслировать нечего.
    fn relay_key(vk: u32, down: bool) {
        if vk as u16 == VK_LWIN || vk as u16 == VK_RWIN {
            return;
        }
        if let Ok(guard) = WINDOW.lock() {
            if let Some(w) = guard.as_ref() {
                let _ = w.emit("hook-relay-key", RelayKey { vk, down });
            }
        }
    }

    /// Диагностический лог для этого хука — отдельно от perf.log
    /// (powershell.rs), т.к. это не про время вызова, а про сам факт и
    /// детали срабатывания. См. заметку в начале файла: без этого третья
    /// попытка починить блокировку Win была бы такой же догадкой вслепую,
    /// как первые две.
    fn log(line: &str) {
        let base = match std::env::var("LOCALAPPDATA") {
            Ok(v) if !v.is_empty() => v,
            _ => return,
        };
        let dir = std::path::PathBuf::from(base).join("Echips").join("HardwareCheck");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("keyhook.log");
        if std::fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
            let _ = std::fs::rename(&path, dir.join("keyhook.old.log"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            use std::io::Write;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let _ = writeln!(f, "{now} · {line}");
        }
    }

    /// «Съедаемые» коды клавиш: сама Win + медиа-клавиши F-ряда, которые
    /// на многих ноутбуках всплывают системным OSD (громкость/медиа/
    /// браузер), + PrtScr (на Windows 11 по умолчанию сама открывает
    /// Ножницы/Snipping Tool) — яркость сюда не входит: у неё нет
    /// отдельного VK-кода, её обрабатывает встроенный контроллер/BIOS ещё
    /// до ОС (как Fn).
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
                | VK_SNAPSHOT
        )
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && ACTIVE.load(Ordering::Relaxed) {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let msg = wparam as u32;
            let is_key_msg = msg == WM_KEYDOWN || msg == WM_KEYUP || msg == WM_SYSKEYDOWN || msg == WM_SYSKEYUP;
            if is_key_msg && is_blocked_vk(kb.vkCode) {
                log(&format!("hook_proc: съедена vk=0x{:02X} msg=0x{:04X}", kb.vkCode, msg));
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                relay_key(kb.vkCode, down);
                return 1; // «съедаем» нажатие — дальше по системе не идёт
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    /// Подстраховка: если «Пуск»/Поиск всё же стал активным окном, пока блок
    /// включён — сразу шлём ему Escape. По реальному логу (keyhook.log,
    /// v0.11.0) подтвердилось: hook_proc честно глотает КАЖДОЕ нажатие Win
    /// (vk=0x5B) — но меню всё равно открывается, причём foreground
    /// переключается на системный UI даже раньше, чем наш хук вообще успел
    /// увидеть нажатие. Значит открытие «Пуска»/Поиска по Win идёт в обход
    /// цепочки WH_KEYBOARD_LL целиком (какой-то более низкоуровневый путь в
    /// самой ОС) — подстраховка через Escape здесь не запасной вариант,
    /// а единственный реально работающий механизм.
    ///
    /// У Start UI (Windows 10/11) класс окна "Windows.UI.Core.CoreWindow",
    /// но так называются и другие системные оверлеи (Поиск, Центр
    /// уведомлений) — поэтому дополнительно проверяем процесс. По тому же
    /// логу выяснилось: на актуальной сборке Windows 11 нажатие Win
    /// открывает не StartMenuExperienceHost.exe, а SearchApp.exe (тот же
    /// объединённый UI Поиска/Пуска) — раньше в списке было только старое
    /// имя процесса, поэтому Escape ни разу не отправлялся (start=false в
    /// каждой строке лога, несмотря на смену foreground).
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
            log("win_event_proc: foreground сменился, GetClassNameW не удалось");
            return;
        }
        let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut proc_name = String::from("?");
        if pid != 0 {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !handle.is_null() {
                let mut name_buf = [0u16; 260];
                let mut name_len = name_buf.len() as u32;
                if QueryFullProcessImageNameW(handle, 0, name_buf.as_mut_ptr(), &mut name_len) != 0 {
                    proc_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                }
                CloseHandle(handle);
            }
        }
        let proc_lower = proc_name.to_lowercase();
        let is_start = class_name == "Windows.UI.Core.CoreWindow"
            && (proc_lower.ends_with("startmenuexperiencehost.exe")
                || proc_lower.ends_with("shellexperiencehost.exe")
                || proc_lower.ends_with("searchapp.exe")
                || proc_lower.ends_with("searchhost.exe"));
        log(&format!(
            "win_event_proc: foreground класс=\"{class_name}\" процесс=\"{proc_name}\" pid={pid} start={is_start}"
        ));
        if !is_start {
            return;
        }
        send_escape();
        log("win_event_proc: Escape отправлен");
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

    pub fn start(window: Window) -> Result<(), String> {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return Ok(()); // уже включено
        }
        if let Ok(mut guard) = WINDOW.lock() {
            *guard = Some(window);
        }
        log("start: включение блокировки");
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || unsafe {
            let hook: HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0);
            if hook.is_null() {
                log("start: SetWindowsHookExW(WH_KEYBOARD_LL) — ОШИБКА");
                ACTIVE.store(false, Ordering::SeqCst);
                let _ = tx.send(Err("Не удалось установить перехват клавиатуры".to_string()));
                return;
            }
            log("start: WH_KEYBOARD_LL установлен");
            let win_event_hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            log(if win_event_hook.is_null() {
                "start: SetWinEventHook — ОШИБКА, подстраховки Escape не будет"
            } else {
                "start: SetWinEventHook установлен"
            });
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
        log("stop: выключение блокировки");
        let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                PostThreadMessageW(tid as u32, WM_QUIT, 0, 0);
            }
        }
        if let Ok(mut guard) = WINDOW.lock() {
            *guard = None;
        }
    }
}

#[tauri::command(async)]
pub fn start_win_key_block(window: tauri::Window) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win::start(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
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
