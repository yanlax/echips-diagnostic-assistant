// Журнал сбоев (аналог BlueScreenView): синие экраны и внезапные перезагрузки.
// Источники: события System (WER-SystemErrorReporting 1001 — bugcheck,
// Kernel-Power 41 — неожиданная перезагрузка) и файлы C:\Windows\Minidump,
// из заголовка которых читается код остановки. Виновный драйвер без
// отладочных символов не определить — показываем код, имя и вероятную причину.

use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CrashEntry {
    pub time: String,
    /// "bugcheck" | "kernel-power" | "minidump"
    pub source: String,
    pub code: String,
    pub name: String,
    pub hint: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CrashHistory {
    pub entries: Vec<CrashEntry>,
    pub minidump_files: u32,
}

/// (код, имя, вероятная причина)
fn describe(code: u32) -> (&'static str, &'static str) {
    match code {
        0x0 => ("UNEXPECTED_SHUTDOWN", "Внезапное отключение/перезагрузка без синего экрана: питание, перегрев, БП/зарядка, плата"),
        0x0A => ("IRQL_NOT_LESS_OR_EQUAL", "Драйвер или ОЗУ"),
        0x1A => ("MEMORY_MANAGEMENT", "Чаще всего ОЗУ — запустите тест памяти"),
        0x1E => ("KMODE_EXCEPTION_NOT_HANDLED", "Драйвер, реже ОЗУ"),
        0x24 => ("NTFS_FILE_SYSTEM", "Диск или файловая система — проверьте здоровье и чтение диска"),
        0x3B => ("SYSTEM_SERVICE_EXCEPTION", "Драйвер (часто видео) или ОЗУ"),
        0x50 => ("PAGE_FAULT_IN_NONPAGED_AREA", "ОЗУ или драйвер"),
        0x7A => ("KERNEL_DATA_INPAGE_ERROR", "Диск, шлейф/разъём диска или ОЗУ"),
        0x7B => ("INACCESSIBLE_BOOT_DEVICE", "Диск/контроллер, режим SATA/NVMe в BIOS"),
        0x7E => ("SYSTEM_THREAD_EXCEPTION_NOT_HANDLED", "Драйвер"),
        0x9F => ("DRIVER_POWER_STATE_FAILURE", "Драйвер и управление питанием"),
        0xC2 => ("BAD_POOL_CALLER", "Драйвер"),
        0xD1 => ("DRIVER_IRQL_NOT_LESS_OR_EQUAL", "Драйвер (часто сетевой/видео) или ОЗУ"),
        0xEF => ("CRITICAL_PROCESS_DIED", "Системный процесс: диск, повреждение системы"),
        0xF4 => ("CRITICAL_OBJECT_TERMINATION", "Диск/контроллер: критический процесс завершён"),
        0x101 => ("CLOCK_WATCHDOG_TIMEOUT", "Процессор, перегрев, BIOS"),
        0x109 => ("CRITICAL_STRUCTURE_CORRUPTION", "ОЗУ или драйвер"),
        0x116 => ("VIDEO_TDR_FAILURE", "Видеодрайвер/видеокарта, перегрев"),
        0x119 => ("VIDEO_SCHEDULER_INTERNAL_ERROR", "Видеодрайвер/видеокарта"),
        0x124 => ("WHEA_UNCORRECTABLE_ERROR", "Аппаратная ошибка: процессор, ОЗУ, питание, перегрев"),
        0x133 => ("DPC_WATCHDOG_VIOLATION", "Драйвер или диск (SSD), прошивка"),
        0x139 => ("KERNEL_SECURITY_CHECK_FAILURE", "Драйвер или ОЗУ"),
        0x154 => ("UNEXPECTED_STORE_EXCEPTION", "Диск (SSD) или драйвер хранилища"),
        _ => ("НЕИЗВЕСТНЫЙ_КОД", "Код нет в справочнике приложения — смотрите документацию Microsoft по Bug Check"),
    }
}

fn entry(time: String, source: &str, code: u32) -> CrashEntry {
    let (name, hint) = describe(code);
    CrashEntry { time, source: source.to_string(), code: format!("0x{code:X}"), name: name.to_string(), hint: hint.to_string() }
}

/// Первое шестнадцатеричное число вида 0x… в тексте события.
fn first_hex(text: &str) -> Option<u32> {
    let pos = text.find("0x").or_else(|| text.find("0X"))?;
    let digits: String = text[pos + 2..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u32::from_str_radix(&digits, 16).ok()
}

#[derive(Debug, Deserialize, Default)]
struct RawEvent {
    #[serde(default)]
    time: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    text: String,
}

#[tauri::command(async)]
pub fn get_crash_history(days: u32) -> Result<CrashHistory, String> {
    #[cfg(target_os = "windows")]
    {
        let days = days.clamp(1, 3650);
        let script = format!(
            r#"
            $since = (Get-Date).AddDays(-{days})
            $ev = @()
            try {{
                $ev += @(Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Microsoft-Windows-WER-SystemErrorReporting'; Id=1001; StartTime=$since}} -MaxEvents 30 -ErrorAction Stop |
                    ForEach-Object {{ [PSCustomObject]@{{ time = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); source = 'bugcheck'; text = [string]$_.Message }} }})
            }} catch {{}}
            try {{
                $ev += @(Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Microsoft-Windows-Kernel-Power'; Id=41; StartTime=$since}} -MaxEvents 30 -ErrorAction Stop |
                    ForEach-Object {{ [PSCustomObject]@{{ time = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); source = 'kernel-power'; text = [string]$_.Properties[0].Value }} }})
            }} catch {{}}
            ConvertTo-Json -InputObject $ev -Compress
        "#
        );
        let raw: Vec<RawEvent> = run_ps_json(&script)?;
        let mut entries: Vec<CrashEntry> = raw
            .into_iter()
            .filter_map(|e| {
                let code = if e.source == "bugcheck" { first_hex(&e.text)? } else { e.text.trim().parse::<u32>().ok()? };
                Some(entry(e.time, &e.source, code))
            })
            .collect();

        // Minidump-файлы: код остановки из заголовка DUMP_HEADER64 (смещение 0x38).
        let mut minidump_files = 0u32;
        let dir = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("Minidump");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for f in rd.flatten() {
                let path = f.path();
                if path.extension().map(|e| e.eq_ignore_ascii_case("dmp")) != Some(true) {
                    continue;
                }
                minidump_files += 1;
                use std::io::Read;
                let mut head = [0u8; 64];
                let ok = std::fs::File::open(&path).and_then(|mut fh| fh.read_exact(&mut head)).is_ok();
                if ok && &head[0..4] == b"PAGE" && &head[4..8] == b"DU64" {
                    let code = u32::from_le_bytes([head[56], head[57], head[58], head[59]]);
                    let time = f
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    entries.push(entry(time, "minidump", code));
                }
            }
        }

        entries.sort_by(|a, b| b.time.cmp(&a.time));
        entries.truncate(40);
        Ok(CrashHistory { entries, minidump_files })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = days;
        Err("Журнал сбоев доступен только в Windows-сборке".to_string())
    }
}
