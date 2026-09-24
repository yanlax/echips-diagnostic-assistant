// Общий PIN-экран при запуске программы (см. CLAUDE.md, задача №2): список
// инженеров и хэшей их PIN лежит не в коде, а в публичном репозитории
// (data/techs.json на GitHub) — тот же принцип, что уже работает для
// авто-обновления (см. update.rs): правка файла в репозитории вместо
// пересборки и релиза приложения. Список подтягивается заново при каждом
// запуске, так что новый инженер (или изменённый PIN) появляется сразу на
// всех станциях без обновления .exe.
//
// Здесь — только скачивание и кэширование списка. Сам PIN нигде не хранится
// и не передаётся в открытом виде: только SHA-256(salt+":"+pin), сравнение
// введённого PIN с хэшем происходит на стороне JS (src/app.js, sha256Hex +
// lockSubmit) через Web Crypto API — тем же способом, каким этот хэш и
// генерируется на экране "Добавить инженера" (app.js, techadminGenerate),
// поэтому дублировать алгоритм хэширования в Rust не нужно.
//
// Файл в репозитории — публичный, как и сами релизы. Это осознанный выбор
// (см. обсуждение с пользователем): PIN нужен только как экран входа для
// сервисной станции, а не как криптографическая защита данных, поэтому
// достаточно, чтобы сам PIN нельзя было восстановить из хэша (соль + SHA-256
// делают перебор по радужным таблицам бессмысленным; для реальной защиты от
// прямого перебора PIN должен быть длиннее 4 цифр — см. пример в data/techs.json).

use serde::{Deserialize, Serialize};

fn default_role() -> String {
    "tech".to_string()
}

/// role: "tech" (по умолчанию, если поля нет в старой записи/кэше — обратная
/// совместимость) или "admin" — админские фичи (например, панель команд по
/// Shift+F10) показываются только при role=="admin", см. app.js.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tech {
    pub id: String,
    pub name: String,
    pub pin_hash: String,
    pub salt: String,
    #[serde(default = "default_role")]
    pub role: String,
}

const TECHS_URL: &str =
    "https://raw.githubusercontent.com/yanlax/echips-diagnostic-assistant/main/data/techs.json";

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base)
        .join("Echips")
        .join("HardwareCheck")
        .join("techs_cache.json")
}

/// Список инженеров на момент сборки (хэши PIN, как в репозитории) — запасной вариант входа без сети.
const BUILTIN_TECHS: &str = include_str!("../../../data/techs.json");
/// Выключить (false) при выпуске программы всем сервисам: тогда без сети и без кэша войти нельзя.
const ALLOW_BUILTIN_TECHS: bool = true;

/// Список + откуда он взят — экран входа показывает предупреждение, если это
/// кэш (иначе «новый инженер не виден, а почему — непонятно»).
#[derive(Debug, Serialize, Clone)]
pub struct TechList {
    pub techs: Vec<Tech>,
    /// "api" | "raw" | "cache" | "builtin"
    pub source: String,
    pub note: String,
}

async fn get_list(client: &reqwest::Client, url: &str, accept: &str) -> Option<(Vec<Tech>, String)> {
    let resp = client
        .get(url)
        .header("Cache-Control", "no-cache")
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", accept)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().await.ok()?;
    let list = serde_json::from_str::<Vec<Tech>>(&text).ok()?;
    Some((list, text))
}

/// Порядок источников: 1) GitHub API — тот же хост, через который приложение
/// само записывает список, всегда свежая версия (без токена лимит 60
/// запросов/час на IP); 2) raw.githubusercontent.com с уникальным параметром
/// (обход его 5-минутного кэша; в части сетей этот хост недоступен, а
/// api.github.com работает); 3) локальный кэш последнего успешного чтения.
/// У каждого запроса таймаут — раньше при недоступном хосте загрузка висела.
#[tauri::command(async)]
pub async fn fetch_techs() -> Result<TechList, String> {
    // Без интернета вход не должен ждать долго: connect 3 с, запрос 6 с (два источника — до ~12 с).
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;
    let stamp = chrono::Utc::now().timestamp_millis();

    let fresh = match get_list(
        &client,
        &format!("{API_URL}?ref=main&t={stamp}"),
        "application/vnd.github.raw+json",
    )
    .await
    {
        Some(r) => Some((r, "api")),
        None => get_list(&client, &format!("{TECHS_URL}?nocache={stamp}"), "*/*").await.map(|r| (r, "raw")),
    };

    if let Some(((list, text), source)) = fresh {
        if let Some(parent) = cache_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(cache_path(), &text);
        return Ok(TechList { techs: list, source: source.to_string(), note: String::new() });
    }

    if let Ok(text) = std::fs::read_to_string(cache_path()) {
        if let Ok(list) = serde_json::from_str::<Vec<Tech>>(&text) {
            return Ok(TechList {
                techs: list,
                source: "cache".to_string(),
                note: "Нет связи с GitHub — использован сохранённый список; недавно добавленных инженеров в нём может не быть.".to_string(),
            });
        }
    }

    // Совсем без сети и без кэша (первый запуск на машине без интернета, WinPE): список, вшитый в
    // exe при сборке (data/techs.json на момент сборки). ВРЕМЕННО, на время тестирования двумя
    // инженерами (Максим и Алексей) — для выпуска всем сервисам с чётким контролем доступа
    // выключить: ALLOW_BUILTIN_TECHS = false.
    if ALLOW_BUILTIN_TECHS {
        if let Ok(list) = serde_json::from_str::<Vec<Tech>>(BUILTIN_TECHS) {
            return Ok(TechList {
                techs: list,
                source: "builtin".to_string(),
                note: "Нет связи с GitHub и нет сохранённого списка — использован список, вшитый в программу; новые инженеры появятся при подключении к интернету.".to_string(),
            });
        }
    }
    Err("Нет сети и нет ранее сохранённого списка инженеров. Подключите станцию к интернету хотя бы один раз.".to_string())
}

// ---------- управление списком из приложения (только администратор) ----------
//
// Токен GitHub (fine-grained, один репозиторий, Contents: write) админ вводит
// один раз — он хранится локально в %LOCALAPPDATA%\Echips\HardwareCheck\
// admin_token.dat (зашифрован DPAPI, см. ниже) и в exe/репозиторий не попадает. Обычные техники токена не
// имеют, поэтому писать в data/techs.json могут только с машины админа.
// Запись — через GitHub Contents API (GET sha + текущее содержимое, затем PUT
// с новым содержимым и тем же sha: если файл успели изменить, GitHub вернёт
// 409 и мы ничего не затрём).

const API_URL: &str =
    "https://api.github.com/repos/yanlax/echips-diagnostic-assistant/contents/data/techs.json";

fn token_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck")
}

/// Токен хранится зашифрованным через DPAPI (ProtectedData, область
/// CurrentUser): расшифровать его может только тот же пользователь Windows на
/// этом же компьютере — копия файла на другой машине/под другим пользователем
/// бесполезна. Файл — base64 от зашифрованного блока.
fn token_path() -> std::path::PathBuf {
    token_dir().join("admin_token.dat")
}

/// Открытый файл из версий до 0.18 — при первом чтении переезжает в DPAPI.
fn legacy_token_path() -> std::path::PathBuf {
    token_dir().join("admin_token.txt")
}

#[cfg(target_os = "windows")]
fn dpapi(protect: bool, input_b64: &str) -> Result<String, String> {
    // input_b64 — только base64 (A–Z a–z 0–9 + / = - _), в одинарные кавычки безопасно.
    let method = if protect { "Protect" } else { "Unprotect" };
    let script = format!(
        "Add-Type -AssemblyName System.Security; \
         $b = [Convert]::FromBase64String('{input_b64}'); \
         [Convert]::ToBase64String([System.Security.Cryptography.ProtectedData]::{method}($b, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser))"
    );
    crate::powershell::run_ps(&script).map(|o| o.trim().to_string())
}

#[cfg(not(target_os = "windows"))]
fn dpapi(_protect: bool, _input_b64: &str) -> Result<String, String> {
    Err("Шифрование токена доступно только в Windows-сборке".to_string())
}

fn store_token(token: &str) -> Result<(), String> {
    let cipher = dpapi(true, &b64_encode(token.as_bytes()))?;
    std::fs::create_dir_all(token_dir()).map_err(|e| e.to_string())?;
    std::fs::write(token_path(), cipher).map_err(|e| format!("Не удалось сохранить токен: {e}"))
}

fn read_token() -> Option<String> {
    if let Ok(cipher) = std::fs::read_to_string(token_path()) {
        let plain_b64 = dpapi(false, cipher.trim()).ok()?;
        let bytes = b64_decode(&plain_b64)?;
        return String::from_utf8(bytes).ok().filter(|t| !t.trim().is_empty());
    }
    let legacy = std::fs::read_to_string(legacy_token_path()).ok()?;
    let t = legacy.trim().to_string();
    if t.is_empty() {
        return None;
    }
    if store_token(&t).is_ok() {
        let _ = std::fs::remove_file(legacy_token_path());
    }
    Some(t)
}

#[tauri::command]
pub fn techs_token_status() -> bool {
    token_path().exists() || legacy_token_path().exists()
}

#[tauri::command]
pub fn techs_save_token(token: String) -> Result<(), String> {
    let t = token.trim();
    if t.len() < 20 || t.chars().any(|c| c.is_whitespace()) {
        return Err("Похоже, это не токен GitHub (слишком короткий или с пробелами)".to_string());
    }
    store_token(t)?;
    let _ = std::fs::remove_file(legacy_token_path());
    Ok(())
}

#[tauri::command]
pub fn techs_clear_token() {
    let _ = std::fs::remove_file(token_path());
    let _ = std::fs::remove_file(legacy_token_path());
}

pub fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i as u32);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

fn gh_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 => "Токен недействителен или истёк — введите новый".to_string(),
        403 | 404 => "Нет доступа к репозиторию: у токена должно быть право Contents: write на yanlax/echips-diagnostic-assistant".to_string(),
        409 | 422 => "Файл в репозитории изменили параллельно — повторите действие".to_string(),
        s => format!("GitHub вернул ошибку {s}"),
    }
}

/// Читает актуальный список и sha, применяет правку, коммитит. Возвращает
/// новый список (чтобы приложение сразу показало его, не дожидаясь CDN raw).
async fn commit_list<F>(message: String, edit: F) -> Result<Vec<Tech>, String>
where
    F: Send + FnOnce(&mut Vec<Tech>) -> Result<(), String>,
{
    let token = read_token().ok_or("Сначала введите GitHub-токен")?;
    let client = reqwest::Client::new();
    let get = |accept: &'static str| {
        client
            .get(API_URL)
            .query(&[("ref", "main")])
            .header("User-Agent", "echips-diagnostic-app")
            .header("Accept", accept)
            .bearer_auth(&token)
    };

    let meta = get("application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !meta.status().is_success() {
        return Err(gh_error(meta.status()));
    }
    let meta: serde_json::Value = meta.json().await.map_err(|e| e.to_string())?;
    let sha = meta["sha"].as_str().ok_or("GitHub не вернул sha файла")?.to_string();

    let raw = get("application/vnd.github.raw+json")
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !raw.status().is_success() {
        return Err(gh_error(raw.status()));
    }
    let mut list: Vec<Tech> = serde_json::from_str(&raw.text().await.map_err(|e| e.to_string())?)
        .map_err(|e| format!("techs.json в репозитории повреждён: {e}"))?;

    edit(&mut list)?;

    let mut body = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    body.push('\n');
    let put = client
        .put(API_URL)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "message": message,
            "content": b64_encode(body.as_bytes()),
            "sha": sha,
            "branch": "main",
        }))
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !put.status().is_success() {
        return Err(gh_error(put.status()));
    }
    let _ = std::fs::write(cache_path(), &body);
    Ok(list)
}

/// Добавляет инженера или (если id уже есть) заменяет запись — так же меняется PIN.
#[tauri::command(async)]
pub async fn techs_upsert(tech: Tech) -> Result<Vec<Tech>, String> {
    let msg = format!("Инженеры: {} ({})", tech.name, tech.id);
    commit_list(msg, move |list| {
        match list.iter_mut().find(|t| t.id == tech.id) {
            Some(existing) => *existing = tech,
            None => list.push(tech),
        }
        Ok(())
    })
    .await
}

#[tauri::command(async)]
pub async fn techs_remove(id: String) -> Result<Vec<Tech>, String> {
    let msg = format!("Инженеры: удалён {id}");
    commit_list(msg, move |list| {
        let before = list.len();
        list.retain(|t| t.id != id);
        if list.len() == before {
            return Err("Такого инженера уже нет в списке".to_string());
        }
        if !list.iter().any(|t| t.role == "admin") {
            return Err("Нельзя удалить последнего администратора".to_string());
        }
        Ok(())
    })
    .await
}
