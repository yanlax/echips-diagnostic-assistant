// Профили моделей из git: файл data/profiles.json в публичном репозитории (как data/techs.json).
// Приложение при запуске скачивает его и подмешивает к профилям из src/profiles.js, поэтому эталон
// новой модели добавляется правкой файла (или кнопкой админа «Сохранить эталон модели» в тесте
// «Системная информация») без пересборки exe.
//
// Порядок источников: GitHub raw (с параметром против кэша) → кэш последнего успешного чтения →
// вшитая на момент сборки копия. Запись (админ, токен DPAPI из techs.rs) — через GitHub Contents API.

use super::techs::{b64_encode, gh_error, read_token};
use serde::Serialize;
use serde_json::{json, Value};

const RAW_URL: &str = "https://raw.githubusercontent.com/yanlax/echips-diagnostic-assistant/main/data/profiles.json";
const API_URL: &str = "https://api.github.com/repos/yanlax/echips-diagnostic-assistant/contents/data/profiles.json";
const BUILTIN: &str = include_str!("../../../data/profiles.json");

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck").join("profiles_cache.json")
}

#[derive(Debug, Serialize, Clone)]
pub struct ProfilesFile {
    pub profiles: Value,
    /// "raw" | "cache" | "builtin"
    pub source: String,
}

fn parse(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v.is_object() { Some(v) } else { None }
}

#[tauri::command(async)]
pub async fn fetch_profiles() -> Result<ProfilesFile, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;
    let stamp = chrono::Utc::now().timestamp_millis();
    if let Ok(resp) = client
        .get(format!("{RAW_URL}?nocache={stamp}"))
        .header("Cache-Control", "no-cache")
        .header("User-Agent", "echips-diagnostic-app")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Some(v) = parse(&text) {
                    if let Some(parent) = cache_path().parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(cache_path(), &text);
                    return Ok(ProfilesFile { profiles: v, source: "raw".into() });
                }
            }
        }
    }
    if let Some(v) = std::fs::read_to_string(cache_path()).ok().and_then(|t| parse(&t)) {
        return Ok(ProfilesFile { profiles: v, source: "cache".into() });
    }
    parse(BUILTIN)
        .map(|v| ProfilesFile { profiles: v, source: "builtin".into() })
        .ok_or_else(|| "Вшитый файл профилей повреждён".to_string())
}

/// Добавляет (или заменяет) профиль модели `key` в data/profiles.json — только для администратора
/// (нужен токен из экрана «Инженеры»). Возвращает обновлённый файл.
#[tauri::command(async)]
pub async fn profiles_save_model(key: String, profile: Value) -> Result<ProfilesFile, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Не указан ключ модели".to_string());
    }
    if !profile.is_object() {
        return Err("Профиль должен быть объектом".to_string());
    }
    let token = read_token().ok_or("Сначала введите GitHub-токен на экране «Инженеры»")?;
    let client = reqwest::Client::new();
    let get = |accept: &'static str| {
        client
            .get(API_URL)
            .query(&[("ref", "main")])
            .header("User-Agent", "echips-diagnostic-app")
            .header("Accept", accept)
            .bearer_auth(&token)
    };

    // sha нужен для обновления; файла ещё может не быть (404) — тогда создаём
    let meta = get("application/vnd.github+json").send().await.map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    let (sha, mut file): (Option<String>, Value) = match meta.status().as_u16() {
        200 => {
            let m: Value = meta.json().await.map_err(|e| e.to_string())?;
            let sha = m["sha"].as_str().map(|s| s.to_string());
            let raw = get("application/vnd.github.raw+json").send().await.map_err(|e| format!("Нет связи с GitHub: {e}"))?;
            if !raw.status().is_success() {
                return Err(gh_error(raw.status()));
            }
            let text = raw.text().await.map_err(|e| e.to_string())?;
            (sha, parse(&text).ok_or("profiles.json в репозитории повреждён")?)
        }
        404 => (None, json!({ "models": {} })),
        _ => return Err(gh_error(meta.status())),
    };

    if !file["models"].is_object() {
        file["models"] = json!({});
    }
    file["models"][key.as_str()] = profile;

    let mut body = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    body.push('\n');
    let mut req = json!({
        "message": format!("Профиль модели {key} (эталон из приложения)"),
        "content": b64_encode(body.as_bytes()),
        "branch": "main",
    });
    if let Some(sha) = sha {
        req["sha"] = json!(sha);
    }
    let put = client
        .put(API_URL)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(&token)
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !put.status().is_success() {
        return Err(gh_error(put.status()));
    }
    if let Some(parent) = cache_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(cache_path(), &body);
    Ok(ProfilesFile { profiles: file, source: "raw".into() })
}
