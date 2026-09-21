// Периферия из заводского набора SFT: LAN, USB-накопитель (запись/чтение),
// яркость матрицы, внешние мониторы и уровень сигнала Wi-Fi.

use crate::powershell::{run_ps, run_ps_json};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- LAN

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LanAdapter {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub speed: String,
    #[serde(default)]
    pub mac: String,
}

#[tauri::command(async)]
pub fn list_lan_adapters() -> Result<Vec<LanAdapter>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $items = @(Get-NetAdapter -Physical -ErrorAction SilentlyContinue |
                Where-Object { $_.PhysicalMediaType -eq '802.3' } |
                ForEach-Object {
                    [PSCustomObject]@{
                        name = [string]$_.Name
                        description = [string]$_.InterfaceDescription
                        status = [string]$_.Status
                        speed = [string]$_.LinkSpeed
                        mac = [string]$_.MacAddress
                    }
                })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<LanAdapter>>(script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

// ------------------------------------------------------ Wi-Fi: сигнал

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct WifiNetwork {
    #[serde(default)]
    pub ssid: String,
    #[serde(default)]
    pub signal: u32,
}

/// Видимые сети с максимальным уровнем сигнала (%) по каждой. Разбор не зависит
/// от языка Windows: SSID ищется по слову SSID, сигнал — по числу с «%».
#[tauri::command(async)]
pub fn scan_wifi_detailed() -> Result<Vec<WifiNetwork>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $cur = $null
            $map = [ordered]@{}
            netsh wlan show networks mode=bssid | ForEach-Object {
                if ($_ -match '^\s*SSID\s+\d+\s*:\s*(.*)$') {
                    $cur = $Matches[1].Trim()
                    if (-not $map.Contains($cur)) { $map[$cur] = 0 }
                } elseif ($cur -and $_ -match ':\s*(\d+)%\s*$') {
                    $v = [int]$Matches[1]
                    if ($v -gt $map[$cur]) { $map[$cur] = $v }
                }
            }
            $items = @($map.GetEnumerator() | ForEach-Object { [PSCustomObject]@{ ssid = [string]$_.Key; signal = [int]$_.Value } })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<WifiNetwork>>(script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

// --------------------------------------------------- Внешние мониторы

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MonitorInfo {
    #[serde(default)]
    pub name: String,
    /// HDMI | DVI | DisplayPort | VGA | internal | other (перевод — на фронтенде,
    /// чтобы не зависеть от кодировки вывода PowerShell)
    #[serde(default)]
    pub connection: String,
    #[serde(default)]
    pub internal: bool,
}

#[tauri::command(async)]
pub fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $types = @{}
            try {
                Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorConnectionParams -ErrorAction Stop |
                    ForEach-Object { $types[[string]$_.InstanceName] = [int64]$_.VideoOutputTechnology }
            } catch {}
            $items = @()
            try {
                $items = @(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorID -ErrorAction Stop | ForEach-Object {
                    $n = ($_.UserFriendlyName | Where-Object { $_ -ne 0 } | ForEach-Object { [char]$_ }) -join ''
                    if (-not $n) { $n = 'Monitor' }
                    $t = $types[[string]$_.InstanceName]
                    $conn = switch ($t) {
                        5 { 'HDMI' } 3 { 'DVI' } 4 { 'DVI' } 10 { 'DisplayPort' } 0 { 'VGA' }
                        2147483648 { 'internal' } 6 { 'internal' } 8 { 'internal' } 9 { 'internal' } 11 { 'internal' }
                        default { 'other' }
                    }
                    [PSCustomObject]@{
                        name = $n
                        connection = $conn
                        internal = ($t -eq 2147483648 -or $t -eq 6 -or $t -eq 8 -or $t -eq 9 -or $t -eq 11)
                    }
                })
            } catch {}
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<MonitorInfo>>(script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

// ------------------------------------------------------------ Яркость

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BrightnessInfo {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub current: u32,
    #[serde(default)]
    pub min: u32,
    #[serde(default)]
    pub max: u32,
}

#[tauri::command(async)]
pub fn get_brightness() -> Result<BrightnessInfo, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            try {
                $b = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1
                if ($null -eq $b) { throw 'none' }
                $levels = @($b.Level)
                [PSCustomObject]@{ available = $true; current = [int]$b.CurrentBrightness; min = [int]($levels | Measure-Object -Minimum).Minimum; max = [int]($levels | Measure-Object -Maximum).Maximum } | ConvertTo-Json -Compress
            } catch {
                [PSCustomObject]@{ available = $false; current = 0; min = 0; max = 100 } | ConvertTo-Json -Compress
            }
        "#;
        run_ps_json::<BrightnessInfo>(script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[tauri::command(async)]
pub fn set_brightness(level: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let level = level.min(100);
        run_ps(&format!(
            "Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods | \
             Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{ Timeout = 1; Brightness = {level} }} | Out-Null"
        ))
        .map(|_| ())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = level;
        Err("Доступно только в Windows-сборке".to_string())
    }
}

// ------------------------------------------------------- USB-накопители

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UsbDrive {
    #[serde(default)]
    pub letter: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub size_gb: f64,
    #[serde(default)]
    pub free_gb: f64,
    #[serde(default)]
    pub fs: String,
}

#[tauri::command(async)]
pub fn list_removable_drives() -> Result<Vec<UsbDrive>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $items = @(Get-Disk -ErrorAction SilentlyContinue | Where-Object { $_.BusType -eq 'USB' } |
                Get-Partition -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter } |
                ForEach-Object {
                    $v = $_ | Get-Volume
                    [PSCustomObject]@{
                        letter = [string]$_.DriveLetter
                        label = [string]$v.FileSystemLabel
                        size_gb = [math]::Round($v.Size / 1GB, 1)
                        free_gb = [math]::Round($v.SizeRemaining / 1GB, 1)
                        fs = [string]$v.FileSystem
                    }
                })
            ConvertTo-Json -InputObject $items -Compress
        "#;
        run_ps_json::<Vec<UsbDrive>>(script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DriveTestResult {
    pub letter: String,
    pub size_mb: u64,
    pub write_mbps: f64,
    pub read_mbps: f64,
    pub errors: u64,
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Запись временного файла на флешку и чтение с проверкой. Обходим кэш Windows
/// (NO_BUFFERING + WRITE_THROUGH), иначе скорость показала бы память, а не порт.
/// Системный диск не трогаем.
#[tauri::command]
pub async fn test_removable_drive(letter: String, size_mb: u64) -> Result<DriveTestResult, String> {
    tauri::async_runtime::spawn_blocking(move || drive_test(&letter, size_mb))
        .await
        .map_err(|e| format!("Тест накопителя завершился аварийно: {e}"))
        .and_then(|r| r)
}

#[cfg(target_os = "windows")]
fn drive_test(letter: &str, size_mb: u64) -> Result<DriveTestResult, String> {
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::Instant;

    const NO_BUFFERING: u32 = 0x2000_0000;
    const WRITE_THROUGH: u32 = 0x8000_0000;
    const BLK: usize = 1 << 20;

    let letter = letter.trim().trim_end_matches(':').to_uppercase();
    let ch = letter.chars().next().filter(|c| c.is_ascii_alphabetic() && letter.len() == 1).ok_or("Некорректная буква диска")?;
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).trim_end_matches(':').to_uppercase();
    if letter == sys {
        return Err("Системный диск не тестируется — выберите USB-накопитель".to_string());
    }
    let size_mb = size_mb.clamp(8, 512);
    let path = format!(r"{ch}:\echips_test_{}.tmp", std::process::id());

    let mut raw = vec![0u8; BLK + 4096];
    let off = raw.as_ptr().align_offset(4096);
    let fill = |buf: &mut [u8], block: u64| {
        for (i, c) in buf.chunks_exact_mut(8).enumerate() {
            c.copy_from_slice(&splitmix((block << 32) | i as u64).to_le_bytes());
        }
    };

    let mut run = || -> Result<DriveTestResult, String> {
        let t = Instant::now();
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .custom_flags(NO_BUFFERING | WRITE_THROUGH)
                .open(&path)
                .map_err(|e| format!("Не удалось создать файл на {ch}: (нет места или защита от записи): {e}"))?;
            for b in 0..size_mb {
                let buf = &mut raw[off..off + BLK];
                fill(buf, b);
                f.write_all(buf).map_err(|e| format!("Ошибка записи на {ch}: {e}"))?;
            }
        }
        let write_secs = t.elapsed().as_secs_f64().max(0.001);

        let t = Instant::now();
        let mut errors = 0u64;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(NO_BUFFERING)
            .open(&path)
            .map_err(|e| format!("Не удалось открыть файл для чтения: {e}"))?;
        let mut expect = vec![0u8; BLK];
        for b in 0..size_mb {
            let buf = &mut raw[off..off + BLK];
            f.read_exact(buf).map_err(|e| format!("Ошибка чтения с {ch}: {e}"))?;
            fill(&mut expect, b);
            if buf != &expect[..] {
                errors += 1;
            }
        }
        let read_secs = t.elapsed().as_secs_f64().max(0.001);
        Ok(DriveTestResult {
            letter: ch.to_string(),
            size_mb,
            write_mbps: size_mb as f64 / write_secs,
            read_mbps: size_mb as f64 / read_secs,
            errors,
        })
    };
    let result = run();
    let _ = std::fs::remove_file(&path);
    result
}

#[cfg(not(target_os = "windows"))]
fn drive_test(_letter: &str, _size_mb: u64) -> Result<DriveTestResult, String> {
    Err("Тест накопителя доступен только в Windows-сборке".to_string())
}
