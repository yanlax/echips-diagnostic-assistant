// Отправка отчётов на сервер Echips (см. srv.rs, server/DEPLOY.md): после автопрогона и при ручном
// экспорте приложение отправляет JSON и PDF отчёта с сессией инженера. Путь на сервере:
// <инженер>/<дата диагностики>/[<приёмка>_]<серийник>/<время начала>[_before|_after][_full|_express].{json,pdf};
// первый сегмент сервер подставляет сам (имя из сессии), поэтому под чужим именем отчёт не сохранить.
// Отчёт одного прогона обновляется в тех же файлах (без дубликатов).
//
// Без сети или без входа на сервере отчёт сохраняется в очередь
// %LOCALAPPDATA%\Echips\HardwareCheck\reports_queue и уходит при следующей отправке или после входа.
// Отчёты другого инженера из очереди остаются до его входа на этом ноутбуке.

use serde_json::{json, Value};
use std::path::PathBuf;

fn queue_dir() -> PathBuf {
    // В WinPE %LOCALAPPDATA% — диск X: в ОЗУ: очередь пропала бы при перезагрузке, а отчёт без
    // сети как раз и ждёт следующей загрузки. Там кладём рядом с exe (флешка).
    if crate::winpe::in_winpe() {
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            return dir.join("reports_queue");
        }
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Echips").join("HardwareCheck").join("reports_queue")
}

fn safe(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' })
        .take(30)
        .collect();
    if cleaned.is_empty() { "unknown".to_string() } else { cleaned }
}

/// Отправляет отчёт (и PDF) на сервер. Err — причина, по которой отчёт остаётся в очереди.
async fn post(envelope: &Value) -> Result<(), String> {
    let sess = super::srv::session().ok_or("Нет входа на сервере — отчёт отправится после входа")?;
    let report = &envelope["report"];
    if report["engineer"].as_str().unwrap_or("") != sess.name {
        return Err("Отчёт другого инженера — отправится после его входа".to_string());
    }
    let serial = safe(report["device_serial"].as_str().unwrap_or(""));
    let kind = safe(envelope["kind"].as_str().unwrap_or(""));
    let started = report["started_at"]
        .as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    // Структура: <инженер>/<дата диагностики>/[<приёмка>_]<серийник>/<время начала>. Приёмка — только цифры (до 6).
    let intake: String = report["intake"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_digit()).take(6).collect();
    let device_dir = if intake.is_empty() { serial.clone() } else { format!("{intake}_{serial}") };
    let date = started.format("%Y-%m-%d").to_string();
    let time = started.format("%H%M%S").to_string();
    // Этап ремонта и режим автопрогона — в имени файла: «История» по ним находит пару «до/после» одного режима.
    let stage = match report["repair_stage"].as_str().unwrap_or("") {
        "before" => "_before",
        "after" => "_after",
        _ => "",
    };
    let mode = match report["run_mode"].as_str().unwrap_or("") {
        "полный" => "_full",
        "экспресс" => "_express",
        _ => "",
    };
    let path = format!("{}/{date}/{device_dir}/{time}{stage}{mode}", safe(&sess.name));
    let pretty = serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?;

    // PDF — тот же, что «Экспорт PDF». Ошибка самой сборки PDF (не сети) не держит отчёт в очереди: уйдёт хотя бы JSON.
    let mut body = json!({ "path": path, "json": pretty, "kind": kind });
    if let Ok(rep) = serde_json::from_value::<super::report::DiagnosticReport>(report.clone()) {
        if let Ok(pdf) = super::report::render_pdf(&rep) {
            body["pdf_b64"] = json!(super::srv::b64_encode(&pdf));
        }
    }
    match super::srv::request("POST", "/report", Some(&body), true).await? {
        (200, _) => Ok(()),
        (code, text) => Err(super::srv::api_err(code, &text)),
    }
}

/// Строка списка отчётов для вкладки «История» (только админ).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ReportRef {
    pub path: String,
    pub engineer: String,
    pub date: String,
    /// «<приёмка>_<серийник>» или «<серийник>»; для старых отчётов — из имени файла
    pub device: String,
    pub file: String,
}

/// Список отчётов с сервера (только админ), новые сверху.
#[tauri::command(async)]
pub async fn list_reports() -> Result<Vec<ReportRef>, String> {
    match super::srv::request("GET", "/reports", None, true).await? {
        (200, text) => serde_json::from_str(&text).map_err(|e| format!("Некорректный ответ сервера: {e}")),
        (code, text) => Err(super::srv::api_err(code, &text)),
    }
}

/// Содержимое одного отчёта (JSON-конверт: kind, app_version, report).
#[tauri::command(async)]
pub async fn fetch_report(path: String) -> Result<Value, String> {
    if path.contains("..") || !path.ends_with(".json") {
        return Err("Некорректный путь отчёта".to_string());
    }
    match super::srv::request("GET", &format!("/report?path={}", urlencoding::encode(&path)), None, true).await? {
        (200, text) => serde_json::from_str(&text).map_err(|e| format!("Отчёт повреждён: {e}")),
        (code, text) => Err(super::srv::api_err(code, &text)),
    }
}

/// Состояние отправки отчётов: файлы `reports_last_ok.txt` / `reports_last_err.txt` рядом с очередью
/// (время последней успешной отправки и причина последней неудачи) — для строки в левом меню.
fn state_path(name: &str) -> PathBuf {
    let q = queue_dir();
    q.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")).join(name)
}

fn note_ok() {
    let _ = std::fs::create_dir_all(queue_dir().parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(state_path("reports_last_ok.txt"), chrono::Local::now().to_rfc3339());
    let _ = std::fs::remove_file(state_path("reports_last_err.txt"));
}

fn note_err(reason: &str) {
    let _ = std::fs::create_dir_all(queue_dir().parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(state_path("reports_last_err.txt"), reason);
}

#[derive(Debug, serde::Serialize)]
pub struct QueueInfo {
    /// сколько отчётов ждёт отправки
    pub count: usize,
    /// время последней успешной отправки (RFC3339) или пусто
    pub last_ok: String,
    /// причина последней неудачи (пусто, если после неё была успешная отправка)
    pub last_err: String,
}

#[tauri::command(async)]
pub fn report_queue_info() -> QueueInfo {
    let count = std::fs::read_dir(queue_dir())
        .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x == "json")).count())
        .unwrap_or(0);
    QueueInfo {
        count,
        last_ok: std::fs::read_to_string(state_path("reports_last_ok.txt")).unwrap_or_default().trim().to_string(),
        last_err: std::fs::read_to_string(state_path("reports_last_err.txt")).unwrap_or_default().trim().to_string(),
    }
}

/// Отправляет накопленное в очереди; на первой же неудаче останавливается.
async fn flush() -> usize {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(queue_dir()) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map_or(false, |x| x == "json"))
            .collect(),
        Err(_) => return 0,
    };
    files.sort();
    let mut sent = 0;
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(body) = serde_json::from_str::<Value>(&text) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        // отчёты другого инженера остаются в очереди до его входа
        if super::srv::session().map_or(true, |s| body["report"]["engineer"].as_str().unwrap_or("") != s.name) {
            continue;
        }
        if let Err(reason) = post(&body).await {
            note_err(&reason);
            break;
        }
        let _ = std::fs::remove_file(&path);
        sent += 1;
        note_ok();
    }
    sent
}

/// kind: "auto" (конец автопрогона) | "manual" (ручной экспорт).
/// Возвращает "sent" или "queued: <причина>".
#[tauri::command(async)]
pub async fn submit_report(kind: String, report: Value) -> Result<String, String> {
    let envelope = json!({
        "kind": kind,
        "app_version": env!("CARGO_PKG_VERSION"),
        "sent_at": chrono::Local::now().to_rfc3339(),
        "report": report,
    });
    match post(&envelope).await {
        Ok(()) => {
            note_ok();
            flush().await;
            Ok("sent".to_string())
        }
        Err(reason) => {
            note_err(&reason);
            let dir = queue_dir();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let name = format!("{}.json", chrono::Local::now().timestamp_millis());
            let text = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
            std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
            Ok(format!("queued: {reason}"))
        }
    }
}

/// При запуске приложения — досылает отчёты, застрявшие без связи.
#[tauri::command(async)]
pub async fn flush_report_queue() -> Result<usize, String> {
    Ok(flush().await)
}
