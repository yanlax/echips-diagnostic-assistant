// Замена материнской платы (гарантийный случай): чтение текущих SN/UUID,
// хэш-цепочка аудит-лога, запись новых значений.
//
// ВАЖНО про write_smbios_identity: я не стал подставлять флаги
// AMIDEWINx64.exe "как есть" — в открытом доступе они встречаются в основном
// на форумах про обход HWID-банов в играх, а не в официальной документации
// AMI, и для операции, которая необратимо переписывает SMBIOS на реальном
// железе, это ненадёжный источник. Функция ниже читает актуальные значения
// (это безопасно, только чтение) и валидирует форму нового SN/UUID, но сама
// команда записи — заглушка с понятной ошибкой: подставьте точный путь к
// AMIDEWINx64.exe и его аргументы из вашей текущей проверенной процедуры
// (раз она уже используется в сервисе), прежде чем включать кнопку в проде.
// Аудит-лог при этом всё равно пишется — с пометкой "заглушка" в статусе —
// чтобы ни одна попытка не прошла бесследно.

use crate::powershell::run_ps;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BoardIdentity {
    pub serial_number: String,
    pub uuid: String,
}

#[tauri::command(async)]
pub fn read_board_identity() -> Result<BoardIdentity, String> {
    #[cfg(target_os = "windows")]
    {
        let serial = run_ps("(Get-CimInstance Win32_BIOS).SerialNumber")?.trim().to_string();
        let uuid = run_ps("(Get-CimInstance Win32_ComputerSystemProduct).UUID")?.trim().to_string();
        Ok(BoardIdentity { serial_number: serial, uuid })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuditEntry {
    pub timestamp: String,
    pub technician: String,
    pub ticket: String,
    pub before_serial: String,
    pub before_uuid: String,
    pub after_serial: String,
    pub after_uuid: String,
    pub status: String, // "written" | "stub_not_written"
    pub prev_hash: String,
    pub hash: String,
}

fn app_data_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck")
}

fn audit_log_path() -> std::path::PathBuf {
    app_data_dir().join("motherboard_audit.log")
}

fn last_hash() -> String {
    let path = audit_log_path();
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return "genesis".to_string(),
    };
    let reader = BufReader::new(file);
    let mut last_line: Option<String> = None;
    for line in reader.lines().map_while(Result::ok) {
        if !line.trim().is_empty() {
            last_line = Some(line);
        }
    }
    match last_line {
        Some(line) => serde_json::from_str::<AuditEntry>(&line).map(|e| e.hash).unwrap_or_else(|_| "genesis".into()),
        None => "genesis".to_string(),
    }
}

fn append_audit_entry(mut entry: AuditEntry) -> Result<AuditEntry, String> {
    entry.prev_hash = last_hash();
    let hash_input = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        entry.prev_hash,
        entry.timestamp,
        entry.technician,
        entry.ticket,
        entry.before_serial,
        entry.before_uuid,
        entry.after_serial,
        entry.after_uuid,
        entry.status
    );
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    entry.hash = format!("{:x}", hasher.finalize());

    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())?;

    Ok(entry)
}

fn is_valid_serial(v: &str) -> bool {
    (8..=20).contains(&v.len()) && v.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_valid_uuid(v: &str) -> bool {
    let parts: Vec<&str> = v.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts.iter().zip(expected_lens).all(|(p, len)| p.len() == len && p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[tauri::command(async)]
pub fn write_smbios_identity(
    technician: String,
    ticket: String,
    before_serial: String,
    before_uuid: String,
    new_serial: String,
    new_uuid: String,
) -> Result<AuditEntry, String> {
    if ticket.trim().is_empty() {
        return Err("Не указан номер наряда.".to_string());
    }
    if !is_valid_serial(&new_serial) {
        return Err("Серийный номер должен быть 8–20 латинскими буквами/цифрами.".to_string());
    }
    if !is_valid_uuid(&new_uuid) {
        return Err("UUID должен быть в формате 8-4-4-4-12.".to_string());
    }

    let timestamp = chrono::Local::now().to_rfc3339();

    // Заглушка: реальная запись через AMIDEWINx64.exe (или другой штатный
    // инструмент) должна быть подставлена здесь — команда ниже НЕ выполняет
    // запись, только фиксирует попытку в аудит-логе, чтобы её можно было
    // включить, доверив точный вызов человеку, который уже это делал.
    let entry = AuditEntry {
        timestamp,
        technician,
        ticket,
        before_serial,
        before_uuid,
        after_serial: new_serial,
        after_uuid: new_uuid,
        status: "stub_not_written".to_string(),
        prev_hash: String::new(),
        hash: String::new(),
    };
    let saved = append_audit_entry(entry)?;

    Err(format!(
        "Запись SN/UUID не выполнена: команда AMIDEWINx64.exe не сконфигурирована в этой сборке \
        (см. комментарий в src-tauri/src/commands/motherboard.rs). Попытка зафиксирована в \
        аудит-логе (хэш {}), физической записи на плату не было.",
        &saved.hash[..12]
    ))
}

#[tauri::command(async)]
pub fn read_audit_log() -> Result<Vec<AuditEntry>, String> {
    let path = audit_log_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(vec![]),
    };
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .collect())
}
