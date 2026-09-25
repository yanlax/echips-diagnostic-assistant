// Профили моделей: подписанный файл _config/profiles.json в ПРИВАТНОМ репозитории echips-reports
// (тот же ключ подписи, что у списка инженеров, см. techs.rs / techs_sign.rs). Приложение при запуске
// скачивает его вшитым токеном отчётов и подмешивает к профилям из src/profiles.js, поэтому эталон
// новой модели добавляется кнопкой админа «Сохранить эталон модели» (тест «Системная информация»)
// без пересборки exe и без ввода токена — нужен только ключ подписи.
//
// Порядок источников (берётся самая свежая версия с верной подписью): GitHub → файл profiles_signed.json
// рядом с exe / в %LOCALAPPDATA% → список, вшитый при сборке (src-tauri/assets/profiles_baked.json).

use super::techs::{b64_encode, copy_dirs, gh_error, sign_to_text, verify_signed_text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API_URL: &str = "https://api.github.com/repos/yanlax/echips-reports/contents/_config/profiles.json";
const BAKED: &str = include_str!("../../assets/profiles_baked.json");
const FILE: &str = "profiles_signed.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Payload {
    version: u64,
    issued_at: String,
    models: Value,
}

#[derive(Debug, Serialize, Clone)]
pub struct ProfilesFile {
    /// {"models": {...}} — как раньше, JS ждёт поле models
    pub profiles: Value,
    /// "github" | "cache" | "builtin"
    pub source: String,
}

fn parse_signed(text: &str) -> Option<Payload> {
    let p: Payload = serde_json::from_str(&verify_signed_text(text)?).ok()?;
    if p.models.is_object() { Some(p) } else { None }
}

fn write_copies(text: &str) {
    for d in copy_dirs() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join(FILE), text);
    }
}

fn as_file(p: &Payload, source: &str) -> ProfilesFile {
    ProfilesFile { profiles: json!({ "models": p.models }), source: source.to_string() }
}

async fn fetch_remote() -> Option<String> {
    let token = super::upload::report_token();
    if token.is_empty() {
        return None;
    }
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .ok()?;
    let resp = client
        .get(API_URL)
        .header("Cache-Control", "no-cache")
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github.raw+json")
        .bearer_auth(token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

#[tauri::command(async)]
pub async fn fetch_profiles() -> Result<ProfilesFile, String> {
    // (версия, приоритет источника, источник, payload, исходный текст)
    let mut cands: Vec<(u64, u8, &'static str, Payload, String)> = Vec::new();
    if let Some(p) = parse_signed(BAKED) {
        cands.push((p.version, 0, "builtin", p, BAKED.to_string()));
    }
    for d in copy_dirs() {
        if let Ok(t) = std::fs::read_to_string(d.join(FILE)) {
            if let Some(p) = parse_signed(&t) {
                cands.push((p.version, 1, "cache", p, t));
            }
        }
    }
    if let Some(t) = fetch_remote().await {
        if let Some(p) = parse_signed(&t) {
            cands.push((p.version, 2, "github", p, t));
        }
    }
    cands.sort_by_key(|c| (c.0, c.1));
    let (_, _, source, payload, text) = cands.pop().ok_or("Нет файла профилей с верной подписью".to_string())?;
    if source == "github" {
        write_copies(&text);
    }
    Ok(as_file(&payload, source))
}

/// Добавляет (или заменяет) профиль модели `key` в _config/profiles.json — только для администратора
/// (нужен ключ подписи с экрана «Инженеры»; записывается вшитым токеном отчётов). Возвращает обновлённый файл.
#[tauri::command(async)]
pub async fn profiles_save_model(key: String, profile: Value) -> Result<ProfilesFile, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Не указан ключ модели".to_string());
    }
    if !profile.is_object() {
        return Err("Профиль должен быть объектом".to_string());
    }
    let token = super::upload::report_token();
    if token.is_empty() {
        return Err("В программу не вшит токен отчётов — сборка без секрета ECHIPS_REPORTS_TOKEN".to_string());
    }
    let client = reqwest::Client::new();
    let get = |accept: &'static str| {
        client
            .get(API_URL)
            .header("User-Agent", "echips-diagnostic-app")
            .header("Accept", accept)
            .bearer_auth(token)
    };

    let meta = get("application/vnd.github+json").send().await.map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    let (sha, mut cur): (String, Payload) = match meta.status().as_u16() {
        200 => {
            let m: Value = meta.json().await.map_err(|e| e.to_string())?;
            let sha = m["sha"].as_str().ok_or("GitHub не вернул sha файла")?.to_string();
            let raw = get("application/vnd.github.raw+json").send().await.map_err(|e| format!("Нет связи с GitHub: {e}"))?;
            if !raw.status().is_success() {
                return Err(gh_error(raw.status()));
            }
            let text = raw.text().await.map_err(|e| e.to_string())?;
            (sha, parse_signed(&text).ok_or("Файл профилей в репозитории повреждён или подписан другим ключом — правка отменена")?)
        }
        _ => return Err(gh_error(meta.status())),
    };

    cur.models[key.as_str()] = profile;
    cur.version += 1;
    cur.issued_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let body = sign_to_text(serde_json::to_string(&cur).map_err(|e| e.to_string())?)?;

    let put = client
        .put(API_URL)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .json(&json!({
            "message": format!("Профиль модели {key} (эталон из приложения)"),
            "content": b64_encode(body.as_bytes()),
            "sha": sha,
        }))
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !put.status().is_success() {
        return Err(gh_error(put.status()));
    }
    write_copies(&body);
    Ok(as_file(&cur, "github"))
}
