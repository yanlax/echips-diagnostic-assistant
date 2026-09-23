// Проверка и загрузка обновления самого приложения.
//
// В echips-driver-assistant это сделано через Яндекс.Диск (version.json с
// полями latest_version/yandex_public_key) — там это оправдано: тот же
// Диск уже используется как канал для драйверов, инфраструктура
// переиспользуется. У этого проекта своя инфраструктура: GitHub Releases
// УЖЕ единственный канал распространения (.github/workflows/build.yml
// публикует .exe при пуше тега vX.Y.Z, см. CLAUDE.md про версии) — заводить
// ещё и Яндекс.Диск для того же самого файла было бы лишней сущностью без
// выгоды. Поэтому здесь GitHub Releases API вместо Яндекс.Диска.
//
// UX — тот же, что в driver-assistant (баннер "Доступна версия X",
// кнопка "Скачать" → прогресс-бар → открыть папку с файлом): скачивание
// в %USERPROFILE%\Downloads с событием прогресса "app-update-progress",
// открытие папки — уже готовой командой report::open_containing_folder
// (тот же приём /select, что и там, не дублируем).

use serde::Serialize;
use std::io::Write;
use tauri::{Emitter, Window};

const REPO: &str = "yanlax/echips-diagnostic-assistant";

#[derive(Debug, Serialize, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
    pub file_name: String,
    pub notes: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: u64,
}

/// "v0.12.0" -> [0,12,0]; нечисловые/отсутствующие сегменты — 0.
fn parse_version(tag: &str) -> Vec<u32> {
    tag.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}
fn is_newer(latest: &str, current: &str) -> bool {
    let l = parse_version(latest);
    let c = parse_version(current);
    for i in 0..l.len().max(c.len()) {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv != cv {
            return lv > cv;
        }
    }
    false
}

/// None — обновлений нет, либо проверка не удалась (тихо игнорируем, как
/// в driver-assistant: это фоновая проверка, она не должна мешать
/// основному сценарию диагностики никаким образом).
#[tauri::command(async)]
pub async fn check_for_update() -> Result<Option<UpdateInfo>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let client = reqwest::Client::new();
    let resp = match client.get(&url).header("User-Agent", "echips-diagnostic-app").send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Ok(None),
    };
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return Ok(None),
    };
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    if tag.is_empty() || !is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let assets = json["assets"].as_array().cloned().unwrap_or_default();
    let Some(asset) = assets.iter().find(|a| a["name"].as_str().unwrap_or("").ends_with(".exe")) else {
        return Ok(None);
    };
    Ok(Some(UpdateInfo {
        version: tag,
        download_url: asset["browser_download_url"].as_str().unwrap_or("").to_string(),
        file_name: asset["name"].as_str().unwrap_or("Echips-Hardware-Check.exe").to_string(),
        notes: json["body"].as_str().unwrap_or("").to_string(),
    }))
}

fn downloads_dir() -> std::path::PathBuf {
    let base = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Downloads")
}

#[tauri::command(async)]
pub async fn download_update(window: Window, url: String, file_name: String) -> Result<String, String> {
    use futures_util::StreamExt;
    let dest_dir = downloads_dir();
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("Не удалось создать папку загрузок: {e}"))?;
    let dest_path = dest_dir.join(&file_name);

    let resp = reqwest::get(&url).await.map_err(|e| format!("Не удалось начать загрузку: {e}"))?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(&dest_path).map_err(|e| format!("Не удалось создать файл: {e}"))?;
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Ошибка загрузки: {e}"))?;
        file.write_all(&chunk).map_err(|e| format!("Не удалось записать файл: {e}"))?;
        downloaded += chunk.len() as u64;
        let _ = window.emit("app-update-progress", UpdateProgress { downloaded, total });
    }
    Ok(dest_path.to_string_lossy().to_string())
}
