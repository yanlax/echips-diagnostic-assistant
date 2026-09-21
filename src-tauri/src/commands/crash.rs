// Журнал сбоев (аналог BlueScreenView): синие экраны и внезапные перезагрузки.
// Источники: события System (WER-SystemErrorReporting 1001 — bugcheck,
// Kernel-Power 41 — неожиданная перезагрузка) и файлы C:\Windows\Minidump
// (код остановки и параметры читаются из заголовка дампа). Записи трёх
// источников склеиваются в один сбой (окно 2 минуты), рядом собираются
// аппаратные события (WHEA, ошибки диска, TDR видео) и строится диагноз по
// шаблону сбоев. Виновный драйвер без отладочных символов Microsoft не
// определяется — для этого нужен разбор самого дампа.

use super::crash_logic::{self as cl, CrashEntry, Diagnosis, HwCounts, RawRecord};
#[cfg(target_os = "windows")]
use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

/// Окно склейки записей одного сбоя, секунд.
const MERGE_WINDOW_SECS: i64 = 120;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct HwEvent {
    pub time: String,
    /// "whea" | "disk" | "tdr"
    pub kind: String,
    pub provider: String,
    pub id: u32,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CrashHistory {
    pub entries: Vec<CrashEntry>,
    /// Сколько записей журнала было до склейки
    pub raw_records: u32,
    pub minidump_files: u32,
    pub hw_events: Vec<HwEvent>,
    pub hw_counts: HwCounts,
    pub diagnosis: Vec<Diagnosis>,
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

/// Все шестнадцатеричные числа вида 0x… в тексте события, по порядку.
fn hex_tokens(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("0x").or_else(|| rest.find("0X")) {
        let tail = &rest[pos + 2..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if let Ok(v) = u32::from_str_radix(&digits, 16) {
            out.push(v);
        }
        rest = &tail[digits.len()..];
    }
    out
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
                $ev += @(Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Microsoft-Windows-WER-SystemErrorReporting'; Id=1001; StartTime=$since}} -MaxEvents 60 -ErrorAction Stop |
                    ForEach-Object {{ [PSCustomObject]@{{ time = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); source = 'bugcheck'; text = [string]$_.Message }} }})
            }} catch {{}}
            try {{
                $ev += @(Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Microsoft-Windows-Kernel-Power'; Id=41; StartTime=$since}} -MaxEvents 60 -ErrorAction Stop |
                    ForEach-Object {{ [PSCustomObject]@{{ time = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); source = 'kernel-power'; text = [string]$_.Properties[0].Value }} }})
            }} catch {{}}
            ConvertTo-Json -InputObject $ev -Compress
        "#
        );
        let raw: Vec<RawEvent> = run_ps_json(&script)?;
        let mut records: Vec<RawRecord> = raw
            .into_iter()
            .filter_map(|e| {
                if e.source == "bugcheck" {
                    let toks = hex_tokens(&e.text);
                    let code = *toks.first()?;
                    let params = toks.iter().skip(1).take(4).map(|p| format!("0x{p:X}")).collect();
                    Some(RawRecord { time: e.time, source: e.source, code, params })
                } else {
                    let code = e.text.trim().parse::<u32>().ok()?;
                    Some(RawRecord { time: e.time, source: e.source, code, params: vec![] })
                }
            })
            .collect();

        // Minidump-файлы: код и параметры остановки из заголовка DUMP_HEADER64 (0x38 / 0x40…).
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
                let mut head = [0u8; 96];
                let ok = std::fs::File::open(&path).and_then(|mut fh| fh.read_exact(&mut head)).is_ok();
                if ok && &head[0..4] == b"PAGE" && &head[4..8] == b"DU64" {
                    let code = u32::from_le_bytes([head[56], head[57], head[58], head[59]]);
                    let params: Vec<String> = (0..4)
                        .map(|i| {
                            let o = 64 + i * 8;
                            let mut b = [0u8; 8];
                            b.copy_from_slice(&head[o..o + 8]);
                            format!("0x{:X}", u64::from_le_bytes(b))
                        })
                        .collect();
                    let time = f
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    records.push(RawRecord { time, source: "minidump".into(), code, params });
                }
            }
        }

        let raw_records = records.len() as u32;
        let entries = cl::merge_records(records, MERGE_WINDOW_SECS);

        // Аппаратные события System за тот же период: WHEA, ошибки диска, TDR видео.
        let hw_script = format!(
            r#"
            $since = (Get-Date).AddDays(-{days})
            $items = @()
            try {{
                $items = @(Get-WinEvent -FilterHashtable @{{LogName='System'; StartTime=$since; Level=1,2,3}} -MaxEvents 2500 -ErrorAction Stop | ForEach-Object {{
                    $p = [string]$_.ProviderName; $id = [int]$_.Id; $k = $null
                    if ($p -like '*WHEA*') {{ $k = 'whea' }}
                    elseif ($p -eq 'disk' -and (7,11,15,51,52,153) -contains $id) {{ $k = 'disk' }}
                    elseif (($p -eq 'stornvme' -or $p -eq 'storahci' -or $p -like 'iaStor*') -and (129,153,130) -contains $id) {{ $k = 'disk' }}
                    elseif ($p -eq 'Ntfs' -and (55,98,137,140) -contains $id) {{ $k = 'disk' }}
                    elseif ($p -eq 'Display' -and $id -eq 4101) {{ $k = 'tdr' }}
                    if ($k) {{
                        $m = [string]$_.Message; if ($m.Length -gt 140) {{ $m = $m.Substring(0, 140) }}
                        [PSCustomObject]@{{ time = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); kind = $k; provider = $p; id = $id; text = ($m -replace '\s+', ' ') }}
                    }}
                }})
            }} catch {{}}
            ConvertTo-Json -InputObject $items -Compress
        "#
        );
        let hw_events: Vec<HwEvent> = run_ps_json(&hw_script).unwrap_or_default();
        let count = |k: &str| hw_events.iter().filter(|e| e.kind == k).count() as u32;
        let hw_counts = HwCounts { whea: count("whea"), disk: count("disk"), tdr: count("tdr") };
        let diagnosis = cl::diagnose(&entries, &hw_counts);
        let mut hw_events = hw_events;
        hw_events.truncate(30);
        Ok(CrashHistory { entries, raw_records, minidump_files, hw_events, hw_counts, diagnosis })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = days;
        let _ = hex_tokens("");
        Err("Журнал сбоев доступен только в Windows-сборке".to_string())
    }
}
