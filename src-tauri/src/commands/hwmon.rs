// Датчики LibreHardwareMonitor: вшитый вспомогательный процесс echips-sensors.exe
// (LibreHardwareMonitorLib, MPL-2.0) и установщик драйвера PawnIO (GPL-2.0),
// без которого температуры и мощность процессора недоступны. Приложение только
// для инженеров, поэтому драйвер ставится по кнопке «Установить драйвер» прямо из
// exe (тихо: PawnIO_setup.exe -install -silent) и может быть удалён обратно.
// Вспомогательный процесс печатает JSON-строку раз в секунду; последняя строка
// хранится здесь и используется опросом датчиков (sensors.rs) и стресс-тестом.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Вложения подготавливает build.rs (при локальной сборке — пустые заглушки).
static SENSORS_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sensors.zip"));
static PAWNIO_SETUP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/PawnIO_setup.exe"));

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct HwSensor {
    #[serde(default)]
    pub hw: String,
    #[serde(default)]
    pub hw_type: String,
    #[serde(default)]
    pub name: String,
    /// Идентификатор LibreHardwareMonitor (/lpc/.../fan/0, /control/0)
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub hw_id: String,
    /// У датчика Control можно задать скорость вручную
    #[serde(default)]
    pub controllable: bool,
    #[serde(default, rename = "type")]
    pub sensor_type: String,
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct HwSnapshot {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub sensors: Vec<HwSensor>,
    #[serde(skip_deserializing, default)]
    pub age_ms: u64,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
}

impl Drop for Running {
    fn drop(&mut self) {
        // сначала вернуть вентиляторы в авторежим, потом завершить процесс
        let _ = writeln!(self.stdin, "DEFAULTALL");
        let _ = self.stdin.flush();
        std::thread::sleep(Duration::from_millis(400));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static PROC: Mutex<Option<Running>> = Mutex::new(None);
static LATEST: Mutex<Option<(Instant, HwSnapshot)>> = Mutex::new(None);

fn data_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck").join("hwmon")
}

/// Распаковывает вспомогательный процесс в %LOCALAPPDATA% (один раз на версию сборки).
fn extract_helper() -> Result<std::path::PathBuf, String> {
    if SENSORS_ZIP.is_empty() {
        return Err("Датчики LibreHardwareMonitor не вшиты в эту сборку (локальная сборка без вложений)".to_string());
    }
    let dir = data_dir().join("helper");
    let exe = dir.join("echips-sensors.exe");
    let marker = dir.join("version.txt");
    let tag = SENSORS_ZIP.len().to_string();
    if exe.exists() && std::fs::read_to_string(&marker).map(|s| s.trim() == tag).unwrap_or(false) {
        return Ok(exe);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку {}: {e}", dir.display()))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(SENSORS_ZIP)).map_err(|e| format!("Повреждён вшитый архив датчиков: {e}"))?;
    archive.extract(&dir).map_err(|e| format!("Не удалось распаковать датчики: {e}"))?;
    if !exe.exists() {
        return Err("В архиве датчиков нет echips-sensors.exe".to_string());
    }
    let _ = std::fs::write(&marker, tag);
    Ok(exe)
}

#[cfg(target_os = "windows")]
fn driver_installed() -> bool {
    Command::new("sc")
        .args(["query", "PawnIO"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn driver_installed() -> bool {
    false
}

fn is_running() -> bool {
    match PROC.lock() {
        Ok(mut g) => match g.as_mut() {
            Some(r) => r.child.try_wait().ok().flatten().is_none(),
            None => false,
        },
        Err(_) => false,
    }
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HwmonStatus {
    /// Вспомогательный процесс вшит в сборку
    pub embedded: bool,
    /// Установщик драйвера PawnIO вшит в сборку
    pub driver_embedded: bool,
    pub driver_installed: bool,
    pub running: bool,
    pub has_data: bool,
    pub sensors: usize,
    pub message: String,
}

#[tauri::command(async)]
pub fn hwmon_status() -> HwmonStatus {
    let snap = fresh(6000);
    let mut st = HwmonStatus {
        embedded: !SENSORS_ZIP.is_empty(),
        driver_embedded: !PAWNIO_SETUP.is_empty(),
        driver_installed: driver_installed(),
        running: is_running(),
        has_data: snap.as_ref().map(|s| s.ok).unwrap_or(false),
        sensors: snap.as_ref().map(|s| s.sensors.len()).unwrap_or(0),
        message: String::new(),
    };
    if !st.embedded {
        st.message = "Датчики LibreHardwareMonitor не вшиты в эту сборку".into();
    } else if let Some(s) = &snap {
        if let Some(e) = &s.error {
            st.message = e.clone();
        }
    }
    st
}

#[tauri::command(async)]
pub fn hwmon_start() -> Result<(), String> {
    if is_running() {
        return Ok(());
    }
    let exe = extract_helper()?;
    let mut cmd = Command::new(&exe);
    cmd.arg("1000")
        .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| format!("Не удалось запустить процесс датчиков: {e}"))?;
    let stdin = child.stdin.take().ok_or("Нет stdin у процесса датчиков")?;
    let stdout = child.stdout.take().ok_or("Нет stdout у процесса датчиков")?;
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() {
            if let Ok(snap) = serde_json::from_str::<HwSnapshot>(&line) {
                if let Ok(mut l) = LATEST.lock() {
                    *l = Some((Instant::now(), snap));
                }
            }
        }
    });
    if let Ok(mut g) = PROC.lock() {
        *g = Some(Running { child, stdin });
    }
    Ok(())
}

#[tauri::command(async)]
pub fn hwmon_stop() {
    if let Ok(mut g) = PROC.lock() {
        *g = None; // Drop убивает процесс
    }
    if let Ok(mut l) = LATEST.lock() {
        *l = None;
    }
}

/// Отправляет команду вспомогательному процессу (управление вентиляторами).
fn send(line: &str) -> Result<(), String> {
    let mut g = PROC.lock().map_err(|_| "Внутренняя ошибка блокировки".to_string())?;
    let r = g.as_mut().ok_or("Датчики не запущены — включите датчики (драйвер PawnIO)")?;
    writeln!(r.stdin, "{line}").and_then(|_| r.stdin.flush()).map_err(|e| format!("Не удалось отправить команду датчикам: {e}"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 200 && id.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.#()".contains(c))
}

/// Ручная скорость вентилятора, % (20–100). Через 40 с без обновления
/// процесс сам вернёт авторежим — приложение должно повторять команду.
#[tauri::command(async)]
pub fn hwmon_fan_set(id: String, percent: f64) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Некорректный идентификатор вентилятора".to_string());
    }
    send(&format!("SET|{id}|{:.0}", percent.clamp(20.0, 100.0)))
}

#[tauri::command(async)]
pub fn hwmon_fan_default(id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Некорректный идентификатор вентилятора".to_string());
    }
    send(&format!("DEFAULT|{id}"))
}

/// Вернуть все вентиляторы в автоматический режим (ошибка «датчики не запущены» игнорируется).
#[tauri::command(async)]
pub fn hwmon_fan_default_all() {
    let _ = send("DEFAULTALL");
}

/// Последний снимок показаний, если он не старше `max_age_ms`.
fn fresh(max_age_ms: u64) -> Option<HwSnapshot> {
    let g = LATEST.lock().ok()?;
    let (at, snap) = g.as_ref()?;
    let age = at.elapsed().as_millis() as u64;
    if age > max_age_ms {
        return None;
    }
    let mut s = snap.clone();
    s.age_ms = age;
    Some(s)
}

#[tauri::command(async)]
pub fn hwmon_snapshot() -> Option<HwSnapshot> {
    fresh(6000)
}

fn run_setup(args: &[&str]) -> Result<String, String> {
    if PAWNIO_SETUP.is_empty() {
        return Err("Установщик драйвера PawnIO не вшит в эту сборку".to_string());
    }
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("PawnIO_setup.exe");
    std::fs::write(&path, PAWNIO_SETUP).map_err(|e| format!("Не удалось записать установщик: {e}"))?;
    let mut cmd = Command::new(&path);
    cmd.args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| format!("Не удалось запустить установщик PawnIO: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if started.elapsed() > Duration::from_secs(180) {
                    let _ = child.kill();
                    return Err("Установщик PawnIO не завершился за 3 минуты".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Ошибка ожидания установщика: {e}")),
        }
    };
    let _ = std::fs::remove_file(&path);
    match status.code() {
        Some(0) => Ok("готово".to_string()),
        Some(3010) => Ok("готово, но для завершения нужна перезагрузка Windows".to_string()),
        Some(c) => Err(format!("Установщик PawnIO вернул код {c}")),
        None => Err("Установщик PawnIO завершён аварийно".to_string()),
    }
}

/// Тихая установка драйвера PawnIO (нужны права администратора — они есть).
#[tauri::command(async)]
pub fn hwmon_install_driver() -> Result<String, String> {
    hwmon_stop();
    let r = run_setup(&["-install", "-silent"])?;
    Ok(format!("Драйвер PawnIO установлен: {r}"))
}

/// Удаление драйвера PawnIO.
#[tauri::command(async)]
pub fn hwmon_uninstall_driver() -> Result<String, String> {
    hwmon_stop();
    let r = run_setup(&["-uninstall", "-silent"])?;
    Ok(format!("Драйвер PawnIO удалён: {r}"))
}

// ------------------------------------------------------------------
//  Сводка для опроса датчиков и стресс-теста
// ------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct HwSummary {
    pub cpu_temp: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub cpu_power_w: Option<f64>,
    /// Средняя реальная частота ядер (LibreHardwareMonitor, «Core #N»), МГц — в отличие от
    /// CallNtPowerInformation, показывает турбо и троттлинг, а не номинал.
    pub cpu_clock_mhz: Option<f64>,
}

/// Ключевые показания из последнего снимка: температура процессора (Package /
/// Tctl/Tdie, иначе максимум по ядрам), температура GPU, максимум оборотов
/// вентиляторов, мощность CPU Package.
pub fn summary() -> Option<HwSummary> {
    let snap = fresh(6000)?;
    if !snap.ok {
        return None;
    }
    let pick = |hw_prefix: &str, ty: &str, prefer: &[&str]| -> Option<f64> {
        let list: Vec<&HwSensor> = snap.sensors.iter().filter(|s| s.hw_type.starts_with(hw_prefix) && s.sensor_type == ty).collect();
        for p in prefer {
            if let Some(s) = list.iter().find(|s| s.name.to_lowercase().contains(&p.to_lowercase())) {
                return Some(s.value);
            }
        }
        list.iter().map(|s| s.value).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.max(v))))
    };
    let cpu_temp = pick("Cpu", "Temperature", &["Package", "Tctl", "Tdie", "Core Max"]).filter(|t| *t > 0.0 && *t < 150.0);
    let gpu_temp = pick("Gpu", "Temperature", &["GPU Core", "Core"]).filter(|t| *t > 0.0 && *t < 150.0);
    let fan_rpm = snap.sensors.iter().filter(|s| s.sensor_type == "Fan" && s.value > 0.0).map(|s| s.value).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.max(v))));
    let cpu_power_w = pick("Cpu", "Power", &["Package"]);
    let cores: Vec<f64> = snap
        .sensors
        .iter()
        .filter(|s| s.hw_type.starts_with("Cpu") && s.sensor_type == "Clock" && s.name.contains("Core #") && !s.name.contains("Effective") && s.value > 0.0)
        .map(|s| s.value)
        .collect();
    let cpu_clock_mhz = if cores.is_empty() { None } else { Some(cores.iter().sum::<f64>() / cores.len() as f64) };
    Some(HwSummary { cpu_temp, gpu_temp, fan_rpm, cpu_power_w, cpu_clock_mhz })
}
