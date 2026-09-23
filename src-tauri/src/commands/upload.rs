// Отправка отчётов администратору: после автопрогона и при ручном экспорте
// приложение кладёт JSON прямо в приватный репозиторий отчётов
// (yanlax/echips-reports) через GitHub Contents API, по структуре
// <инженер>/<дата диагностики>/<время>_<серийник>_<auto|manual>.json.
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

async fn post(client: &reqwest::Client, envelope: &Value) -> Result<(), String> {
    if TOKEN.is_empty() {
        return Err("В этой сборке нет токена отправки отчётов".to_string());
    }
    let report = &envelope["report"];
    let serial = safe(report["device_serial"].as_str().unwrap_or(""));
    let engineer = safe(report["engineer"].as_str().unwrap_or(""));
    let kind = safe(envelope["kind"].as_str().unwrap_or(""));
    let now = chrono::Local::now();
    // Папка дня — дата самой диагностики (начало прогона), а не отправки:
    // отчёт из очереди, ушедший на следующий день, всё равно ляжет в свой день.
    let day = report["started_at"]
        .as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&chrono::Local))
        .unwrap_or(now)
        .format("%Y-%m-%d")
        .to_string();
    // Структура: <инженер>/<дата диагностики>/<время отправки>_<серийник>_<тип>.json
    let path = format!("{}/{}/{}_{}_{}.json", engineer, day, now.format("%H%M%S%3f"), serial, kind);
    let url_path: Vec<String> = path.split('/').map(|s| urlencoding::encode(s).into_owned()).collect();
    let url = format!("https://api.github.com/repos/{REPO}/contents/{}", url_path.join("/"));
    let pretty = serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?;
    let message = format!(
        "Отчёт: {} / {} ({})",
        safe(report["device_model"].as_str().unwrap_or("")),
        engineer,
        kind
    );

    let resp = client
        .put(&url)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(TOKEN)
        .json(&json!({ "message": message, "content": b64_encode(pretty.as_bytes()) }))
        .send()
        .await
        .map_err(|e| format!("Нет связи: {e}"))?;
    match resp.status().as_u16() {
        200 | 201 => Ok(()),
        401 => Err("Токен отправки отчётов недействителен или истёк".to_string()),
        403 | 404 => Err("Нет доступа к репозиторию отчётов".to_string()),
        s => Err(format!("GitHub вернул {s}")),
    }
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
