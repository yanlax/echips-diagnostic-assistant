// Вход по PIN (см. CLAUDE.md): список инженеров и хэшей PIN хранится в ПРИВАТНОМ репозитории
// отчётов (yanlax/echips-reports, файл _config/techs.json) и ПОДПИСАН администратором
// (ECDSA P-256, см. techs_sign.rs). Публичный ключ вшит в exe, приватный есть только у админа.
//
// Вход работает без интернета. Источники списка (берётся самая свежая версия с верной подписью):
//   1) GitHub — при запуске, если есть сеть (читается вшитым токеном отчётов);
//   2) файл techs_signed.json рядом с exe (на флешке — переезжает вместе с программой)
//      и копия в %LOCALAPPDATA%\Echips\HardwareCheck;
//   3) список, вшитый в exe при сборке (CI скачивает его из репозитория, src-tauri/assets/techs_baked.json).
// Подмена файла на флешке бессмысленна: без приватного ключа подпись не сделать.
//
// Срок годности: если список не подтверждали в сети (и exe собран) больше 7 суток, войти могут
// только администраторы — уволенный инженер не остаётся в допуске навсегда. Сравнение PIN с хэшем
// делает JS (app.js, sha256Hex + lockSubmit), тем же способом, что и генерация хэша на экране «Инженеры».

use super::techs_sign as sig;
use serde::{Deserialize, Serialize};

fn default_role() -> String {
    "tech".to_string()
}

/// role: "tech" (по умолчанию) или "admin" — админские вкладки («История», «Инженеры»),
/// панель команд по Shift+F10 показываются только при role=="admin", см. app.js.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tech {
    pub id: String,
    pub name: String,
    pub pin_hash: String,
    pub salt: String,
    #[serde(default = "default_role")]
    pub role: String,
}

/// Содержимое подписанного файла (строка payload внутри {"payload":..., "sig":...}).
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Payload {
    version: u64,
    issued_at: String,
    techs: Vec<Tech>,
}

/// Публичный ключ подписи (data/techs_pub.txt, hex, SEC1) и список на момент сборки.
const PUB_HEX: &str = include_str!("../../../data/techs_pub.txt");
const BAKED: &str = include_str!("../../assets/techs_baked.json");
/// Время сборки (unix-секунды) — CI задаёт ECHIPS_BUILD_UNIX; без него вшитый список считается «без даты».
const BAKED_AT: &str = match option_env!("ECHIPS_BUILD_UNIX") {
    Some(v) => v,
    None => "0",
};
const REPO: &str = "yanlax/echips-reports";
const LIST_PATH: &str = "_config/techs.json";
const LIST_FILE: &str = "techs_signed.json";
const STAMP_FILE: &str = "techs_sync.json";

fn local_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck")
}

fn exe_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Где хранить копии: рядом с exe (флешка) и в %LOCALAPPDATA%.
fn store_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = exe_dir() {
        v.push(d);
    }
    v.push(local_dir());
    v
}

/// Разбор и проверка подписи; None — файл повреждён или подписан не нашим ключом.
fn parse_signed(text: &str) -> Option<Payload> {
    let s: sig::Signed = serde_json::from_str(text).ok()?;
    if !sig::verify(PUB_HEX.trim(), &s.payload, &s.sig) {
        return None;
    }
    serde_json::from_str::<Payload>(&s.payload).ok()
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Serialize, Deserialize, Default)]
struct Stamp {
    #[serde(default)]
    last_ok: i64,
    #[serde(default)]
    max_seen: i64,
}

fn read_stamps() -> Vec<Stamp> {
    store_dirs()
        .iter()
        .filter_map(|d| std::fs::read_to_string(d.join(STAMP_FILE)).ok())
        .filter_map(|t| serde_json::from_str::<Stamp>(&t).ok())
        .collect()
}

/// Обновляет метки: max_seen (максимум виденного времени, защита от отката часов) и, если
/// список только что подтверждён в сети, last_ok.
fn write_stamps(old: &[Stamp], now: i64, online: bool) {
    let max_seen = old.iter().map(|s| s.max_seen).chain(std::iter::once(now)).max().unwrap_or(now);
    let last_ok = if online { now } else { old.iter().map(|s| s.last_ok).filter(|t| *t <= now + 86400).max().unwrap_or(0) };
    let text = serde_json::to_string(&Stamp { last_ok, max_seen }).unwrap_or_default();
    for d in store_dirs() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join(STAMP_FILE), &text);
    }
}

fn write_list_copies(text: &str) {
    for d in store_dirs() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join(LIST_FILE), text);
    }
}

/// Список + откуда он взят и не просрочен ли (экран входа).
#[derive(Debug, Serialize, Clone)]
pub struct TechList {
    pub techs: Vec<Tech>,
    /// "github" | "cache" | "builtin"
    pub source: String,
    pub note: String,
    /// Список не подтверждали в сети более 7 суток (или часы откатывали): войти могут только админы.
    pub expired: bool,
    /// Сколько полных суток назад список подтверждён (-1 — неизвестно).
    pub age_days: i64,
    pub version: u64,
}

async fn fetch_remote(token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .ok()?;
    let resp = client
        .get(format!("https://api.github.com/repos/{REPO}/contents/{LIST_PATH}"))
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
pub async fn fetch_techs() -> Result<TechList, String> {
    let now = now_unix();
    let stamps = read_stamps();

    // (версия, приоритет источника, источник, payload, исходный текст)
    let mut cands: Vec<(u64, u8, &'static str, Payload, String)> = Vec::new();
    if let Some(p) = parse_signed(BAKED) {
        cands.push((p.version, 0, "builtin", p, BAKED.to_string()));
    }
    for d in store_dirs() {
        if let Ok(t) = std::fs::read_to_string(d.join(LIST_FILE)) {
            if let Some(p) = parse_signed(&t) {
                cands.push((p.version, 1, "cache", p, t));
            }
        }
    }
    let online = match fetch_remote(super::upload::report_token()).await {
        Some(t) => match parse_signed(&t) {
            Some(p) => {
                cands.push((p.version, 2, "github", p, t));
                true
            }
            None => false,
        },
        None => false,
    };

    cands.sort_by_key(|c| (c.0, c.1));
    let (version, _, source, payload, text) = cands.pop().ok_or(
        "Нет списка инженеров с верной подписью. Подключите станцию к интернету или обновите программу.".to_string(),
    )?;
    if online {
        write_list_copies(&text);
    }
    let baked_at = BAKED_AT.trim().parse::<i64>().unwrap_or(0);
    let fr = sig::freshness(
        now,
        &stamps.iter().map(|s| s.last_ok).chain(if online { Some(now) } else { None }).collect::<Vec<_>>(),
        &stamps.iter().map(|s| s.max_seen).collect::<Vec<_>>(),
        baked_at,
    );
    write_stamps(&stamps, now, online);

    let note = if source == "github" {
        String::new()
    } else if fr.clock_rollback {
        "Часы компьютера сдвинуты назад — войти может только администратор.".to_string()
    } else if source == "builtin" {
        "Нет связи с GitHub — использован список, вшитый в программу; изменения появятся при подключении к интернету.".to_string()
    } else {
        "Нет связи с GitHub — использован сохранённый список; изменения появятся при подключении к интернету.".to_string()
    };
    Ok(TechList { techs: payload.techs, source: source.to_string(), note, expired: fr.expired, age_days: fr.age_days, version })
}

// ---------- управление списком из приложения (только администратор) ----------
//
// Нужны ДВА секрета, оба хранятся только на компьютере администратора (DPAPI, CurrentUser):
//   * токен GitHub (fine-grained, Contents: write на yanlax/echips-reports) — запись файла;
//   * ключ подписи (hex, 64 символа) — им подписывается список; без него остальные exe список не примут.
// Запись — через GitHub Contents API (GET sha + содержимое, затем PUT с тем же sha: если файл успели
// изменить, GitHub вернёт 409 и мы ничего не затрём).

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

pub(crate) fn read_token() -> Option<String> {
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


// ---------- ключ подписи ----------

fn signing_key_path() -> std::path::PathBuf {
    token_dir().join("signing_key.dat")
}

fn read_signing_key() -> Option<String> {
    let cipher = std::fs::read_to_string(signing_key_path()).ok()?;
    let plain_b64 = dpapi(false, cipher.trim()).ok()?;
    let bytes = b64_decode(&plain_b64)?;
    String::from_utf8(bytes).ok().filter(|t| !t.trim().is_empty())
}

#[tauri::command]
pub fn techs_signing_status() -> bool {
    signing_key_path().exists()
}

#[tauri::command]
pub fn techs_save_signing_key(key: String) -> Result<(), String> {
    let k = key.trim().to_lowercase();
    let public = sig::public_of(&k)?;
    if public != PUB_HEX.trim().to_lowercase() {
        return Err("Этот ключ не подходит к публичному ключу, вшитому в программу".to_string());
    }
    let cipher = dpapi(true, &b64_encode(k.as_bytes()))?;
    std::fs::create_dir_all(token_dir()).map_err(|e| e.to_string())?;
    std::fs::write(signing_key_path(), cipher).map_err(|e| format!("Не удалось сохранить ключ: {e}"))
}

#[tauri::command]
pub fn techs_clear_signing_key() {
    let _ = std::fs::remove_file(signing_key_path());
}

const API_URL_FMT: &str = "https://api.github.com/repos/yanlax/echips-reports/contents/_config/techs.json";

pub(crate) fn gh_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 => "Токен недействителен или истёк — введите новый".to_string(),
        403 | 404 => "Нет доступа к репозиторию: у токена должно быть право Contents: write на yanlax/echips-reports".to_string(),
        409 | 422 => "Файл в репозитории изменили параллельно — повторите действие".to_string(),
        s => format!("GitHub вернул ошибку {s}"),
    }
}

/// Читает актуальный список и sha, применяет правку, подписывает и коммитит. Возвращает
/// новый список (чтобы приложение сразу показало его).
async fn commit_list<F>(message: String, edit: F) -> Result<Vec<Tech>, String>
where
    F: Send + FnOnce(&mut Vec<Tech>) -> Result<(), String>,
{
    let token = read_token().ok_or("Сначала введите GitHub-токен")?;
    let key = read_signing_key().ok_or("Сначала импортируйте ключ подписи (экран «Инженеры»)")?;
    let client = reqwest::Client::new();
    let get = |accept: &'static str| {
        client
            .get(API_URL_FMT)
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
    let mut cur = parse_signed(&raw.text().await.map_err(|e| e.to_string())?)
        .ok_or("Список в репозитории повреждён или подписан другим ключом — правка отменена")?;

    edit(&mut cur.techs)?;
    cur.version += 1;
    cur.issued_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let payload = serde_json::to_string(&cur).map_err(|e| e.to_string())?;
    let sig_hex = sig::sign(&key, &payload)?;
    let mut body = serde_json::to_string_pretty(&sig::Signed { payload, sig: sig_hex }).map_err(|e| e.to_string())?;
    body.push('\n');
    let put = client
        .put(API_URL_FMT)
        .header("User-Agent", "echips-diagnostic-app")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "message": message,
            "content": b64_encode(body.as_bytes()),
            "sha": sha,
        }))
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    if !put.status().is_success() {
        return Err(gh_error(put.status()));
    }
    write_list_copies(&body);
    write_stamps(&read_stamps(), now_unix(), true);
    Ok(cur.techs)
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
