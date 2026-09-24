// Замена материнской платы (гарантийный случай): чтение текущих SN/UUID,
// хэш-цепочка аудит-лога, запись новых значений.
//
// Запись SN/UUID (write_smbios_identity) — по заводской процедуре, см.
// комментарий у модуля flash ниже. Аудит-лог пишется при любом исходе —
// успешном или нет, чтобы ни одна попытка не прошла бесследно.

use crate::powershell::run_ps;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use tauri::Manager;

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
    pub status: String, // "written" | "failed: <причина>" | (старые записи) "stub_not_written"
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
    // Реальные серийники бывают длинными (26+ символов). Не с дефиса — иначе
    // утилита примет значение за свой ключ; значения передаются аргументом
    // процесса без оболочки, инъекции через них невозможны.
    let mut chars = v.chars();
    (4..=40).contains(&v.len())
        && chars.next().map_or(false, |c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn is_valid_uuid(v: &str) -> bool {
    let parts: Vec<&str> = v.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts.iter().zip(expected_lens).all(|(p, len)| p.len() == len && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Запись SN/UUID платы — по заводской процедуре (FlashSerialNumber.cmd из
/// комплекта тестовых утилит завода, присланного сервисом): тип BIOS
/// определяется по Win32_BIOS; AMI — AMIDEWINx64.exe (`/BS <SN>`, `/SS <SN>`,
/// успех — "Done" в выводе), Insyde — H2OSDE-Wx64.exe (`-W -BS <SN>`,
/// `-W -SS <SN>`, успех — "OK"). После записи серийник читается обратно теми
/// же утилитами и сверяется (как в заводском скрипте). UUID: Insyde `-SU <uuid>`
/// (есть в ReadMe утилиты), AMI `/SU <uuid>` — штатный ключ AMIDEWIN,
/// подтверждается чтением обратно; в самом заводском скрипте UUID для AMI нет,
/// поэтому его стоит проверить на реальной плате. Утилиты вшиты в exe
/// (src-tauri/assets/smbios, распространение разрешено заводом-разработчиком)
/// и при первой записи распаковываются в %LOCALAPPDATA%\Echips\HardwareCheck\smbios
/// (драйверы amifldrv64.sys/amigendrv64.sys лежат рядом с AMIDEWINx64.exe; с v0.38.0 —
/// новая 64-битная AMIDEWIN 2020 г. от завода для Aptio V вместо 32-битной 2014 г.).
/// Любой исход (успех/сбой) пишется в аудит-лог.
#[cfg(target_os = "windows")]
mod flash {
    use super::app_data_dir;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const AMIDEWIN: &[u8] = include_bytes!("../../assets/smbios/AMIDEWINx64.exe");
    const H2OSDE: &[u8] = include_bytes!("../../assets/smbios/H2OSDE-Wx64.exe");
    const AMIFLDRV64: &[u8] = include_bytes!("../../assets/smbios/amifldrv64.sys");
    const AMIGENDRV64: &[u8] = include_bytes!("../../assets/smbios/amigendrv64.sys");

    #[derive(Clone, Copy, PartialEq)]
    enum Vendor {
        Insyde,
        Ami,
    }

    pub struct Tool {
        dir: PathBuf,
        vendor: Vendor,
        bios_info: String,
    }

    fn run(dir: &Path, exe: &str, args: &[&str]) -> Result<String, String> {
        let out = Command::new(dir.join(exe))
            .args(args)
            .current_dir(dir)
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| format!("Не удалось запустить {exe}: {e}"))?;
        Ok(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }

    /// Запуск с таймаутом и кодом возврата: DMIEDITx64.EXE — GUI-приложение, вывода в консоль у него может
    /// не быть, поэтому результат смотрим по коду возврата (ErrCode.txt из комплекта завода) и чтением обратно.
    fn run_status(dir: &Path, exe: &str, args: &[&str], secs: u64) -> Result<(Option<i32>, String), String> {
        let mut child = Command::new(dir.join(exe))
            .args(args)
            .current_dir(dir)
            .creation_flags(0x08000000)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Не удалось запустить {exe}: {e}"))?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    let out = child.wait_with_output().map_err(|e| e.to_string())?;
                    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    return Ok((out.status.code(), text));
                }
                Ok(None) => {
                    if started.elapsed() > Duration::from_secs(secs) {
                        let _ = child.kill();
                        return Err(format!("{exe} не завершилась за {secs} с"));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn dmiedit_code(code: Option<i32>) -> String {
        match code {
            Some(0) => "0 (успех)".into(),
            Some(0x10) => "0x10 (не загрузился драйвер)".into(),
            Some(0x49) => "0x49 (платформа не позволяет)".into(),
            Some(0x61) => "0x61 (программа уже запущена)".into(),
            Some(0xE0) => "0xE0 (не удалось инициализировать SMBIOS)".into(),
            Some(0xE1) => "0xE1 (не прочитаны данные DMI)".into(),
            Some(0xE2) => "0xE2 (запись DMI не удалась)".into(),
            Some(0xE3) => "0xE3 (система не поддерживает)".into(),
            Some(c) => format!("{c:#X}"),
            None => "нет кода".into(),
        }
    }

    impl Tool {
        pub fn new() -> Result<Tool, String> {
            let dir = app_data_dir().join("smbios");
            std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку утилит: {e}"))?;
            let files: [(&str, &[u8]); 4] = [
                ("AMIDEWINx64.exe", AMIDEWIN),
                ("H2OSDE-Wx64.exe", H2OSDE),
                ("amifldrv64.sys", AMIFLDRV64),
                ("amigendrv64.sys", AMIGENDRV64),
            ];
            for (name, bytes) in files {
                let path = dir.join(name);
                let same = std::fs::metadata(&path).map(|m| m.len() == bytes.len() as u64).unwrap_or(false);
                if !same {
                    std::fs::write(&path, bytes).map_err(|e| format!("Не удалось распаковать {name}: {e}"))?;
                }
            }
            let info = crate::powershell::run_ps(
                r#"Get-CimInstance Win32_BIOS | ForEach-Object { "$($_.Manufacturer) $($_.Name) $($_.Version) $($_.SMBIOSBIOSVersion)" }"#,
            )?;
            let low = info.to_lowercase();
            let vendor = if low.contains("insyde") {
                Vendor::Insyde
            } else if low.contains("american") || low.contains("alaska") {
                Vendor::Ami
            } else {
                return Err(format!(
                    "Тип BIOS не поддерживается (заводская процедура — только Insyde и AMI): {}.",
                    info.trim()
                ));
            };
            Ok(Tool { dir, vendor, bios_info: info.trim().to_string() })
        }

        fn exe(&self) -> &'static str {
            match self.vendor {
                Vendor::Insyde => "H2OSDE-Wx64.exe",
                Vendor::Ami => "AMIDEWINx64.exe",
            }
        }

        fn flag(&self, field: &str) -> String {
            match self.vendor {
                Vendor::Insyde => format!("-{field}"),
                Vendor::Ami => format!("/{field}"),
            }
        }

        /// Проверка ДО записи: читаем серийник плата тем же инструментом. Если утилита сообщает, что
        /// система не поддерживается (напр. AMIDEWIN 2014 г. на BIOS 2025 г.: «d7 - Error: System
        /// doesn't support»), писать бессмысленно — возвращаем понятную причину, а не сырой вывод.
        pub fn check_supported(&self) -> Result<(), String> {
            let out = self.read_serial("BS")?;
            let low = out.to_lowercase();
            if low.contains("doesn't support") || low.contains("not support") {
                let tool = match self.vendor {
                    Vendor::Insyde => "H2OSDE (Insyde)",
                    Vendor::Ami => "AMIDEWIN (AMI Aptio V, 2020 г.)",
                };
                let reason = out
                    .lines()
                    .map(|l| l.trim())
                    .find(|l| l.to_lowercase().contains("error"))
                    .unwrap_or("система не поддерживается")
                    .to_string();
                return Err(format!(
                    "Заводская утилита {tool} не поддерживает BIOS этого устройства ({reason}). \
                     Нужна более новая версия утилиты — запросите её у разработчиков завода. BIOS: {}.",
                    self.bios_info
                ));
            }
            Ok(())
        }

        /// field: "BS" (плата) или "SS" (система)
        pub fn write_serial(&self, field: &str, sn: &str) -> Result<(), String> {
            let flag = self.flag(field);
            let (out, done) = match self.vendor {
                Vendor::Insyde => (run(&self.dir, self.exe(), &["-W", flag.as_str(), sn])?, "OK"),
                Vendor::Ami => (run(&self.dir, self.exe(), &[flag.as_str(), sn])?, "Done"),
            };
            if out.contains(done) {
                Ok(())
            } else {
                // без баннера утилиты (рамка с копирайтом) — только строки с ошибкой, иначе весь вывод
                let errs: Vec<&str> = out.lines().map(|l| l.trim()).filter(|l| l.to_lowercase().contains("error")).collect();
                let detail = if errs.is_empty() { out.trim().to_string() } else { errs.join("; ") };
                Err(format!("Утилита не подтвердила запись серийного номера ({field}): {detail}"))
            }
        }

        pub fn read_serial(&self, field: &str) -> Result<String, String> {
            let flag = self.flag(field);
            match self.vendor {
                Vendor::Insyde => run(&self.dir, self.exe(), &["-R", flag.as_str()]),
                Vendor::Ami => run(&self.dir, self.exe(), &[flag.as_str()]),
            }
        }

        /// Запись UUID с проверкой чтением обратно. AMI: справка AMIDEWIN/DMIEDIT — «/SU [16 Bytes]»,
        /// «Error: The size of UUID is too long»: UUID передаётся как 32 hex-символа БЕЗ дефисов (так же
        /// делает заводской test.bat), с дефисами (36 символов) утилита отказывает. DMIEDITx64.EXE при
        /// запуске с аргументами просто открывает своё окно — из программы его не используем.
        /// Insyde — как раньше (`-SU`).
        pub fn write_uuid_verified(&self, uuid: &str) -> Result<(), String> {
            let norm = |t: &str| t.to_lowercase().replace('-', "");
            let want = norm(uuid);
            let flag = self.flag("SU");
            match self.vendor {
                Vendor::Insyde => {
                    let wrote = run(&self.dir, self.exe(), &[flag.as_str(), uuid])?;
                    let read = run(&self.dir, self.exe(), &[flag.as_str()])?;
                    if norm(&read).contains(&want) {
                        Ok(())
                    } else {
                        Err(format!(
                            "UUID после записи не совпал. Запись: «{}». Прочитано: «{}».",
                            super::brief(&wrote),
                            super::brief(&read)
                        ))
                    }
                }
                Vendor::Ami => {
                    let plain = uuid.replace('-', "");
                    let attempts: [(&str, &str); 1] = [("AMIDEWINx64.exe", plain.as_str())];
                    let mut log: Vec<String> = Vec::new();
                    for (exe, val) in attempts {
                        match run_status(&self.dir, exe, &[flag.as_str(), val], 40) {
                            Ok((code, out)) => log.push(format!("{exe} {flag} {val}: код {}, «{}»", dmiedit_code(code), super::brief(&out))),
                            Err(e) => {
                                log.push(format!("{exe}: {e}"));
                                continue;
                            }
                        }
                        let read = run(&self.dir, "AMIDEWINx64.exe", &[flag.as_str()])?;
                        if norm(&read).contains(&want) {
                            return Ok(());
                        }
                    }
                    Err(format!("UUID не удалось записать: {}", log.join(" | ")))
                }
            }
        }
    }
}

/// Без рамки-баннера утилиты: только полезные строки вывода (для сообщения об ошибке).
#[cfg(target_os = "windows")]
fn brief(out: &str) -> String {
    let lines: Vec<&str> = out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('|') && !l.starts_with('+') && !l.to_lowercase().contains("copyright"))
        .collect();
    let joined = lines.join(" · ");
    joined.chars().take(300).collect()
}

#[cfg(target_os = "windows")]
fn do_flash(new_serial: &str, new_uuid: &str) -> Result<(), String> {
    // пустое значение = это поле не меняем (SN и UUID можно писать раздельно)
    let tool = flash::Tool::new()?;
    tool.check_supported()?;
    if !new_serial.is_empty() {
        tool.write_serial("BS", new_serial)?;
        tool.write_serial("SS", new_serial)?;
        for field in ["BS", "SS"] {
            if !tool.read_serial(field)?.contains(new_serial) {
                return Err(format!("Проверка после записи не прошла: серийник ({field}) не совпал."));
            }
        }
    }
    if !new_uuid.is_empty() {
        tool.write_uuid_verified(new_uuid).map_err(|e| {
            if new_serial.is_empty() { e } else { format!("Серийные номера записаны, но {e}") }
        })?;
    }
    Ok(())
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
    let (new_serial, new_uuid) = (new_serial.trim().to_string(), new_uuid.trim().to_string());
    if new_serial.is_empty() && new_uuid.is_empty() {
        return Err("Укажите новый серийный номер и/или UUID.".to_string());
    }
    if !new_serial.is_empty() && !is_valid_serial(&new_serial) {
        return Err("Серийный номер: 4–40 символов — латинские буквы, цифры, . _ - (не с дефиса).".to_string());
    }
    if !new_uuid.is_empty() && !is_valid_uuid(&new_uuid) {
        return Err("UUID должен быть в формате 8-4-4-4-12.".to_string());
    }

    #[cfg(target_os = "windows")]
    let outcome = do_flash(&new_serial, &new_uuid);
    #[cfg(not(target_os = "windows"))]
    let outcome: Result<(), String> = Err("Запись SN/UUID доступна только в Windows-сборке.".to_string());

    let status = match &outcome {
        Ok(()) => "written".to_string(),
        Err(e) => format!("failed: {}", e.chars().take(120).collect::<String>()),
    };
    // при раздельной записи не тронутое поле остаётся равным значению «до»
    let after_serial = if new_serial.is_empty() { before_serial.clone() } else { new_serial };
    let after_uuid = if new_uuid.is_empty() { before_uuid.clone() } else { new_uuid };
    let entry = AuditEntry {
        timestamp: chrono::Local::now().to_rfc3339(),
        technician,
        ticket,
        before_serial,
        before_uuid,
        after_serial,
        after_uuid,
        status,
        prev_hash: String::new(),
        hash: String::new(),
    };
    let saved = append_audit_entry(entry)?;

    match outcome {
        Ok(()) => Ok(saved),
        Err(e) => Err(format!("{e} Попытка записана в аудит-лог (хэш {}).", &saved.hash[..12])),
    }
}

/// Последняя запись SN/UUID хранится на диске: после перезагрузки тест «Идентификаторы» сверяет систему с ней.
/// `boot_time` — время последней загрузки Windows: если запись сделана ПОСЛЕ неё, WMI ещё показывает старые
/// значения SMBIOS и расхождение — не ошибка, а «нужна перезагрузка».
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct MbLast {
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub uuid: String,
    /// RFC3339, когда записано
    #[serde(default)]
    pub written_at: String,
    /// RFC3339, последняя загрузка Windows (заполняется при чтении)
    #[serde(default)]
    pub boot_time: String,
}

fn mb_last_path() -> std::path::PathBuf {
    app_data_dir().join("mb_last.json")
}

#[tauri::command(async)]
pub fn mb_last_save(serial: String, uuid: String) -> Result<(), String> {
    // раздельная запись: пустое поле не затирает ранее записанное значение
    let mut cur = std::fs::read_to_string(mb_last_path()).ok().and_then(|t| serde_json::from_str::<MbLast>(&t).ok()).unwrap_or_default();
    if !serial.trim().is_empty() {
        cur.serial = serial.trim().to_string();
    }
    if !uuid.trim().is_empty() {
        cur.uuid = uuid.trim().to_string();
    }
    cur.written_at = chrono::Local::now().to_rfc3339();
    let _ = std::fs::create_dir_all(app_data_dir());
    let text = serde_json::to_string_pretty(&cur).map_err(|e| e.to_string())?;
    std::fs::write(mb_last_path(), text).map_err(|e| format!("Не удалось сохранить: {e}"))
}

#[tauri::command(async)]
pub fn mb_last_load() -> Option<MbLast> {
    let mut m = std::fs::read_to_string(mb_last_path()).ok().and_then(|t| serde_json::from_str::<MbLast>(&t).ok())?;
    #[cfg(target_os = "windows")]
    {
        if let Ok(t) = crate::powershell::run_ps("(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToString('o')") {
            m.boot_time = t.trim().to_string();
        }
    }
    Some(m)
}

/// Сохранённые значения SN/UUID платы: перед заменой инженер сохраняет их в файл, после замены подставляет обратно
/// из файла (вкладка «Замена платы»). Файлы лежат в `<папка данных приложения>\board_identity`.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SavedIdentity {
    #[serde(default)]
    pub saved_at: String,
    /// серийный номер (SMBIOS System/Baseboard)
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub bios_version: String,
    #[serde(default)]
    pub engineer: String,
    #[serde(default)]
    pub ticket: String,
    /// путь к файлу (заполняется при чтении списка / после сохранения)
    #[serde(default)]
    pub path: String,
}

fn identity_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Не удалось определить папку данных приложения: {e}"))?.join("board_identity");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку: {e}"))?;
    Ok(dir)
}

/// Сохраняет значения в JSON-файл `<серийник>_<дата_время>.json`; возвращает полный путь (для открытия папки).
#[tauri::command(async)]
pub fn mb_save_identity(app: tauri::AppHandle, data: SavedIdentity) -> Result<String, String> {
    if data.serial.trim().is_empty() && data.uuid.trim().is_empty() {
        return Err("Нечего сохранять: серийный номер и UUID пусты".to_string());
    }
    let dir = identity_dir(&app)?;
    let safe: String = data.serial.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).take(40).collect();
    let name = format!("{}_{}.json", if safe.is_empty() { "board".to_string() } else { safe }, chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let path = dir.join(name);
    let mut d = data;
    if d.saved_at.is_empty() {
        d.saved_at = chrono::Local::now().to_rfc3339();
    }
    d.path = String::new();
    let text = serde_json::to_string_pretty(&d).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Не удалось записать файл: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// Список сохранённых файлов, новые сверху (до 30).
#[tauri::command(async)]
pub fn mb_list_identities(app: tauri::AppHandle) -> Result<Vec<SavedIdentity>, String> {
    let dir = identity_dir(&app)?;
    let mut out: Vec<SavedIdentity> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let mut v: SavedIdentity = serde_json::from_str(&text).ok()?;
            v.path = e.path().to_string_lossy().to_string();
            Some(v)
        })
        .collect();
    out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    out.truncate(30);
    Ok(out)
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
