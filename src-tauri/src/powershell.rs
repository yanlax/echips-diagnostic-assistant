// Общий хелпер для вызова PowerShell и декодирования вывода.
//
// Кодировка: принудительно переключаем консоль PowerShell на UTF-8 в начале
// скрипта и читаем вывод как UTF-8 (НЕ cp1251 — это давало кракозябры).
//
// Быстродействие: запуск powershell.exe и автозагрузка модулей CIM на каждый
// вызов на бюджетных ноутбуках занимают секунды, а автопрогон делает десятки
// вызовов. Поэтому `run_ps`/`run_ps_json` сначала пробуют постоянный
// PowerShell-процесс (пул из двух), которому скрипты отправляются по stdin.
// Любая проблема с постоянным процессом (не запустился, самопроверка не
// прошла, оборвался протокол) прозрачно откатывает вызов на прежний способ —
// отдельный процесс на вызов; после двух неудач хост отключается совсем.
// Переменная окружения ECHIPS_PS_HOST=0 отключает постоянный процесс.
// У обоих способов есть таймаут: зависший PowerShell убивается, тест не
// блокирует автопрогон навсегда. Время каждого вызова пишется в perf.log.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const UTF8_PREAMBLE: &str = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8;";

/// Таймаут одного вызова, секунд.
const CALL_TIMEOUT_SECS: u64 = 120;

// ------------------------------------------------------------------
//  Классический способ: отдельный процесс на вызов (с таймаутом)
// ------------------------------------------------------------------

fn build_command(script: &str) -> Command {
    let full_script = format!("{UTF8_PREAMBLE} {script}");
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &full_script]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd
}

/// Запускает команду и ждёт не дольше `timeout`; по таймауту убивает процесс.
fn output_with_timeout(mut cmd: Command, timeout: Duration) -> Result<(String, String, bool), String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("Не удалось запустить PowerShell: {e}"))?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let h_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_end(&mut b);
        }
        b
    });
    let h_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut b);
        }
        b
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("PowerShell не ответил за {} с — вызов прерван", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("Ошибка ожидания PowerShell: {e}")),
        }
    };
    let so = String::from_utf8_lossy(&h_out.join().unwrap_or_default()).trim().to_string();
    let se = String::from_utf8_lossy(&h_err.join().unwrap_or_default()).trim().to_string();
    Ok((so, se, status.success()))
}

fn run_classic(script: &str) -> Result<String, String> {
    let (stdout, stderr, ok) = output_with_timeout(build_command(script), Duration::from_secs(CALL_TIMEOUT_SECS))?;
    if !ok {
        return Err(format!("PowerShell завершился с ошибкой: {stderr}"));
    }
    Ok(stdout)
}

// ------------------------------------------------------------------
//  Постоянный процесс PowerShell
// ------------------------------------------------------------------

struct Host {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    err: Arc<Mutex<String>>,
    verified: bool,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

enum HostOutcome {
    /// Скрипт выполнен (успешно или с ошибкой скрипта)
    Done(Result<String, String>),
    /// Постоянный процесс недоступен — нужен откат на классический способ
    Unavailable,
}

static HOST_A: Mutex<Option<Host>> = Mutex::new(None);
static HOST_B: Mutex<Option<Host>> = Mutex::new(None);
static HOST_FAILURES: AtomicU32 = AtomicU32::new(0);
static HOST_DISABLED: AtomicBool = AtomicBool::new(false);
static SEQ: AtomicU64 = AtomicU64::new(1);

fn host_enabled() -> bool {
    !HOST_DISABLED.load(Ordering::Relaxed) && std::env::var("ECHIPS_PS_HOST").map(|v| v != "0").unwrap_or(true)
}

/// Сбой запуска или самопроверки постоянного процесса: отключаем его сразу,
/// чтобы не тратить время на повторные ожидания — работаем прежним способом.
fn note_host_failure() {
    HOST_FAILURES.fetch_add(1, Ordering::Relaxed);
    HOST_DISABLED.store(true, Ordering::Relaxed);
}

fn spawn_host() -> Option<Host> {
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoLogo", "-NoProfile", "-NoExit", "-ExecutionPolicy", "Bypass", "-Command", "-"]);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;

    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let err = Arc::new(Mutex::new(String::new()));
    let err2 = Arc::clone(&err);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            if let Ok(mut b) = err2.lock() {
                if b.len() < 8192 {
                    b.push_str(&line);
                    b.push('\n');
                }
            }
        }
    });

    let mut host = Host { child, stdin, rx, err, verified: false };
    let ready = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'; Write-Output '<<ECHIPS_READY>>'\n";
    host.stdin.write_all(ready.as_bytes()).ok()?;
    host.stdin.flush().ok()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        match host.rx.recv_timeout(left) {
            Ok(l) if l.contains("<<ECHIPS_READY>>") => return Some(host),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

/// Подготовка скрипта для постоянного процесса: хвостовой `; exit 0` превращается
/// в «игнорировать статус», любой другой `exit` запрещает этот способ (он убил бы
/// общий процесс). Возвращает (тело, игнорировать_статус).
fn prepare(script: &str) -> Option<(String, bool)> {
    let s = script.trim_end();
    let mut body = s.to_string();
    let mut ignore = false;
    if let Some(idx) = body.rfind("exit 0") {
        let before = body[..idx].trim_end();
        if body[idx + 6..].trim().is_empty() && before.ends_with(';') {
            body = before.trim_end_matches(';').to_string();
            ignore = true;
        }
    }
    let has_exit = body
        .split(|c: char| !(c.is_alphanumeric() || c == '$' || c == '_'))
        .any(|w| w.eq_ignore_ascii_case("exit"));
    if has_exit {
        return None;
    }
    Some((body, ignore))
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn exec_on_host(host: &mut Host, body: &str, ignore_status: bool, timeout: Duration) -> HostOutcome {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut b) = host.err.lock() {
        b.clear();
    }
    // Скрипт в base64 — одна строка без экранирования; выполняется в дочерней
    // области видимости (& scriptblock), поэтому переменные вызовов не пересекаются.
    let line = format!(
        "$__s=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{}')); Write-Output '<<S{seq}>>'; $__ok=$true; \
         try {{ & ([scriptblock]::Create($__s)); $__ok=$? }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); $__ok=$false }}; \
         Write-Output ('<<E{seq}>>' + [int](-not $__ok))\n",
        b64(body.as_bytes())
    );
    if host.stdin.write_all(line.as_bytes()).is_err() || host.stdin.flush().is_err() {
        return HostOutcome::Unavailable;
    }
    let start_marker = format!("<<S{seq}>>");
    let end_marker = format!("<<E{seq}>>");
    let deadline = Instant::now() + timeout;
    let mut started = false;
    let mut lines: Vec<String> = Vec::new();
    loop {
        let left = match deadline.checked_duration_since(Instant::now()) {
            Some(d) => d,
            None => {
                // зависший вызов: хост будет убит и пересоздан
                return HostOutcome::Done(Err(format!("PowerShell не ответил за {} с — вызов прерван", timeout.as_secs())));
            }
        };
        match host.rx.recv_timeout(left) {
            Ok(l) => {
                if !started {
                    if l.contains(&start_marker) {
                        started = true;
                    }
                    continue;
                }
                if let Some(pos) = l.find(&end_marker) {
                    let tail = &l[..pos];
                    if !tail.trim().is_empty() {
                        lines.push(tail.to_string());
                    }
                    let failed = l[pos + end_marker.len()..].trim() == "1";
                    let out = lines.join("\n").trim().to_string();
                    if failed && !ignore_status {
                        let e = host.err.lock().map(|b| b.trim().to_string()).unwrap_or_default();
                        return HostOutcome::Done(Err(format!("PowerShell завершился с ошибкой: {e}")));
                    }
                    return HostOutcome::Done(Ok(out));
                }
                lines.push(l);
            }
            Err(RecvTimeoutError::Timeout) => {
                return HostOutcome::Done(Err(format!("PowerShell не ответил за {} с — вызов прерван", timeout.as_secs())));
            }
            Err(RecvTimeoutError::Disconnected) => return HostOutcome::Unavailable,
        }
    }
}

fn run_on_slot(body: &str, ignore: bool, slot: &mut Option<Host>) -> HostOutcome {
    if slot.is_none() {
        match spawn_host() {
            Some(h) => *slot = Some(h),
            None => {
                note_host_failure();
                return HostOutcome::Unavailable;
            }
        }
    }
    if !slot.as_ref().map(|h| h.verified).unwrap_or(false) {
        // самопроверка протокола на тривиальном скрипте
        let ok = match slot.as_mut() {
            Some(h) => matches!(
                exec_on_host(h, "Write-Output 'echips-ok'", false, Duration::from_secs(20)),
                HostOutcome::Done(Ok(ref s)) if s == "echips-ok"
            ),
            None => false,
        };
        if !ok {
            *slot = None;
            note_host_failure();
            return HostOutcome::Unavailable;
        }
        if let Some(h) = slot.as_mut() {
            h.verified = true;
        }
    }
    let outcome = match slot.as_mut() {
        Some(h) => exec_on_host(h, body, ignore, Duration::from_secs(CALL_TIMEOUT_SECS)),
        None => HostOutcome::Unavailable,
    };
    match &outcome {
        HostOutcome::Unavailable => *slot = None,
        // зависший процесс не переиспользуем
        HostOutcome::Done(Err(e)) if e.contains("не ответил") => *slot = None,
        _ => {}
    }
    outcome
}

fn run_host(script: &str) -> HostOutcome {
    if !host_enabled() {
        return HostOutcome::Unavailable;
    }
    let (body, ignore) = match prepare(script) {
        Some(x) => x,
        None => return HostOutcome::Unavailable,
    };
    // свободный из двух процессов, иначе ждём первый
    if let Ok(mut g) = HOST_A.try_lock() {
        return run_on_slot(&body, ignore, &mut *g);
    }
    if let Ok(mut g) = HOST_B.try_lock() {
        return run_on_slot(&body, ignore, &mut *g);
    }
    match HOST_A.lock() {
        Ok(mut g) => run_on_slot(&body, ignore, &mut *g),
        Err(_) => HostOutcome::Unavailable,
    }
}

/// Завершает постоянные процессы PowerShell при выходе из приложения.
pub fn shutdown() {
    for slot in [&HOST_A, &HOST_B] {
        if let Ok(mut g) = slot.lock() {
            *g = None; // Drop у Host убивает процесс
        }
    }
}

// ------------------------------------------------------------------
//  Лог времени вызовов
// ------------------------------------------------------------------

fn perf_log(mode: &str, script: &str, took: Duration, ok: bool) {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if base.is_empty() {
        return;
    }
    let dir = std::path::PathBuf::from(base).join("Echips").join("HardwareCheck");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("perf.log");
    if std::fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("perf.old.log"));
    }
    // ключи продуктов и подобное в лог не попадают
    if script.contains("ProductKey") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{:>6} мс · {} · {} · <скрипт с ключом продукта скрыт>", took.as_millis(), mode, if ok { "ok" } else { "ERR" });
        }
        return;
    }
    let head: String = script.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(70).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{:>6} мс · {} · {} · {}", took.as_millis(), mode, if ok { "ok" } else { "ERR" }, head);
    }
}

// ------------------------------------------------------------------
//  Публичный интерфейс
// ------------------------------------------------------------------

/// Выполняет PowerShell-скрипт и возвращает stdout как UTF-8, без крайних пробелов.
pub fn run_ps(script: &str) -> Result<String, String> {
    let t = Instant::now();
    let (mode, res) = match run_host(script) {
        HostOutcome::Done(r) => ("host", r),
        HostOutcome::Unavailable => ("proc", run_classic(script)),
    };
    perf_log(mode, script, t.elapsed(), res.is_ok());
    res
}

/// Как run_ps, но также возвращает, завершилась ли команда успешно (код 0) —
/// нужно там, где важен сам факт успеха, а не только текст (точка
/// восстановления, установка драйверов). Всегда отдельный процесс и без
/// таймаута: установка драйверов может быть долгой.
pub fn run_ps_with_status(script: &str) -> (String, bool) {
    let full_script = format!("{script}; exit $LASTEXITCODE");
    match build_command(&full_script).output() {
        Ok(out) => (String::from_utf8_lossy(&out.stdout).trim().to_string(), out.status.success()),
        Err(_) => (String::new(), false),
    }
}

/// Выполняет PowerShell-команду и парсит stdout как JSON (через `ConvertTo-Json`
/// на стороне скрипта).
pub fn run_ps_json<T: serde::de::DeserializeOwned>(script: &str) -> Result<T, String> {
    let raw = run_ps(script)?;
    if raw.trim().is_empty() {
        return Err("PowerShell не вернул данных".to_string());
    }
    serde_json::from_str(raw.trim()).map_err(|e| format!("Не удалось разобрать JSON: {e}\nВывод: {raw}"))
}
