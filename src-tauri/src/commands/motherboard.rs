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

/// Запись SN/UUID платы — по заводской процедуре (FlashSerialNumber.cmd из
/// комплекта тестовых утилит завода, присланного сервисом): тип BIOS
/// определяется по Win32_BIOS; AMI — Amidewin.exe (`/BS <SN>`, `/SS <SN>`,
/// успех — "Done" в выводе), Insyde — H2OSDE-Wx64.exe (`-W -BS <SN>`,
/// `-W -SS <SN>`, успех — "OK"). После записи серийник читается обратно теми
/// же утилитами и сверяется (как в заводском скрипте). UUID: Insyde `-SU <uuid>`
/// (есть в ReadMe утилиты), AMI `/SU <uuid>` — штатный ключ AMIDEWIN,
/// подтверждается чтением обратно; в самом заводском скрипте UUID для AMI нет,
/// поэтому его стоит проверить на реальной плате. Утилиты вшиты в exe
/// (src-tauri/assets/smbios, распространение разрешено заводом-разработчиком)
/// и при первой записи распаковываются в %LOCALAPPDATA%\Echips\HardwareCheck\smbios
/// (драйверу amifldrv64.sys нужно лежать рядом с Amidewin.exe).
/// Любой исход (успех/сбой) пишется в аудит-лог.
#[cfg(target_os = "windows")]
mod flash {
    use super::app_data_dir;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const AMIDEWIN: &[u8] = include_bytes!("../../assets/smbios/Amidewin.exe");
    const H2OSDE: &[u8] = include_bytes!("../../assets/smbios/H2OSDE-Wx64.exe");
    const AMIFLDRV64: &[u8] = include_bytes!("../../assets/smbios/amifldrv64.sys");
    const AMIFLDRV32: &[u8] = include_bytes!("../../assets/smbios/amifldrv32.sys");

    #[derive(Clone, Copy, PartialEq)]
    enum Vendor {
        Insyde,
        Ami,
    }

    pub struct Tool {
        dir: PathBuf,
        vendor: Vendor,
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

    impl Tool {
        pub fn new() -> Result<Tool, String> {
            let dir = app_data_dir().join("smbios");
            std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку утилит: {e}"))?;
            let files: [(&str, &[u8]); 4] = [
                ("Amidewin.exe", AMIDEWIN),
                ("H2OSDE-Wx64.exe", H2OSDE),
                ("amifldrv64.sys", AMIFLDRV64),
                ("amifldrv32.sys", AMIFLDRV32),
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
            Ok(Tool { dir, vendor })
        }

        fn exe(&self) -> &'static str {
            match self.vendor {
                Vendor::Insyde => "H2OSDE-Wx64.exe",
                Vendor::Ami => "Amidewin.exe",
            }
        }

        fn flag(&self, field: &str) -> String {
            match self.vendor {
                Vendor::Insyde => format!("-{field}"),
                Vendor::Ami => format!("/{field}"),
            }
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
                Err(format!("Утилита не подтвердила запись серийного номера ({field}): {}", out.trim()))
            }
        }

        pub fn read_serial(&self, field: &str) -> Result<String, String> {
            let flag = self.flag(field);
            match self.vendor {
                Vendor::Insyde => run(&self.dir, self.exe(), &["-R", flag.as_str()]),
                Vendor::Ami => run(&self.dir, self.exe(), &[flag.as_str()]),
            }
        }

        pub fn write_uuid(&self, uuid: &str) -> Result<(), String> {
            let flag = self.flag("SU");
            run(&self.dir, self.exe(), &[flag.as_str(), uuid]).map(|_| ())
        }

        pub fn read_uuid(&self) -> Result<String, String> {
            let flag = self.flag("SU");
            run(&self.dir, self.exe(), &[flag.as_str()])
        }
    }
}

#[cfg(target_os = "windows")]
fn do_flash(new_serial: &str, new_uuid: &str) -> Result<(), String> {
    let tool = flash::Tool::new()?;
    tool.write_serial("BS", new_serial)?;
    tool.write_serial("SS", new_serial)?;
    for field in ["BS", "SS"] {
        if !tool.read_serial(field)?.contains(new_serial) {
            return Err(format!("Проверка после записи не прошла: серийник ({field}) не совпал."));
        }
    }
    tool.write_uuid(new_uuid)?;
    if !tool.read_uuid()?.to_lowercase().contains(&new_uuid.to_lowercase()) {
        return Err("Серийные номера записаны, но UUID после записи не совпал (проверьте команду UUID для этого BIOS).".to_string());
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
    if !is_valid_serial(&new_serial) {
        return Err("Серийный номер должен быть 8–20 латинскими буквами/цифрами.".to_string());
    }
    if !is_valid_uuid(&new_uuid) {
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
    let entry = AuditEntry {
        timestamp: chrono::Local::now().to_rfc3339(),
        technician,
        ticket,
        before_serial,
        before_uuid,
        after_serial: new_serial,
        after_uuid: new_uuid,
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
