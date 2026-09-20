// Разбор SMART без зависимостей от Windows API: атрибуты ATA (512-байтный блок
// MSStorageDriver_ATAPISmartData), пороги и лог здоровья NVMe (страница 0x02).
// Отдельный файл — чтобы проверять юнит-тестами на любой ОС:
//   скопировать файл в пустой cargo-проект с serde и выполнить cargo test

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct SmartAttr {
    pub id: u8,
    pub name: String,
    pub current: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw: u64,
    /// "ok" | "warn" | "bad"
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct NvmeHealth {
    pub critical_warning: u8,
    pub temp_c: f64,
    pub available_spare: u8,
    pub spare_threshold: u8,
    pub percentage_used: u8,
    pub data_read_gb: f64,
    pub data_written_gb: f64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log_entries: u64,
}

pub fn hex_decode(s: &str) -> Vec<u8> {
    let s = s.trim();
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len() / 2);
    let mut i = 0;
    while i + 1 < b.len() {
        let hi = (b[i] as char).to_digit(16);
        let lo = (b[i + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push((h * 16 + l) as u8),
            _ => return out,
        }
        i += 2;
    }
    out
}

pub fn attr_name(id: u8) -> &'static str {
    match id {
        1 => "Read Error Rate",
        2 => "Throughput Performance",
        3 => "Spin-Up Time",
        4 => "Start/Stop Count",
        5 => "Reallocated Sectors Count",
        7 => "Seek Error Rate",
        8 => "Seek Time Performance",
        9 => "Power-On Hours",
        10 => "Spin Retry Count",
        11 => "Calibration Retry Count",
        12 => "Power Cycle Count",
        13 => "Soft Read Error Rate",
        168 => "SATA Phy Error Count",
        170 => "Available Reserved Space",
        171 => "Program Fail Count",
        172 => "Erase Fail Count",
        173 => "Wear Leveling Count",
        174 => "Unexpected Power Loss Count",
        175 => "Power Loss Protection Failure",
        177 => "Wear Leveling Count",
        178 => "Used Reserved Block Count",
        179 => "Used Reserved Block Count (Total)",
        180 => "Unused Reserved Block Count",
        181 => "Program Fail Count (Total)",
        182 => "Erase Fail Count",
        183 => "Runtime Bad Block",
        184 => "End-to-End Error",
        187 => "Reported Uncorrectable Errors",
        188 => "Command Timeout",
        189 => "High Fly Writes",
        190 => "Airflow Temperature",
        191 => "G-Sense Error Rate",
        192 => "Power-Off Retract Count",
        193 => "Load/Unload Cycle Count",
        194 => "Temperature",
        195 => "Hardware ECC Recovered",
        196 => "Reallocation Event Count",
        197 => "Current Pending Sector Count",
        198 => "Uncorrectable Sector Count",
        199 => "UltraDMA CRC Error Count",
        200 => "Write Error Rate",
        201 => "Soft Read Error Rate",
        202 => "Data Address Mark Errors",
        203 => "Run Out Cancel",
        204 => "Soft ECC Correction",
        205 => "Thermal Asperity Rate",
        206 => "Flying Height",
        207 => "Spin High Current",
        208 => "Spin Buzz",
        209 => "Offline Seek Performance",
        220 => "Disk Shift",
        221 => "G-Sense Error Rate",
        222 => "Loaded Hours",
        223 => "Load/Unload Retry Count",
        224 => "Load Friction",
        225 => "Load/Unload Cycle Count",
        226 => "Load-in Time",
        227 => "Torque Amplification Count",
        228 => "Power-Off Retract Count",
        230 => "Drive Life Protection Status",
        231 => "SSD Life Left",
        232 => "Endurance Remaining",
        233 => "Media Wearout Indicator",
        234 => "Average Erase Count",
        235 => "Good Block Count",
        240 => "Head Flying Hours",
        241 => "Total LBAs Written",
        242 => "Total LBAs Read",
        243 => "Total LBAs Written (Expanded)",
        244 => "Total LBAs Read (Expanded)",
        245 => "NAND Writes",
        246 => "Total Host Sector Writes",
        247 => "Host Program Page Count",
        248 => "Background Program Page Count",
        249 => "NAND Writes (1GiB)",
        250 => "Read Error Retry Rate",
        251 => "Minimum Spares Remaining",
        252 => "Newly Added Bad Flash Block",
        254 => "Free Fall Protection",
        _ => "Vendor Specific",
    }
}

pub fn raw_u48(raw: &[u8]) -> u64 {
    raw.iter().take(6).enumerate().fold(0u64, |acc, (i, b)| acc | ((*b as u64) << (8 * i)))
}

/// Пороги: блок 512 байт, записи по 12 байт с 2-го байта: [id, порог, ...].
pub fn parse_thresholds(data: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    for i in 0..30 {
        let o = 2 + i * 12;
        if o + 2 > data.len() {
            break;
        }
        let id = data[o];
        if id != 0 {
            out.push((id, data[o + 1]));
        }
    }
    out
}

/// Атрибуты: блок 512 байт, записи по 12 байт с 2-го байта:
/// [id, флаги(2), текущее, худшее, raw(6), резерв].
pub fn parse_ata(data: &[u8], thresholds: &[(u8, u8)]) -> Vec<SmartAttr> {
    let mut out = Vec::new();
    for i in 0..30 {
        let o = 2 + i * 12;
        if o + 12 > data.len() {
            break;
        }
        let id = data[o];
        if id == 0 {
            continue;
        }
        let current = data[o + 3];
        let worst = data[o + 4];
        let raw = raw_u48(&data[o + 5..o + 11]);
        let threshold = thresholds.iter().find(|(tid, _)| *tid == id).map(|(_, t)| *t).unwrap_or(0);
        let mut a = SmartAttr { id, name: attr_name(id).to_string(), current, worst, threshold, raw, status: "ok".into() };
        a.status = attr_status(&a).to_string();
        out.push(a);
    }
    out
}

/// Оценка атрибута в духе CrystalDiskInfo: ниже порога — плохо; счётчики
/// переназначенных/нестабильных секторов и ошибок > 0 — предупреждение.
pub fn attr_status(a: &SmartAttr) -> &'static str {
    if a.threshold > 0 && a.current <= a.threshold {
        return "bad";
    }
    let counter_ids: [u8; 9] = [5, 10, 183, 184, 187, 196, 197, 198, 199];
    if counter_ids.contains(&a.id) && (a.raw & 0xFFFF_FFFF) > 0 {
        return "warn";
    }
    "ok"
}

pub fn find_raw(attrs: &[SmartAttr], id: u8) -> Option<u64> {
    attrs.iter().find(|a| a.id == id).map(|a| a.raw)
}

/// Остаток ресурса SSD в % по нормализованному значению атрибутов износа.
pub fn ssd_life_percent(attrs: &[SmartAttr]) -> Option<f64> {
    for id in [231u8, 233, 232, 177, 173, 169] {
        if let Some(a) = attrs.iter().find(|a| a.id == id) {
            if a.current > 0 && a.current <= 100 {
                return Some(a.current as f64);
            }
        }
    }
    None
}

/// Итог по диску: "good" | "caution" | "bad".
pub fn ata_overall(attrs: &[SmartAttr], predict_failure: Option<bool>, life: Option<f64>) -> &'static str {
    if predict_failure == Some(true) || attrs.iter().any(|a| a.status == "bad") {
        return "bad";
    }
    if let Some(l) = life {
        if l <= 10.0 {
            return "bad";
        }
        if l <= 50.0 {
            return "caution";
        }
    }
    if attrs.iter().any(|a| a.status == "warn") {
        return "caution";
    }
    "good"
}

fn le_u128(b: &[u8]) -> u128 {
    b.iter().take(16).enumerate().fold(0u128, |acc, (i, x)| acc | ((*x as u128) << (8 * i)))
}

/// Лог здоровья NVMe (Log Page 0x02, 512 байт).
pub fn parse_nvme_health(b: &[u8]) -> Option<NvmeHealth> {
    if b.len() < 208 {
        return None;
    }
    let temp_k = u16::from_le_bytes([b[1], b[2]]) as f64;
    Some(NvmeHealth {
        critical_warning: b[0],
        temp_c: if temp_k > 0.0 { temp_k - 273.15 } else { 0.0 },
        available_spare: b[3],
        spare_threshold: b[4],
        percentage_used: b[5],
        // единица = 1000 блоков по 512 байт = 512 000 байт
        data_read_gb: le_u128(&b[32..48]) as f64 * 512_000.0 / 1e9,
        data_written_gb: le_u128(&b[48..64]) as f64 * 512_000.0 / 1e9,
        power_cycles: le_u128(&b[112..128]) as u64,
        power_on_hours: le_u128(&b[128..144]) as u64,
        unsafe_shutdowns: le_u128(&b[144..160]) as u64,
        media_errors: le_u128(&b[160..176]) as u64,
        error_log_entries: le_u128(&b[176..192]) as u64,
    })
}

pub fn nvme_overall(h: &NvmeHealth) -> &'static str {
    if h.critical_warning != 0 || h.available_spare <= h.spare_threshold && h.spare_threshold > 0 || h.percentage_used >= 90 {
        return "bad";
    }
    let life = 100u8.saturating_sub(h.percentage_used);
    if life <= 50 || h.media_errors > 0 {
        return "caution";
    }
    "good"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(entries: &[(u8, u8, u8, [u8; 6])]) -> Vec<u8> {
        let mut d = vec![0u8; 512];
        for (i, (id, cur, worst, raw)) in entries.iter().enumerate() {
            let o = 2 + i * 12;
            d[o] = *id;
            d[o + 3] = *cur;
            d[o + 4] = *worst;
            d[o + 5..o + 11].copy_from_slice(raw);
        }
        d
    }

    #[test]
    fn hex() {
        assert_eq!(hex_decode("00FF10aB"), vec![0x00, 0xFF, 0x10, 0xAB]);
        assert_eq!(hex_decode(""), Vec::<u8>::new());
    }

    #[test]
    fn ata_parse_and_status() {
        let data = block(&[
            (9, 95, 95, [0x10, 0x27, 0, 0, 0, 0]),  // 10000 часов
            (5, 100, 100, [3, 0, 0, 0, 0, 0]),       // 3 переназначенных
            (194, 60, 50, [40, 0, 0, 0, 0, 0]),      // 40 °C
            (231, 88, 88, [0, 0, 0, 0, 0, 0]),       // остаток 88%
        ]);
        let mut thr = vec![0u8; 512];
        for (i, (id, t)) in [(9u8, 0u8), (5, 10), (194, 0), (231, 10)].iter().enumerate() {
            thr[2 + i * 12] = *id;
            thr[2 + i * 12 + 1] = *t;
        }
        let attrs = parse_ata(&data, &parse_thresholds(&thr));
        assert_eq!(attrs.len(), 4);
        assert_eq!(find_raw(&attrs, 9), Some(10000));
        assert_eq!(attrs[1].status, "warn"); // raw>0 у атрибута 5
        assert_eq!(attrs[2].raw & 0xFF, 40);
        assert_eq!(ssd_life_percent(&attrs), Some(88.0));
        assert_eq!(ata_overall(&attrs, Some(false), Some(88.0)), "caution");
        assert_eq!(ata_overall(&attrs, Some(true), Some(88.0)), "bad");
    }

    #[test]
    fn ata_threshold_bad() {
        let data = block(&[(5, 8, 8, [0; 6])]);
        let mut thr = vec![0u8; 512];
        thr[2] = 5;
        thr[3] = 10;
        let attrs = parse_ata(&data, &parse_thresholds(&thr));
        assert_eq!(attrs[0].status, "bad");
    }

    #[test]
    fn ata_clean_disk_is_good() {
        let data = block(&[(9, 99, 99, [5, 0, 0, 0, 0, 0]), (5, 100, 100, [0; 6]), (197, 100, 100, [0; 6])]);
        let attrs = parse_ata(&data, &[]);
        assert_eq!(ata_overall(&attrs, None, None), "good");
    }

    #[test]
    fn nvme_parse() {
        let mut b = vec![0u8; 512];
        b[1..3].copy_from_slice(&(273u16 + 40).to_le_bytes()); // ~40 °C
        b[3] = 100;
        b[4] = 10;
        b[5] = 7;
        b[32..36].copy_from_slice(&1_000_000u32.to_le_bytes()); // 512 ГБ прочитано
        b[48..52].copy_from_slice(&500_000u32.to_le_bytes());
        b[112] = 123;
        b[128..132].copy_from_slice(&4321u32.to_le_bytes());
        b[144] = 9;
        b[160] = 0;
        let h = parse_nvme_health(&b).unwrap();
        assert!((h.temp_c - 39.85).abs() < 0.01);
        assert_eq!(h.percentage_used, 7);
        assert!((h.data_read_gb - 512.0).abs() < 0.01);
        assert!((h.data_written_gb - 256.0).abs() < 0.01);
        assert_eq!(h.power_cycles, 123);
        assert_eq!(h.power_on_hours, 4321);
        assert_eq!(h.unsafe_shutdowns, 9);
        assert_eq!(nvme_overall(&h), "good");
        assert!(parse_nvme_health(&b[..100]).is_none());
    }

    #[test]
    fn nvme_status_rules() {
        let mut h = NvmeHealth { available_spare: 100, spare_threshold: 10, percentage_used: 40, ..Default::default() };
        assert_eq!(nvme_overall(&h), "good");
        h.percentage_used = 55;
        assert_eq!(nvme_overall(&h), "caution");
        h.percentage_used = 95;
        assert_eq!(nvme_overall(&h), "bad");
        h.percentage_used = 5;
        h.critical_warning = 1;
        assert_eq!(nvme_overall(&h), "bad");
        h.critical_warning = 0;
        h.media_errors = 2;
        assert_eq!(nvme_overall(&h), "caution");
    }
}
