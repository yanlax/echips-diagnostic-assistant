// Профили моделей: хранятся на сервере Echips (GET/PUT /v1/profiles, см. srv.rs), поверх встроенных в
// src/profiles.js. Эталон новой модели добавляет админ кнопкой «Сохранить эталон модели» (тест «Системная
// информация») — без пересборки exe. Без входа на сервере используется копия последней загрузки
// (%LOCALAPPDATA%\\Echips\\HardwareCheck\\profiles_cache.json), а если её нет — только встроенные профили.

use super::srv;
use serde::Serialize;
use serde_json::{json, Value};

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck").join("profiles_cache.json")
}

#[derive(Debug, Serialize, Clone)]
pub struct ProfilesFile {
    /// {"models": {...}} — JS ждёт поле models
    pub profiles: Value,
    /// "server" | "cache"
    pub source: String,
}

fn parse(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v["models"].is_object() { Some(v) } else { None }
}

fn save_cache(text: &str) {
    if let Some(parent) = cache_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(cache_path(), text);
}

#[tauri::command(async)]
pub async fn fetch_profiles() -> Result<ProfilesFile, String> {
    if srv::session().is_some() {
        if let Ok((200, text)) = srv::request("GET", "/profiles", None, true).await {
            if let Some(v) = parse(&text) {
                save_cache(&text);
                return Ok(ProfilesFile { profiles: v, source: "server".into() });
            }
        }
    }
    if let Some(v) = std::fs::read_to_string(cache_path()).ok().and_then(|t| parse(&t)) {
        return Ok(ProfilesFile { profiles: v, source: "cache".into() });
    }
    Err("Профили моделей не загружены: нет входа на сервере и сохранённой копии".to_string())
}

/// Добавляет (или заменяет) профиль модели `key` на сервере — только для администратора.
#[tauri::command(async)]
pub async fn profiles_save_model(key: String, profile: Value) -> Result<ProfilesFile, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Не указан ключ модели".to_string());
    }
    if !profile.is_object() {
        return Err("Профиль должен быть объектом".to_string());
    }
    match srv::request("PUT", "/profiles", Some(&json!({ "key": key, "profile": profile })), true).await? {
        (200, text) => {
            let v = parse(&text).ok_or("Сервер вернул некорректные профили")?;
            save_cache(&text);
            Ok(ProfilesFile { profiles: v, source: "server".into() })
        }
        (code, text) => Err(srv::api_err(code, &text)),
    }
}
