// SMART дисков (как в CrystalDiskInfo / Victoria).
//  - SATA: атрибуты и пороги из WMI (MSStorageDriver_ATAPISmartData /
//    FailurePredictThresholds / FailurePredictStatus), нужны права администратора.
//  - NVMe: лог здоровья (Log Page 0x02) прямым запросом
//    IOCTL_STORAGE_QUERY_PROPERTY к \\.\PhysicalDriveN.
//  - Иначе (USB-переходники, RAID и т. п.) — базовые счётчики Windows.

use super::smart_parse::{self as sp, NvmeHealth, SmartAttr};
use crate::powershell::run_ps_json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SmartDisk {
    pub number: u32,
    pub name: String,
    pub serial: String,
    pub firmware: String,
    pub bus: String,
    pub media: String,
    pub size_gb: f64,
    /// Системный диск (с ним связан том C:)
    pub is_system: bool,
    /// "ata" | "nvme" | "basic" — откуда получены данные
    pub kind: String,
    /// "good" | "caution" | "bad" | "unknown"
    pub status: String,
    /// Остаток ресурса, % (SSD/NVMe)
    pub health_pct: Option<f64>,
    pub temp_c: Option<f64>,
    pub power_on_hours: Option<u64>,
    pub power_cycles: Option<u64>,
    pub written_gb: Option<f64>,
    pub read_gb: Option<f64>,
    pub attrs: Vec<SmartAttr>,
    pub nvme: Option<NvmeHealth>,
    pub notes: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct RawDisk {
    #[serde(default)]
    number: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    serial: String,
    #[serde(default)]
    firmware: String,
    #[serde(default)]
    bus: String,
    #[serde(default)]
    media: String,
    #[serde(default)]
    size_gb: f64,
    #[serde(default)]
    is_system: bool,
    #[serde(default)]
    ata_hex: String,
    #[serde(default)]
    thr_hex: String,
    #[serde(default)]
    predict_failure: Option<bool>,
    #[serde(default)]
    rel_temp_c: Option<f64>,
    #[serde(default)]
    rel_wear: Option<f64>,
    #[serde(default)]
    rel_hours: Option<u64>,
}

#[tauri::command(async)]
pub fn get_smart_report() -> Result<Vec<SmartDisk>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
            $sysDisk = $null
            try { $sysDisk = (Get-Partition -DriveLetter C -ErrorAction Stop).DiskNumber } catch {}
            $phys = @{}
            Get-PhysicalDisk -ErrorAction SilentlyContinue | ForEach-Object { $phys[[int]$_.DeviceId] = $_ }
            $smart = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_ATAPISmartData -ErrorAction SilentlyContinue)
            $thr = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictThresholds -ErrorAction SilentlyContinue)
            $pred = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictStatus -ErrorAction SilentlyContinue)
            function ToHex($b) { if ($null -eq $b) { return '' }; return ([BitConverter]::ToString([byte[]]$b)).Replace('-', '') }
            function ForDisk($list, $pnp) { $list | Where-Object { $_.InstanceName -and $_.InstanceName.StartsWith($pnp, [StringComparison]::OrdinalIgnoreCase) } | Select-Object -First 1 }
            $items = @(Get-CimInstance Win32_DiskDrive | ForEach-Object {
                $dd = $_
                $pnp = [string]$dd.PNPDeviceID
                $s = ForDisk $smart $pnp
                $t = ForDisk $thr $pnp
                $p = ForDisk $pred $pnp
                $ph = $phys[[int]$dd.Index]
                $r = $null
                if ($ph) { try { $r = $ph | Get-StorageReliabilityCounter -ErrorAction Stop } catch {} }
                [PSCustomObject]@{
                    number = [int]$dd.Index
                    name = [string]$dd.Model
                    serial = ([string]$dd.SerialNumber).Trim()
                    firmware = [string]$dd.FirmwareRevision
                    bus = if ($ph) { [string]$ph.BusType } else { [string]$dd.InterfaceType }
                    media = if ($ph) { [string]$ph.MediaType } else { '' }
                    size_gb = [math]::Round($dd.Size / 1GB, 0)
                    is_system = ($null -ne $sysDisk -and [int]$dd.Index -eq [int]$sysDisk)
                    ata_hex = if ($s) { ToHex $s.VendorSpecific } else { '' }
                    thr_hex = if ($t) { ToHex $t.VendorSpecific } else { '' }
                    predict_failure = if ($p) { [bool]$p.PredictFailure } else { $null }
                    rel_temp_c = if ($r -and $r.Temperature) { $r.Temperature } else { $null }
                    rel_wear = if ($r) { $r.Wear } else { $null }
                    rel_hours = if ($r) { $r.PowerOnHours } else { $null }
                }
            })
            ConvertTo-Json -InputObject $items -Compress -Depth 3
        "#;
        let raws: Vec<RawDisk> = run_ps_json(script)?;
        Ok(raws.into_iter().map(build_disk).collect())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("SMART доступен только в Windows-сборке".to_string())
    }
}

#[cfg(target_os = "windows")]
fn build_disk(r: RawDisk) -> SmartDisk {
    let mut d = SmartDisk {
        number: r.number,
        name: r.name.clone(),
        serial: r.serial.clone(),
        firmware: r.firmware.clone(),
        bus: r.bus.clone(),
        media: r.media.clone(),
        size_gb: r.size_gb,
        is_system: r.is_system,
        status: "unknown".into(),
        ..Default::default()
    };
    let is_nvme = r.bus.to_lowercase().contains("nvme");

    if is_nvme {
        match nvme_health_log(r.number) {
            Ok(buf) => {
                if let Some(h) = sp::parse_nvme_health(&buf) {
                    d.kind = "nvme".into();
                    d.status = sp::nvme_overall(&h).into();
                    d.health_pct = Some(100.0 - h.percentage_used as f64);
                    d.temp_c = Some((h.temp_c * 10.0).round() / 10.0);
                    d.power_on_hours = Some(h.power_on_hours);
                    d.power_cycles = Some(h.power_cycles);
                    d.written_gb = Some(h.data_written_gb);
                    d.read_gb = Some(h.data_read_gb);
                    if h.critical_warning != 0 {
                        d.notes.push(format!("Критическое предупреждение контроллера: 0x{:02X}", h.critical_warning));
                    }
                    if h.media_errors > 0 {
                        d.notes.push(format!("Ошибки целостности данных: {}", h.media_errors));
                    }
                    d.nvme = Some(h);
                    return d;
                }
                d.notes.push("NVMe: получен лог неожиданного размера".into());
            }
            Err(e) => d.notes.push(format!("NVMe SMART недоступен: {e}")),
        }
    } else if !r.ata_hex.is_empty() {
        let data = sp::hex_decode(&r.ata_hex);
        let thr = sp::parse_thresholds(&sp::hex_decode(&r.thr_hex));
        let attrs = sp::parse_ata(&data, &thr);
        if !attrs.is_empty() {
            let life = sp::ssd_life_percent(&attrs);
            d.kind = "ata".into();
            d.status = sp::ata_overall(&attrs, r.predict_failure, life).into();
            d.health_pct = life;
            d.temp_c = sp::find_raw(&attrs, 194).or_else(|| sp::find_raw(&attrs, 190)).map(|v| (v & 0xFF) as f64);
            d.power_on_hours = sp::find_raw(&attrs, 9).map(|v| v & 0xFFFF_FFFF);
            d.power_cycles = sp::find_raw(&attrs, 12).map(|v| v & 0xFFFF_FFFF);
            // LBA по 512 байт — оценка: у части производителей единица другая
            d.written_gb = sp::find_raw(&attrs, 241).map(|v| v as f64 * 512.0 / 1e9);
            d.read_gb = sp::find_raw(&attrs, 242).map(|v| v as f64 * 512.0 / 1e9);
            // у ряда SSD единица атрибута 241/242 не LBA×512 (получается «0 ГБ» при тысячах часов
            // работы) — такое значение не показываем, чтобы не вводить в заблуждение
            if d.power_on_hours.unwrap_or(0) > 200 {
                if d.written_gb.map(|g| g < 1.0).unwrap_or(false) {
                    d.written_gb = None;
                }
                if d.read_gb.map(|g| g < 1.0).unwrap_or(false) {
                    d.read_gb = None;
                }
            }
            if r.predict_failure == Some(true) {
                d.notes.push("Диск сам предсказывает скорый отказ (PredictFailure)".into());
            }
            d.attrs = attrs;
            return d;
        }
    }

    // Запасной вариант: базовые счётчики Windows
    d.kind = "basic".into();
    d.temp_c = r.rel_temp_c;
    d.power_on_hours = r.rel_hours;
    if let Some(w) = r.rel_wear {
        d.health_pct = Some(100.0 - w);
    }
    if d.temp_c.is_some() || d.power_on_hours.is_some() || d.health_pct.is_some() {
        d.status = match d.health_pct {
            Some(h) if h <= 10.0 => "bad",
            Some(h) if h <= 50.0 => "caution",
            _ => "good",
        }
        .into();
    }
    d.notes.push("Полный SMART недоступен для этого диска (USB-переходник, RAID или драйвер) — показаны базовые счётчики Windows".into());
    d
}

/// Лог здоровья NVMe: IOCTL_STORAGE_QUERY_PROPERTY, StorageDeviceProtocolSpecificProperty.
#[cfg(target_os = "windows")]
fn nvme_health_log(disk: u32) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    extern "system" {
        fn DeviceIoControl(
            h: *mut c_void,
            code: u32,
            in_buf: *const c_void,
            in_size: u32,
            out_buf: *mut c_void,
            out_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;
    const PROTOCOL_TYPE_NVME: u32 = 3;
    const NVME_DATA_TYPE_LOG_PAGE: u32 = 2;
    const NVME_LOG_PAGE_HEALTH_INFO: u32 = 2;
    const HEADER: usize = 8; // PropertyId + QueryType
    const PROTO_STRUCT: usize = 40; // STORAGE_PROTOCOL_SPECIFIC_DATA
    const LOG_LEN: usize = 512;

    let path = format!(r"\\.\PhysicalDrive{disk}");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .or_else(|_| std::fs::OpenOptions::new().read(true).open(&path))
        .map_err(|e| format!("не удалось открыть {path} (нужны права администратора): {e}"))?;

    let mut last_err = String::new();
    for sub_value in [0u32, 0xFFFF_FFFF] {
        let total = HEADER + PROTO_STRUCT + LOG_LEN;
        let mut buf = vec![0u8; total];
        let put = |b: &mut Vec<u8>, off: usize, v: u32| b[off..off + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut buf, 0, STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY);
        put(&mut buf, 4, 0); // PropertyStandardQuery
        put(&mut buf, 8, PROTOCOL_TYPE_NVME);
        put(&mut buf, 12, NVME_DATA_TYPE_LOG_PAGE);
        put(&mut buf, 16, NVME_LOG_PAGE_HEALTH_INFO);
        put(&mut buf, 20, sub_value);
        put(&mut buf, 24, PROTO_STRUCT as u32); // ProtocolDataOffset
        put(&mut buf, 28, LOG_LEN as u32); // ProtocolDataLength

        let mut returned: u32 = 0;
        let ptr = buf.as_mut_ptr() as *mut c_void;
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle() as *mut c_void,
                IOCTL_STORAGE_QUERY_PROPERTY,
                ptr as *const c_void,
                total as u32,
                ptr,
                total as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            last_err = format!("DeviceIoControl вернул ошибку: {}", std::io::Error::last_os_error());
            continue;
        }
        let off = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]) as usize;
        let len = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]) as usize;
        let start = HEADER + off;
        if len < 200 || start + len > buf.len() {
            last_err = format!("неожиданный размер ответа ({len} байт)");
            continue;
        }
        return Ok(buf[start..start + len].to_vec());
    }
    Err(last_err)
}
