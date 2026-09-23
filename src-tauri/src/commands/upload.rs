// Отправка отчётов администратору: после автопрогона и при ручном экспорте
// приложение шлёт JSON на промежуточный сервис (Cloudflare Worker, см.
// worker/reports), а тот кладёт файл в приватный репозиторий отчётов. Токена
// GitHub на машинах инженеров нет: он живёт только в настройках Worker.
//
// Секретов в приложении и в сборке нет: адрес Worker — не секрет, а защита от
// мусорных запросов — на стороне Worker (проверка формата и размера,
// ограничение частоты). Если адрес не задан или нет сети — отчёт
// сохраняется в очередь %LOCALAPPDATA%\Echips\HardwareCheck\reports_queue и
// уходит при следующей отправке или запуске приложения.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

/// Адрес развёрнутого Worker (https://<имя>.<аккаунт>.workers.dev). Пока пусто —
/// отчёты копятся в очереди и уйдут после подстановки адреса.
const REPORTS_URL: &str = "";

fn queue_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Echips").join("HardwareCheck").join("reports_queue")
}

async fn post(client: &reqwest::Client, body: &Value) -> Result<(), String> {
    if REPORTS_URL.is_empty() {
        return Err("Адрес приёма отчётов не задан в этой сборке".to_string());
    }
    let resp = client
        .post(REPORTS_URL)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Нет связи: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Сервер отчётов вернул {}", resp.status().as_u16()))
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
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
