// Логика журнала сбоев без зависимостей от Windows: справочник кодов остановки,
// склейка записей из трёх источников в один сбой и диагноз по шаблону сбоев.
// Проверяется юнит-тестами (скопировать файл в пустой cargo-проект с serde).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Запись журнала до склейки.
#[derive(Debug, Clone, Default)]
pub struct RawRecord {
    /// "YYYY-MM-DD HH:MM:SS"
    pub time: String,
    /// "bugcheck" | "kernel-power" | "minidump"
    pub source: String,
    pub code: u32,
    pub params: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct CrashEntry {
    pub time: String,
    /// Код как в журнале, например 0x1000007E
    pub code: String,
    pub name: String,
    pub hint: String,
    /// "ram" | "disk" | "driver" | "video" | "hw" | "power" | "system" | "unknown"
    pub category: String,
    /// Источники, из которых склеен сбой
    pub sources: Vec<String>,
    /// Сколько записей журнала объединено
    pub records: u32,
    pub params: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Diagnosis {
    /// "warn" | "info"
    pub level: String,
    pub title: String,
    pub text: String,
    /// id тестов приложения, которые стоит запустить
    pub actions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct HwCounts {
    pub whea: u32,
    pub disk: u32,
    pub tdr: u32,
}

/// (код, имя, вероятная причина, категория)
const TABLE: &[(u32, &str, &str, &str)] = &[
    (0x0, "UNEXPECTED_SHUTDOWN", "Внезапное отключение/перезагрузка без синего экрана: питание, зарядка/БП, перегрев, плата", "power"),
    (0x1, "APC_INDEX_MISMATCH", "Драйвер", "driver"),
    (0xA, "IRQL_NOT_LESS_OR_EQUAL", "Драйвер или ОЗУ", "driver"),
    (0x12, "TRAP_CAUSE_UNKNOWN", "Драйвер, ОЗУ, перегрев", "hw"),
    (0x1A, "MEMORY_MANAGEMENT", "Чаще всего ОЗУ — запустите тест памяти", "ram"),
    (0x1E, "KMODE_EXCEPTION_NOT_HANDLED", "Драйвер, реже ОЗУ", "driver"),
    (0x24, "NTFS_FILE_SYSTEM", "Диск или файловая система", "disk"),
    (0x27, "RDR_FILE_SYSTEM", "Сетевой редиректор / диск", "driver"),
    (0x2E, "DATA_BUS_ERROR", "Ошибка чётности памяти или шины: ОЗУ, плата", "ram"),
    (0x3B, "SYSTEM_SERVICE_EXCEPTION", "Драйвер (часто видео) или ОЗУ", "driver"),
    (0x3F, "NO_MORE_SYSTEM_PTES", "Драйвер (утечка ресурсов)", "driver"),
    (0x4E, "PFN_LIST_CORRUPT", "Повреждение списка страниц памяти: ОЗУ или драйвер", "ram"),
    (0x50, "PAGE_FAULT_IN_NONPAGED_AREA", "ОЗУ или драйвер", "ram"),
    (0x51, "REGISTRY_ERROR", "Диск или повреждение реестра", "disk"),
    (0x58, "FTDISK_INTERNAL_ERROR", "Диск / RAID", "disk"),
    (0x5A, "CRITICAL_SERVICE_FAILED", "Критическая служба: диск, повреждение системы", "system"),
    (0x5C, "HAL_INITIALIZATION_FAILED", "Плата/BIOS", "hw"),
    (0x67, "CONFIG_INITIALIZATION_FAILED", "Реестр/диск", "disk"),
    (0x74, "BAD_SYSTEM_CONFIG_INFO", "Реестр/диск", "disk"),
    (0x77, "KERNEL_STACK_INPAGE_ERROR", "Диск или ОЗУ", "disk"),
    (0x7A, "KERNEL_DATA_INPAGE_ERROR", "Диск, шлейф/разъём диска или ОЗУ", "disk"),
    (0x7B, "INACCESSIBLE_BOOT_DEVICE", "Диск/контроллер, режим SATA/NVMe в BIOS", "disk"),
    (0x7D, "INSTALL_MORE_MEMORY", "Недостаточно ОЗУ / неисправный модуль", "ram"),
    (0x7E, "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED", "Драйвер", "driver"),
    (0x7F, "UNEXPECTED_KERNEL_MODE_TRAP", "ОЗУ, перегрев, разгон, драйвер", "hw"),
    (0x80, "NMI_HARDWARE_FAILURE", "Аппаратный сбой (NMI): плата, ОЗУ", "hw"),
    (0x8E, "KERNEL_MODE_EXCEPTION_NOT_HANDLED", "Драйвер, реже ОЗУ", "driver"),
    (0x9C, "MACHINE_CHECK_EXCEPTION", "Аппаратная ошибка процессора: CPU, питание, перегрев", "hw"),
    (0x9F, "DRIVER_POWER_STATE_FAILURE", "Драйвер и управление питанием", "driver"),
    (0xA0, "INTERNAL_POWER_ERROR", "Питание/ACPI", "power"),
    (0xA5, "ACPI_BIOS_ERROR", "BIOS/ACPI — обновите BIOS", "hw"),
    (0xBE, "ATTEMPTED_WRITE_TO_READONLY_MEMORY", "Драйвер", "driver"),
    (0xC1, "SPECIAL_POOL_DETECTED_MEMORY_CORRUPTION", "Драйвер", "driver"),
    (0xC2, "BAD_POOL_CALLER", "Драйвер", "driver"),
    (0xC4, "DRIVER_VERIFIER_DETECTED_VIOLATION", "Драйвер (включён Driver Verifier)", "driver"),
    (0xC5, "DRIVER_CORRUPTED_EXPOOL", "Драйвер или ОЗУ", "ram"),
    (0xCA, "PNP_DETECTED_FATAL_ERROR", "Драйвер устройства PnP", "driver"),
    (0xCE, "DRIVER_UNLOADED_WITHOUT_CANCELLING_PENDING_OPERATIONS", "Драйвер", "driver"),
    (0xD1, "DRIVER_IRQL_NOT_LESS_OR_EQUAL", "Драйвер (часто сетевой/видео) или ОЗУ", "driver"),
    (0xD3, "DRIVER_PORTION_MUST_BE_NONPAGED", "Драйвер", "driver"),
    (0xD5, "DRIVER_PAGE_FAULT_IN_FREED_SPECIAL_POOL", "Драйвер", "driver"),
    (0xDA, "SYSTEM_PTE_MISUSE", "Драйвер", "driver"),
    (0xDE, "POOL_CORRUPTION_IN_FILE_AREA", "Драйвер/ОЗУ", "ram"),
    (0xEA, "THREAD_STUCK_IN_DEVICE_DRIVER", "Видеодрайвер/видеокарта", "video"),
    (0xEF, "CRITICAL_PROCESS_DIED", "Системный процесс: диск, повреждение системы", "system"),
    (0xF4, "CRITICAL_OBJECT_TERMINATION", "Диск/контроллер: критический процесс завершён", "disk"),
    (0xF7, "DRIVER_OVERRAN_STACK_BUFFER", "Драйвер", "driver"),
    (0xFC, "ATTEMPTED_EXECUTE_OF_NOEXECUTE_MEMORY", "Драйвер или ОЗУ", "ram"),
    (0xFE, "BUGCODE_USB_DRIVER", "USB-драйвер/устройство", "driver"),
    (0x101, "CLOCK_WATCHDOG_TIMEOUT", "Процессор, перегрев, BIOS", "hw"),
    (0x109, "CRITICAL_STRUCTURE_CORRUPTION", "ОЗУ или драйвер", "ram"),
    (0x10D, "WDF_VIOLATION", "Драйвер", "driver"),
    (0x113, "VIDEO_DXGKRNL_FATAL_ERROR", "Видеодрайвер/видеокарта", "video"),
    (0x116, "VIDEO_TDR_FAILURE", "Видеодрайвер/видеокарта, перегрев", "video"),
    (0x117, "VIDEO_TDR_TIMEOUT_DETECTED", "Видеодрайвер/видеокарта", "video"),
    (0x119, "VIDEO_SCHEDULER_INTERNAL_ERROR", "Видеодрайвер/видеокарта", "video"),
    (0x124, "WHEA_UNCORRECTABLE_ERROR", "Аппаратная ошибка: процессор, ОЗУ, питание, перегрев", "hw"),
    (0x133, "DPC_WATCHDOG_VIOLATION", "Драйвер или диск (SSD), прошивка", "driver"),
    (0x139, "KERNEL_SECURITY_CHECK_FAILURE", "Драйвер или ОЗУ", "ram"),
    (0x13A, "KERNEL_MODE_HEAP_CORRUPTION", "Драйвер или ОЗУ", "ram"),
    (0x14C, "FATAL_ABNORMAL_RESET_ERROR", "Аппаратный сброс: питание, перегрев, плата", "hw"),
    (0x154, "UNEXPECTED_STORE_EXCEPTION", "Диск (SSD) или драйвер хранилища", "disk"),
    (0x15E, "BUGCODE_NDIS_DRIVER_LIVE_DUMP", "Сетевой драйвер", "driver"),
    (0x1CA, "SYNTHETIC_WATCHDOG_TIMEOUT", "Драйвер/зависание, гипервизор", "driver"),
    (0xC000021A, "WINLOGON_FATAL_ERROR", "Повреждение системы или диск", "system"),
    (0xC0000221, "STATUS_IMAGE_CHECKSUM_MISMATCH", "Повреждённый файл: диск", "disk"),
];

/// Снимает флаг варианта 0x10000000 (например 0x1000007E → 0x7E).
pub fn base_code(code: u32) -> u32 {
    if code & 0xF000_0000 == 0x1000_0000 {
        code & 0x0FFF_FFFF
    } else {
        code
    }
}

/// (имя, подсказка, категория)
pub fn describe(code: u32) -> (String, String, String) {
    let base = base_code(code);
    if let Some((_, n, h, c)) = TABLE.iter().find(|(k, _, _, _)| *k == code || *k == base) {
        let suffix = if base != code { " (вариант с флагом)" } else { "" };
        return (format!("{n}{suffix}"), h.to_string(), c.to_string());
    }
    (
        "КОД_НЕ_В_СПРАВОЧНИКЕ".to_string(),
        "Кода нет в справочнике приложения — смотрите документацию Microsoft по Bug Check".to_string(),
        "unknown".to_string(),
    )
}

/// "YYYY-MM-DD HH:MM:SS" → секунды (местное время как есть, нужна лишь разница).
pub fn parse_ts(s: &str) -> Option<i64> {
    let b = s.trim();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| b.get(r)?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

fn compatible(group_code: u32, code: u32) -> bool {
    group_code == 0 || code == 0 || base_code(group_code) == base_code(code)
}

/// Склеивает записи в сбои: записи одного сбоя (дамп, событие WER, Kernel-Power)
/// идут с разницей до нескольких минут и с одним кодом. Возвращает сбои от новых к старым.
pub fn merge_records(mut records: Vec<RawRecord>, window_secs: i64) -> Vec<CrashEntry> {
    records.sort_by(|a, b| a.time.cmp(&b.time));
    struct Group {
        first: String,
        last_ts: i64,
        code: u32,
        sources: BTreeSet<String>,
        params: Vec<String>,
        n: u32,
        dump_time: Option<String>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for r in records {
        let ts = match parse_ts(&r.time) {
            Some(t) => t,
            None => continue,
        };
        let mut placed = false;
        if let Some(g) = groups.last_mut() {
            if ts - g.last_ts <= window_secs && compatible(g.code, r.code) {
                if g.code == 0 {
                    g.code = r.code;
                }
                g.last_ts = ts;
                g.sources.insert(r.source.clone());
                if g.params.is_empty() && !r.params.is_empty() {
                    g.params = r.params.clone();
                }
                if r.source == "minidump" && g.dump_time.is_none() {
                    g.dump_time = Some(r.time.clone());
                }
                g.n += 1;
                placed = true;
            }
        }
        if !placed {
            let mut sources = BTreeSet::new();
            sources.insert(r.source.clone());
            groups.push(Group {
                first: r.time.clone(),
                last_ts: ts,
                code: r.code,
                sources,
                params: r.params.clone(),
                n: 1,
                dump_time: if r.source == "minidump" { Some(r.time.clone()) } else { None },
            });
        }
    }
    let mut out: Vec<CrashEntry> = groups
        .into_iter()
        .map(|g| {
            let (name, hint, category) = describe(g.code);
            CrashEntry {
                // время сбоя — по файлу дампа, если он есть (события пишутся уже после перезагрузки)
                time: g.dump_time.unwrap_or(g.first),
                code: format!("0x{:X}", g.code),
                name,
                hint,
                category,
                sources: g.sources.into_iter().collect(),
                records: g.n,
                params: g.params,
            }
        })
        .collect();
    out.sort_by(|a, b| b.time.cmp(&a.time));
    out
}

fn median(v: &mut Vec<i64>) -> i64 {
    v.sort();
    v[v.len() / 2]
}

/// Диагноз по шаблону сбоев.
pub fn diagnose(entries: &[CrashEntry], hw: &HwCounts) -> Vec<Diagnosis> {
    let mut out: Vec<Diagnosis> = Vec::new();
    let crashes: Vec<&CrashEntry> = entries.iter().filter(|e| e.code != "0x0").collect();
    let power = entries.len() - crashes.len();
    let n = crashes.len();
    let cat = |c: &str| crashes.iter().filter(|e| e.category == c).count();
    let distinct: BTreeSet<u32> = crashes
        .iter()
        .filter_map(|e| u32::from_str_radix(e.code.trim_start_matches("0x"), 16).ok())
        .map(base_code)
        .collect();
    let mut push = |level: &str, title: &str, text: &str, actions: &[&str]| {
        out.push(Diagnosis {
            level: level.into(),
            title: title.into(),
            text: text.into(),
            actions: actions.iter().map(|s| s.to_string()).collect(),
        });
    };

    if n >= 3 && distinct.len() >= 3 {
        push(
            "warn",
            &format!("Разные коды подряд: {} разных кодов за {} сбоев", distinct.len(), n),
            "Такой разброс чаще указывает на ОЗУ, накопитель, питание или перегрев, а не на один драйвер.",
            &["mem", "smart", "stress"],
        );
    }
    if cat("ram") >= 2 {
        push("warn", "Коды, характерные для ОЗУ", "Несколько сбоев с признаками повреждения памяти. Проверьте модули по одному и слоты.", &["mem"]);
    }
    if cat("disk") >= 1 || hw.disk >= 1 {
        push(
            "warn",
            "Признаки проблем накопителя",
            &format!("Сбоев с дисковыми кодами: {}, ошибок диска в журнале: {}. Проверьте SMART, чтение и поверхность.", cat("disk"), hw.disk),
            &["smart", "diskread", "surface"],
        );
    }
    if power >= 2 || (power >= 1 && n >= 1) {
        push(
            "warn",
            &format!("Внезапные отключения без синего экрана: {power}"),
            "Питание (батарея, зарядка/БП), перегрев или плата. Прогоните без батареи и с другим блоком питания, посмотрите температуры под нагрузкой.",
            &["bat", "sens", "stress"],
        );
    }
    if hw.whea >= 1 || cat("hw") >= 1 {
        push(
            "warn",
            "Аппаратные ошибки (WHEA / машинные проверки)",
            &format!("Событий WHEA: {}, сбоев с аппаратными кодами: {}. Процессор, ОЗУ, питание или перегрев.", hw.whea, cat("hw")),
            &["mem", "stress", "sens"],
        );
    }
    if cat("video") >= 2 || hw.tdr >= 2 {
        push("warn", "Сбои видеодрайвера / GPU", "Переустановите видеодрайвер, проверьте температуру и питание видеочипа.", &["stress"]);
    }
    // один и тот же код много раз
    let mut counts: std::collections::BTreeMap<u32, usize> = Default::default();
    for e in &crashes {
        if let Ok(c) = u32::from_str_radix(e.code.trim_start_matches("0x"), 16) {
            *counts.entry(base_code(c)).or_default() += 1;
        }
    }
    if let Some((code, cnt)) = counts.iter().max_by_key(|(_, c)| **c) {
        if *cnt >= 3 {
            let (name, _, _) = describe(*code);
            push(
                "info",
                &format!("Один код повторяется: {name} × {cnt}"),
                "Вероятна причина в конкретном драйвере или модуле — нужен анализ дампа (виновный модуль).",
                &[],
            );
        }
    }
    // регулярный интервал
    let mut times: Vec<i64> = entries.iter().filter_map(|e| parse_ts(&e.time)).collect();
    times.sort();
    if times.len() >= 3 {
        let mut gaps: Vec<i64> = times.windows(2).map(|w| (w[1] - w[0]) / 60).filter(|g| *g > 0).collect();
        if gaps.len() >= 2 {
            let mn = *gaps.iter().min().unwrap();
            let mx = *gaps.iter().max().unwrap();
            let med = median(&mut gaps);
            if (20..=180).contains(&med) && mx <= mn * 3 {
                push(
                    "info",
                    &format!("Сбои идут с интервалом около {med} мин"),
                    "Регулярный интервал характерен для перегрева, питания или энергосбережения (Modern Standby, USB selective suspend), а не для случайной ошибки драйвера.",
                    &["stress", "sens"],
                );
            }
        }
    }
    if entries.len() >= 3 {
        push(
            "info",
            "Как отделить железо от Windows",
            "Загрузитесь с Live USB (WinPE или Linux) и погоняйте нагрузку 30–60 минут: если перезагрузится там — железо (плата, питание, SSD, перегрев); если стабильно — драйверы/прошивка, тогда чистая установка или откат драйверов.",
            &[],
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(time: &str, source: &str, code: u32) -> RawRecord {
        RawRecord { time: time.into(), source: source.into(), code, params: vec![] }
    }

    #[test]
    fn base_code_masks_flag() {
        assert_eq!(base_code(0x1000007E), 0x7E);
        assert_eq!(base_code(0x133), 0x133);
        assert_eq!(base_code(0xC000021A), 0xC000021A);
        assert!(describe(0x1000007E).0.contains("SYSTEM_THREAD_EXCEPTION_NOT_HANDLED"));
        assert_eq!(describe(0x4E).0, "PFN_LIST_CORRUPT");
        assert_eq!(describe(0xDEAD).2, "unknown");
    }

    #[test]
    fn timestamps_diff() {
        let a = parse_ts("2026-09-21 10:45:00").unwrap();
        let b = parse_ts("2026-09-21 10:46:30").unwrap();
        assert_eq!(b - a, 90);
        let c = parse_ts("2026-09-22 10:45:00").unwrap();
        assert_eq!(c - a, 86400);
        assert!(parse_ts("bad").is_none());
    }

    #[test]
    fn three_sources_become_one_crash() {
        let v = vec![
            rec("2026-09-21 10:45:00", "minidump", 0x4E),
            rec("2026-09-21 10:46:10", "bugcheck", 0x4E),
            rec("2026-09-21 10:46:12", "kernel-power", 0x4E),
        ];
        let m = merge_records(v, 120);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].records, 3);
        assert_eq!(m[0].time, "2026-09-21 10:45:00"); // время по дампу
        assert_eq!(m[0].sources.len(), 3);
    }

    #[test]
    fn kernel_power_without_code_joins_bugcheck() {
        let v = vec![rec("2026-09-21 09:25:00", "bugcheck", 0xA), rec("2026-09-21 09:26:00", "kernel-power", 0)];
        let m = merge_records(v, 120);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].code, "0xA");
    }

    #[test]
    fn far_or_different_stay_separate() {
        let v = vec![
            rec("2026-09-21 08:45:00", "minidump", 0x7E),
            rec("2026-09-21 09:25:00", "minidump", 0xA),
            rec("2026-09-21 10:45:00", "minidump", 0x1000007E),
            rec("2026-09-21 11:40:00", "kernel-power", 0),
        ];
        let m = merge_records(v, 120);
        assert_eq!(m.len(), 4);
        assert_eq!(m[0].code, "0x0");
    }

    #[test]
    fn diagnosis_mixed_codes_points_to_hardware() {
        let v = vec![
            rec("2026-09-21 08:45:00", "minidump", 0x7E),
            rec("2026-09-21 09:25:00", "minidump", 0xA),
            rec("2026-09-21 10:45:00", "minidump", 0x4E),
            rec("2026-09-21 11:40:00", "kernel-power", 0),
        ];
        let m = merge_records(v, 120);
        let d = diagnose(&m, &HwCounts::default());
        assert!(d.iter().any(|x| x.title.starts_with("Разные коды подряд")));
        assert!(d.iter().any(|x| x.title.starts_with("Внезапные отключения")));
        assert!(d.iter().any(|x| x.title.starts_with("Как отделить железо")));
        let acts: Vec<&String> = d.iter().flat_map(|x| x.actions.iter()).collect();
        assert!(acts.iter().any(|a| a.as_str() == "mem"));
    }

    #[test]
    fn regular_interval_detected() {
        let v = vec![
            rec("2026-09-21 08:00:00", "minidump", 0x7E),
            rec("2026-09-21 08:50:00", "minidump", 0x7E),
            rec("2026-09-21 09:40:00", "minidump", 0x7E),
            rec("2026-09-21 10:30:00", "minidump", 0x7E),
        ];
        let m = merge_records(v, 120);
        let d = diagnose(&m, &HwCounts::default());
        assert!(d.iter().any(|x| x.title.contains("интервалом около 50")));
        assert!(d.iter().any(|x| x.title.starts_with("Один код повторяется")));
    }

    #[test]
    fn hw_events_raise_flags() {
        let d = diagnose(&[], &HwCounts { whea: 2, disk: 1, tdr: 0 });
        assert!(d.iter().any(|x| x.title.starts_with("Аппаратные ошибки")));
        assert!(d.iter().any(|x| x.title.starts_with("Признаки проблем накопителя")));
    }
}
