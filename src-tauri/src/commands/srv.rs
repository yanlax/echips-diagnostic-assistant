// Связь с сервером Echips (Yandex Cloud, см. server/DEPLOY.md): вход по PIN, инженеры, профили, отчёты.
// Сессия (токен на 12 ч) и PIN живут только в памяти процесса — токен и PIN на диск не пишутся. Когда сессия
// истекает или связи не было при входе, приложение тихо входит заново тем же PIN (без вопросов инженеру).
//
// Вход без интернета: при успешном онлайн-входе сервер выдаёт «аренду» на 7 суток — подписанный (ECDSA P-256,
// публичный ключ data/lease_pub.txt вшит в exe) файл с проверочным значением PIN (PBKDF2). Аренда хранится
// зашифрованной DPAPI (только этот ПК и пользователь Windows) в %LOCALAPPDATA%; офлайн приложение проверяет
// подпись, срок и PIN. Отчёты офлайн-сессии копятся в очереди и уходят, как только появляется связь.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use super::techs_sign as sig;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const API: &str = "https://d5d2gp51qbo80vhgv6sn.nkhmighe.apigw.yandexcloud.net/v1";

#[derive(Clone, Debug)]
pub struct Session {
    /// пусто в офлайн-сессии (вход по аренде без связи с сервером)
    pub token: String,
    pub exp: i64,
    pub id: String,
    pub name: String,
    pub role: String,
    pub offline: bool,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// PIN текущего инженера — только в памяти, для тихого повторного входа (истёк токен / появилась связь).
static PIN: Mutex<Option<String>> = Mutex::new(None);
static LAST_RETRY: Mutex<i64> = Mutex::new(0);

/// Публичный ключ подписи аренды (data/lease_pub.txt, hex, SEC1).
const LEASE_PUB: &str = include_str!("../../../data/lease_pub.txt");

/// Действующая сессия (не истёкшая; офлайн или онлайн) или None.
pub fn session() -> Option<Session> {
    let g = SESSION.lock().ok()?;
    g.clone().filter(|s| s.exp > chrono::Utc::now().timestamp())
}

/// Сессия с токеном сервера (не офлайн) или None.
fn online_session() -> Option<Session> {
    session().filter(|s| !s.offline && !s.token.is_empty())
}

fn set_session(s: Option<Session>) {
    if let Ok(mut g) = SESSION.lock() {
        *g = s;
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

/// Сообщение об ошибке из ответа сервера ({"error": "..."}) или общий текст по коду.
pub fn api_err(status: u16, text: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(e) = v["error"].as_str() {
            return e.to_string();
        }
    }
    match status {
        401 => "Сессия недействительна — войдите заново".to_string(),
        403 => "Недостаточно прав".to_string(),
        s => format!("Сервер вернул {s}"),
    }
}

/// Голый запрос к серверу (без сессии, если bearer не задан). Err — только сетевая ошибка; иначе (код, тело).
async fn send(method: &str, path: &str, body: Option<&Value>, bearer: Option<String>) -> Result<(u16, String), String> {
    let c = client()?;
    let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut req = c.request(m, format!("{API}{path}")).header("User-Agent", "echips-diagnostic-app");
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req.send().await.map_err(|e| format!("Нет связи с сервером: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok((status, text))
}

/// Запрос к серверу. auth=true добавляет токен сессии (при необходимости сначала тихо входит заново).
pub async fn request(method: &str, path: &str, body: Option<&Value>, auth: bool) -> Result<(u16, String), String> {
    if !auth {
        return send(method, path, body, None).await;
    }
    ensure_online().await?;
    match online_session() {
        Some(s) => send(method, path, body, Some(s.token)).await,
        None => Ok((401, json!({ "error": "Нет входа на сервере — войдите заново" }).to_string())),
    }
}

/// Идентификатор ноутбука для журнала сервера: MachineGuid Windows (в WinPE его может не быть — тогда имя ПК).
fn machine() -> &'static (String, String) {
    static M: OnceLock<(String, String)> = OnceLock::new();
    M.get_or_init(|| {
        let name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".to_string());
        #[cfg(target_os = "windows")]
        let guid = crate::powershell::run_ps("(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Cryptography' -ErrorAction SilentlyContinue).MachineGuid")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        #[cfg(not(target_os = "windows"))]
        let guid = String::new();
        (if guid.is_empty() { name.clone() } else { guid }, name)
    })
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct LoginOutcome {
    pub ok: bool,
    /// вход выполнен по аренде без связи с сервером (отчёты пойдут в очередь)
    pub offline_mode: bool,
    /// нет связи с сервером и подходящей аренды (а не «неверный PIN»)
    pub offline: bool,
    pub message: String,
    pub id: String,
    pub name: String,
    pub role: String,
    pub exp: i64,
}

fn outcome_of(s: &Session) -> LoginOutcome {
    LoginOutcome { ok: true, offline_mode: s.offline, offline: false, message: String::new(), id: s.id.clone(), name: s.name.clone(), role: s.role.clone(), exp: s.exp }
}

/// Вход на сервере; при отсутствии связи — по аренде. Сессия сохраняется в памяти.
async fn do_login(pin: &str) -> LoginOutcome {
    let (mid, mname) = machine().clone();
    let body = json!({ "pin": pin, "machine_id": mid, "machine_name": mname, "app_version": env!("CARGO_PKG_VERSION") });
    match send("POST", "/login", Some(&body), None).await {
        Err(e) => match offline_login(pin) {
            Some(s) => {
                let out = outcome_of(&s);
                set_session(Some(s));
                out
            }
            None => LoginOutcome { offline: true, message: format!("{e}. Аренды для входа без интернета нет или она истекла — подключитесь к интернету."), ..Default::default() },
        },
        Ok((200, text)) => {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let (Some(token), Some(exp)) = (v["token"].as_str(), v["exp"].as_i64()) else {
                return LoginOutcome { message: "Сервер вернул некорректный ответ".to_string(), ..Default::default() };
            };
            let u = &v["user"];
            let s = Session {
                token: token.to_string(),
                exp,
                id: u["id"].as_str().unwrap_or("").to_string(),
                name: u["name"].as_str().unwrap_or("").to_string(),
                role: u["role"].as_str().unwrap_or("tech").to_string(),
                offline: false,
            };
            if let Some(l) = v.get("lease").filter(|l| l.is_object()) {
                save_lease(&s.id, l);
            }
            let out = outcome_of(&s);
            set_session(Some(s));
            out
        }
        Ok((code, text)) => LoginOutcome { message: api_err(code, &text), ..Default::default() },
    }
}

#[tauri::command(async)]
pub async fn srv_login(pin: String) -> LoginOutcome {
    let out = do_login(&pin).await;
    if out.ok {
        if let Ok(mut g) = PIN.lock() {
            *g = Some(pin);
        }
    }
    out
}

/// Есть ли действующая сессия с токеном; если нет — тихая попытка войти заново тем же PIN (не чаще раза в 20 с).
async fn ensure_online() -> Result<(), String> {
    if online_session().is_some() {
        return Ok(());
    }
    let pin = PIN.lock().ok().and_then(|g| g.clone());
    let Some(pin) = pin else { return Ok(()) };
    let now = chrono::Utc::now().timestamp();
    {
        let mut last = LAST_RETRY.lock().map_err(|e| e.to_string())?;
        if now - *last < 20 {
            return Ok(());
        }
        *last = now;
    }
    let (mid, mname) = machine().clone();
    let body = json!({ "pin": pin, "machine_id": mid, "machine_name": mname, "app_version": env!("CARGO_PKG_VERSION") });
    // прямой запрос без рекурсии в ensure_online
    match send("POST", "/login", Some(&body), None).await {
        Ok((200, text)) => {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if let (Some(token), Some(exp)) = (v["token"].as_str(), v["exp"].as_i64()) {
                let u = &v["user"];
                let s = Session {
                    token: token.to_string(),
                    exp,
                    id: u["id"].as_str().unwrap_or("").to_string(),
                    name: u["name"].as_str().unwrap_or("").to_string(),
                    role: u["role"].as_str().unwrap_or("tech").to_string(),
                    offline: false,
                };
                if let Some(l) = v.get("lease").filter(|l| l.is_object()) {
                    save_lease(&s.id, l);
                }
                set_session(Some(s));
            }
            Ok(())
        }
        Ok((401, _)) => {
            // PIN больше не подходит (сменили или удалили инженера) — забываем его, потребуется новый вход
            if let Ok(mut g) = PIN.lock() {
                *g = None;
            }
            set_session(None);
            Err("Сессия недействительна — войдите заново".to_string())
        }
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

#[tauri::command(async)]
pub async fn srv_ping() -> bool {
    matches!(request("GET", "/ping", None, false).await, Ok((200, _)))
}

#[tauri::command]
pub fn srv_logout() {
    set_session(None);
    if let Ok(mut g) = PIN.lock() {
        *g = None;
    }
}

// ---------- инженеры (только админ) ----------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub active: bool,
}

fn users_result(r: Result<(u16, String), String>) -> Result<Vec<UserInfo>, String> {
    match r? {
        (200, text) => serde_json::from_str(&text).map_err(|e| format!("Некорректный ответ сервера: {e}")),
        (code, text) => Err(api_err(code, &text)),
    }
}

#[tauri::command(async)]
pub async fn techs_list() -> Result<Vec<UserInfo>, String> {
    users_result(request("GET", "/users", None, true).await)
}

/// Добавляет инженера или (если id уже есть) меняет имя, роль и PIN. PIN уходит на сервер по HTTPS и хэшируется там.
#[tauri::command(async)]
pub async fn techs_upsert(id: String, name: String, pin: String, role: String) -> Result<Vec<UserInfo>, String> {
    let body = json!({ "id": id, "name": name, "pin": pin, "role": role });
    users_result(request("POST", "/users", Some(&body), true).await)
}

#[tauri::command(async)]
pub async fn techs_remove(id: String) -> Result<Vec<UserInfo>, String> {
    users_result(request("DELETE", &format!("/users?id={}", urlencoding::encode(&id)), None, true).await)
}

/// Журнал событий за день (только админ): [{t, type, user, ip, data}].
#[tauri::command(async)]
pub async fn srv_events(date: String) -> Result<Value, String> {
    match request("GET", &format!("/events?date={}", urlencoding::encode(&date)), None, true).await? {
        (200, text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
        (code, text) => Err(api_err(code, &text)),
    }
}

/// Событие в журнал сервера (начало диагностики и т. п.) — молча игнорируется без связи или сессии.
#[tauri::command(async)]
pub async fn srv_event(kind: String, data: Value) -> Result<(), String> {
    let body = json!({ "type": kind, "data": data });
    let _ = request("POST", "/event", Some(&body), true).await;
    Ok(())
}

/// Кто сейчас вошёл (имя, роль, офлайн-режим). При истёкшей сессии сначала тихо входит заново тем же PIN.
#[tauri::command(async)]
pub async fn srv_whoami() -> Value {
    if online_session().is_none() {
        let _ = ensure_online().await;
        if session().is_none() {
            // токен истёк, а связи нет — пробуем аренду
            if let Some(pin) = PIN.lock().ok().and_then(|g| g.clone()) {
                if let Some(s) = offline_login(&pin) {
                    set_session(Some(s));
                }
            }
        }
    }
    match session() {
        Some(s) => json!({ "id": s.id, "name": s.name, "role": s.role, "exp": s.exp, "offline": s.offline }),
        None => Value::Null,
    }
}

// ---------- аренда для входа без интернета ----------

fn leases_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck").join("leases")
}

fn safe_name(id: &str) -> String {
    id.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// DPAPI (CurrentUser): расшифровать файл может только тот же пользователь Windows на этом же ПК.
#[cfg(target_os = "windows")]
fn dpapi(protect: bool, input_b64: &str) -> Result<String, String> {
    let method = if protect { "Protect" } else { "Unprotect" };
    let script = format!(
        "Add-Type -AssemblyName System.Security; \
         $b = [Convert]::FromBase64String('{input_b64}'); \
         [Convert]::ToBase64String([System.Security.Cryptography.ProtectedData]::{method}($b, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser))"
    );
    crate::powershell::run_ps(&script).map(|o| o.trim().to_string())
}

#[cfg(not(target_os = "windows"))]
fn dpapi(_protect: bool, input_b64: &str) -> Result<String, String> {
    // вне Windows (тесты, разработка) — без шифрования
    Ok(input_b64.to_string())
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

fn save_lease(id: &str, lease: &Value) {
    let Ok(text) = serde_json::to_string(lease) else { return };
    let Ok(cipher) = dpapi(true, &b64_encode(text.as_bytes())) else { return };
    let dir = leases_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{}.dat", safe_name(id))), cipher);
}

fn read_leases() -> Vec<Value> {
    let Ok(rd) = std::fs::read_dir(leases_dir()) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "dat"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|c| dpapi(false, c.trim()).ok())
        .filter_map(|p| b64_decode(&p))
        .filter_map(|b| String::from_utf8(b).ok())
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .collect()
}

/// PBKDF2-HMAC-SHA256 (dk 32 байта) — на sha2, без лишних зависимостей. Совпадает с hashlib.pbkdf2_hmac на сервере.
pub(crate) fn pbkdf2_sha256(password: &[u8], salt: &[u8], iters: u32) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    fn hmac(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
        let mut k = [0u8; 64];
        if key.len() > 64 {
            k[..32].copy_from_slice(&Sha256::digest(key));
        } else {
            k[..key.len()].copy_from_slice(key);
        }
        let mut inner = Sha256::new();
        inner.update(k.iter().map(|b| b ^ 0x36).collect::<Vec<u8>>());
        for d in data {
            inner.update(d);
        }
        let mut outer = Sha256::new();
        outer.update(k.iter().map(|b| b ^ 0x5c).collect::<Vec<u8>>());
        outer.update(inner.finalize());
        outer.finalize().into()
    }
    let mut u = hmac(password, &[salt, &1u32.to_be_bytes()]);
    let mut t = u;
    for _ in 1..iters {
        u = hmac(password, &[&u]);
        for i in 0..32 {
            t[i] ^= u[i];
        }
    }
    t
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Проверка одной аренды: подпись сервера, срок, PIN. Возвращает офлайн-сессию.
pub(crate) fn check_lease(lease: &Value, pin: &str, now: i64) -> Option<Session> {
    let payload = lease["payload"].as_str()?;
    if !sig::verify(LEASE_PUB.trim(), payload, lease["sig"].as_str()?) {
        return None;
    }
    let p: Value = serde_json::from_str(payload).ok()?;
    let (iat, exp) = (p["iat"].as_i64()?, p["exp"].as_i64()?);
    // просрочена, либо выдана «в будущем» (часы сдвинуты назад) — не принимаем
    if exp <= now || iat > now + 3600 {
        return None;
    }
    let iters = p["iters"].as_u64()? as u32;
    let salt = sig::hex_decode(p["salt"].as_str()?)?;
    if !(1_000..=2_000_000).contains(&iters) {
        return None;
    }
    if hex_of(&pbkdf2_sha256(pin.as_bytes(), &salt, iters)) != p["verifier"].as_str()? {
        return None;
    }
    Some(Session {
        token: String::new(),
        exp,
        id: p["sub"].as_str()?.to_string(),
        name: p["name"].as_str()?.to_string(),
        role: p["role"].as_str().unwrap_or("tech").to_string(),
        offline: true,
    })
}

fn offline_login(pin: &str) -> Option<Session> {
    let now = chrono::Utc::now().timestamp();
    read_leases().iter().find_map(|l| check_lease(l, pin, now))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_matches_python() {
        // python3: hashlib.pbkdf2_hmac('sha256', b'1234', bytes.fromhex('00112233445566778899aabbccddeeff'), 1000).hex()
        let got = hex_of(&pbkdf2_sha256(b"1234", &sig::hex_decode("00112233445566778899aabbccddeeff").unwrap(), 1000));
        assert_eq!(got, "b8ad0f1dacec0beb4f418fadcb2ee5939ef45823391bb89692028dd1b67e9cef");
    }

    #[test]
    fn b64_roundtrip() {
        for n in 0..20u8 {
            let data: Vec<u8> = (0..n).collect();
            assert_eq!(b64_decode(&b64_encode(&data)).unwrap(), data);
        }
    }
}
