// Отправка отчётов администратору: после автопрогона и при ручном экспорте
// приложение кладёт JSON прямо в приватный репозиторий отчётов
// (yanlax/echips-reports) через GitHub Contents API, по структуре
// <инженер>/<дата диагностики>/<время начала>_<серийник>.json и рядом .pdf;
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
    // Структура: <инженер>/<дата диагностики>/<время начала>_<серийник>.{json,pdf}
    let base = format!("{}/{}/{}_{}", engineer, started.format("%Y-%m-%d"), started.format("%H%M%S"), serial);
    let message = format!(
        "Отчёт: {} / {} ({})",
        safe(report["device_model"].as_str().unwrap_or("")),
        engineer,
        kind
    );

    let pretty = serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?;
    put_file(client, &format!("{base}.json"), pretty.as_bytes(), &message).await?;

    // PDF — тот же, что «Экспорт PDF» (тема «Графит»). Ошибка самой сборки PDF
    // (не сети) не должна вечно держать отчёт в очереди — тогда остаётся
    // хотя бы JSON.
    if let Ok(rep) = serde_json::from_value::<super::report::DiagnosticReport>(report.clone()) {
        if let Ok(pdf) = super::report::render_pdf(&rep) {
            put_file(client, &format!("{base}.pdf"), &pdf, &message).await?;
        }
    }
    Ok(())
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
