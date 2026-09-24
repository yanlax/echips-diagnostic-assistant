// Отправка отчётов администратору: после автопрогона и при ручном экспорте
// приложение кладёт JSON прямо в приватный репозиторий отчётов
// (yanlax/echips-reports) через GitHub Contents API, по структуре
// <инженер>/<дата диагностики>/[<приёмка>_]<серийник>/<время начала>.json и .pdf
// (плюс копия в _по_ноутбукам/<серийник>/ для истории по устройству);
// отчёт одного прогона обновляется в тех же файлах (без дубликатов).
//
// Токен вшивается в exe при сборке из секрета GitHub Actions
// ECHIPS_REPORTS_TOKEN (в публичном коде его нет). Это осознанный компромисс
// (вариант 1 из обсуждения): токен можно достать из exe, поэтому он должен
// быть fine-grained, ТОЛЬКО на репозиторий отчётов, право Contents: write и
// срок жизни — максимум год (после истечения нужен новый токен и релиз). Худшее,
// что можно сделать с утёкшим токеном, — засорить или переписать отчёты; до
// кода и списка инженеров он не дотягивается.
//
// Без токена в сборке или без сети отчёт сохраняется в очередь
// %LOCALAPPDATA%\Echips\HardwareCheck\reports_queue и уходит при следующей
// отправке или запуске приложения.

use super::techs::b64_encode;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

const REPO: &str = "yanlax/echips-reports";
const TOKEN: &str = match option_env!("ECHIPS_REPORTS_TOKEN") {
    Some(v) => v,
    None => "",
};

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

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn contents_url(path: &str) -> String {
    let url_path: Vec<String> = path.split('/').map(|s| urlencoding::encode(s).into_owned()).collect();
    format!("https://api.github.com/repos/{REPO}/contents/{}", url_path.join("/"))
}

fn map_status(s: u16) -> String {
    match s {
        401 => "Токен отправки отчётов недействителен или истёк".to_string(),
        403 | 404 => "Нет доступа к репозиторию отчётов".to_string(),
        other => format!("GitHub вернул {other}"),
    }
}

/// Кладёт файл в репозиторий; если файл уже есть (отчёт того же прогона
/// обновился) — заменяет его, передавая sha текущей версии. Так один прогон =
/// одна пара файлов, без дубликатов.
async fn put_file(client: &reqwest::Client, path: &str, bytes: &[u8], message: &str) -> Result<(), String> {
    let url = contents_url(path);
    let get = client
        .get(&url)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(TOKEN)
        .send()
        .await
        .map_err(|e| format!("Нет связи: {e}"))?;
    let sha: Option<String> = match get.status().as_u16() {
        200 => get.json::<Value>().await.ok().and_then(|v| v["sha"].as_str().map(|s| s.to_string())),
        404 => None,
        other => return Err(map_status(other)),
    };
    let mut body = json!({ "message": message, "content": b64_encode(bytes) });
    if let Some(sha) = sha {
        body["sha"] = json!(sha);
    }
    let resp = client
        .put(&url)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Нет связи: {e}"))?;
    match resp.status().as_u16() {
        200 | 201 => Ok(()),
        other => Err(map_status(other)),
    }
}

/// Загружает JSON и PDF отчёта. Путь зависит только от прогона (инженер, дата
/// и время начала, серийник), а не от момента отправки — повторная отправка
/// того же прогона (в том числе из очереди) обновляет те же файлы.
async fn post(client: &reqwest::Client, envelope: &Value) -> Result<(), String> {
    if TOKEN.is_empty() {
        return Err("В этой сборке нет токена отправки отчётов".to_string());
    }
    let report = &envelope["report"];
    let serial = safe(report["device_serial"].as_str().unwrap_or(""));
    let engineer = safe(report["engineer"].as_str().unwrap_or(""));
    let kind = safe(envelope["kind"].as_str().unwrap_or(""));
    let started = report["started_at"]
        .as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    // Структура: <инженер>/<дата диагностики>/[<приёмка>_]<серийник>/<время начала>.{json,pdf}.
    // Приёмка — только цифры (до 6), иначе не используется.
    let intake: String = report["intake"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_digit()).take(6).collect();
    let device_dir = if intake.is_empty() { serial.clone() } else { format!("{intake}_{serial}") };
    let date = started.format("%Y-%m-%d").to_string();
    let time = started.format("%H%M%S").to_string();
    // Этап ремонта («до» / «после») попадает в имя файла: вкладка «История» по нему находит пару.
    let stage = match report["repair_stage"].as_str().unwrap_or("") {
        "before" => "_before",
        "after" => "_after",
        _ => "",
    };
    let base = format!("{engineer}/{date}/{device_dir}/{time}{stage}");
    // Копия для просмотра «все отчёты по ноутбуку»: _по_ноутбукам/<серийник>/ — история
    // всех инженеров и дат в одной папке.
    let by_device = format!(
        "_по_ноутбукам/{serial}/{date}_{time}_{engineer}{}",
        if intake.is_empty() { String::new() } else { format!("_приёмка{intake}") }
    );
    let message = format!(
        "Отчёт: {} / {} ({})",
        safe(report["device_model"].as_str().unwrap_or("")),
        engineer,
        kind
    );

    let pretty = serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?;
    put_file(client, &format!("{base}.json"), pretty.as_bytes(), &message).await?;
    put_file(client, &format!("{by_device}.json"), pretty.as_bytes(), &message).await?;

    // PDF — тот же, что «Экспорт PDF» (тема «Графит»). Ошибка самой сборки PDF
    // (не сети) не должна вечно держать отчёт в очереди — тогда остаётся
    // хотя бы JSON.
    if let Ok(rep) = serde_json::from_value::<super::report::DiagnosticReport>(report.clone()) {
        if let Ok(pdf) = super::report::render_pdf(&rep) {
            put_file(client, &format!("{base}.pdf"), &pdf, &message).await?;
            put_file(client, &format!("{by_device}.pdf"), &pdf, &message).await?;
        }
    }
    // Индекс по серийнику: _по_ноутбукам/<серийник>/index.md со списком всех отчётов по устройству.
    // Вспомогательная вещь — её сбой не должен держать отчёт в очереди.
    let _ = update_device_index(client, &serial).await;
    Ok(())
}

/// Собирает `_по_ноутбукам/<серийник>/index.md`: таблица всех отчётов (дата, время, инженер, приёмка,
/// ссылки на JSON и PDF), новые сверху. Файлы называются `<дата>_<ЧЧММСС>_<инженер>[_приёмка<N>].{json,pdf}`.
async fn update_device_index(client: &reqwest::Client, serial: &str) -> Result<(), String> {
    let dir = format!("_по_ноутбукам/{serial}");
    let resp = client
        .get(contents_url(&dir))
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(TOKEN)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(map_status(resp.status().as_u16()));
    }
    let items: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut names: Vec<String> = items
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["name"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    names.retain(|n| n.ends_with(".json"));
    names.sort();
    names.reverse();
    let mut md = format!("# Отчёты по устройству {serial}\n\n| Дата | Время | Инженер | Приёмка / ремонт | JSON | PDF |\n|---|---|---|---|---|---|\n");
    for n in &names {
        let stem = n.trim_end_matches(".json");
        let mut parts = stem.splitn(4, '_');
        let (date, time, eng) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let intake = parts.next().unwrap_or("").trim_start_matches("приёмка");
        let time_fmt = if time.len() == 6 { format!("{}:{}:{}", &time[0..2], &time[2..4], &time[4..6]) } else { time.to_string() };
        let enc = |f: &str| urlencoding::encode(f).into_owned();
        md.push_str(&format!(
            "| {date} | {time_fmt} | {eng} | {intake} | [json]({}) | [pdf]({}) |\n",
            enc(n),
            enc(&format!("{stem}.pdf"))
        ));
    }
    put_file(client, &format!("{dir}/index.md"), md.as_bytes(), &format!("Индекс отчётов: {serial}")).await
}

/// Строка списка отчётов для вкладки «История» (только админ).
#[derive(Debug, serde::Serialize)]
pub struct ReportRef {
    pub path: String,
    pub engineer: String,
    pub date: String,
    /// «<приёмка>_<серийник>» или «<серийник>»; для старых отчётов — из имени файла
    pub device: String,
    pub file: String,
}

/// Список отчётов из репозитория (одним запросом дерева). Копии в `_по_ноутбукам/` пропускаются.
#[tauri::command(async)]
pub async fn list_reports() -> Result<Vec<ReportRef>, String> {
    if TOKEN.is_empty() {
        return Err("В этой сборке нет токена доступа к отчётам".to_string());
    }
    let client = client()?;
    let resp = client
        .get(format!("https://api.github.com/repos/{REPO}/git/trees/main?recursive=1"))
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(TOKEN)
        .send()
        .await
        .map_err(|e| format!("Нет связи: {e}"))?;
    if !resp.status().is_success() {
        return Err(map_status(resp.status().as_u16()));
    }
    let tree: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out: Vec<ReportRef> = Vec::new();
    for item in tree["tree"].as_array().cloned().unwrap_or_default() {
        let path = match item["path"].as_str() { Some(p) => p, None => continue };
        if !path.ends_with(".json") || path.starts_with("_по_ноутбукам/") {
            continue;
        }
        let parts: Vec<&str> = path.split('/').collect();
        let (engineer, date, device, file) = match parts.len() {
            4 => (parts[0], parts[1], parts[2].to_string(), parts[3]),
            3 => {
                // старый формат: <инженер>/<дата>/<ЧЧММСС>_<серийник>[_вид].json
                let f = parts[2].trim_end_matches(".json");
                let dev = f.splitn(2, '_').nth(1).unwrap_or(f).to_string();
                (parts[0], parts[1], dev, parts[2])
            }
            _ => continue,
        };
        out.push(ReportRef { path: path.to_string(), engineer: engineer.to_string(), date: date.to_string(), device, file: file.to_string() });
    }
    out.sort_by(|a, b| (b.date.as_str(), b.file.as_str()).cmp(&(a.date.as_str(), a.file.as_str())));
    Ok(out)
}

/// Содержимое одного отчёта (JSON-конверт: kind, app_version, report).
#[tauri::command(async)]
pub async fn fetch_report(path: String) -> Result<Value, String> {
    if TOKEN.is_empty() {
        return Err("В этой сборке нет токена доступа к отчётам".to_string());
    }
    if path.contains("..") || !path.ends_with(".json") {
        return Err("Некорректный путь отчёта".to_string());
    }
    let resp = client()?
        .get(contents_url(&path))
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github.raw+json")
        .bearer_auth(TOKEN)
        .send()
        .await
        .map_err(|e| format!("Нет связи: {e}"))?;
    if !resp.status().is_success() {
        return Err(map_status(resp.status().as_u16()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("Отчёт повреждён: {e}"))
}

/// Отправляет накопленное в очереди; на первой же неудаче останавливается.
async fn flush(client: &reqwest::Client) -> usize {
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
        if post(client, &body).await.is_err() {
            break;
        }
        let _ = std::fs::remove_file(&path);
        sent += 1;
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
    let client = client()?;
    match post(&client, &envelope).await {
        Ok(()) => {
            flush(&client).await;
            Ok("sent".to_string())
        }
        Err(reason) => {
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
    Ok(flush(&client()?).await)
}
