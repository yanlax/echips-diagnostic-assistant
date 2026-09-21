// Установка драйверов — порт реальной логики из echips-driver-assistant
// (main.rs) на общий powershell.rs-хелпер этого проекта. Каскад
// определения пакета (по имени модели → по префиксу серийника → ручной
// выбор) и сам механизм скачивания/установки идентичны оригиналу.

use crate::powershell::{run_ps, run_ps_with_status};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::Command;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{Emitter, Window};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const MIN_FREE_MB: u64 = 500;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ManifestEntry {
    pub yandex_public_key: String,
    pub path: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DownloadProgress {
    pub stage: String,
    pub downloaded: u64,
    pub total: u64,
    pub file_label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstallResult {
    pub success: bool,
    pub message: String,
    pub installed_drivers: Vec<String>,
    pub log_path: String,
}

fn normalize_code(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase()
}

#[tauri::command(async)]
pub fn find_by_name(manifest: serde_json::Value, manufacturer: String, model: String) -> Option<(String, ManifestEntry)> {
    let haystack = format!("{manufacturer} {model}").to_lowercase();
    let obj = manifest.as_object()?;
    for (key, value) in obj {
        if key.starts_with('_') {
            continue;
        }
        let key_lower = key.to_lowercase();
        if haystack.contains(&key_lower) || key_lower.contains(&haystack) {
            if let Ok(entry) = serde_json::from_value::<ManifestEntry>(value.clone()) {
                return Some((key.clone(), entry));
            }
        }
    }
    None
}

#[tauri::command(async)]
pub fn find_by_serial_prefix(manifest: serde_json::Value, serial: String) -> Option<(String, ManifestEntry)> {
    let norm_serial = normalize_code(&serial);
    if norm_serial.is_empty() {
        return None;
    }
    let obj = manifest.as_object()?;
    let mut best: Option<(String, ManifestEntry, usize)> = None;
    for (key, value) in obj {
        if key.starts_with('_') {
            continue;
        }
        let norm_key = normalize_code(key);
        if !norm_key.is_empty()
            && norm_serial.starts_with(&norm_key)
            && norm_key.len() > best.as_ref().map(|b| b.2).unwrap_or(0)
        {
            if let Ok(entry) = serde_json::from_value::<ManifestEntry>(value.clone()) {
                best = Some((key.clone(), entry, norm_key.len()));
            }
        }
    }
    best.map(|(k, e, _)| (k, e))
}

#[tauri::command]
pub async fn fetch_public_json(public_url: String) -> Result<serde_json::Value, String> {
    let href = yandex_get_download_href(&public_url, None).await.map_err(|e| e.to_string())?;
    let resp = reqwest::get(&href).await.map_err(|e| e.to_string())?;
    resp.json::<serde_json::Value>().await.map_err(|e| e.to_string())
}

async fn yandex_get_download_href(public_key: &str, path: Option<&str>) -> Result<String, reqwest::Error> {
    let mut url = format!(
        "https://cloud-api.yandex.net/v1/disk/public/resources/download?public_key={}",
        urlencoding::encode(public_key)
    );
    if let Some(p) = path {
        url.push_str(&format!("&path={}", urlencoding::encode(p)));
    }
    let resp = reqwest::get(&url).await?;
    let json: serde_json::Value = resp.json().await?;
    Ok(json["href"].as_str().unwrap_or_default().to_string())
}

#[tauri::command]
pub async fn yandex_list_folder(public_key: String) -> Result<Vec<(String, String)>, String> {
    let url = format!(
        "https://cloud-api.yandex.net/v1/disk/public/resources?public_key={}&path=/&limit=200",
        urlencoding::encode(&public_key)
    );
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let items = json["_embedded"]["items"].as_array().cloned().unwrap_or_default();
    let mut result = vec![];
    for item in items {
        if item["type"].as_str() == Some("file") {
            let name = item["name"].as_str().unwrap_or_default().to_string();
            let path = item["path"].as_str().unwrap_or_default().to_string();
            result.push((name, path));
        }
    }
    Ok(result)
}

fn app_data_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Echips").join("HardwareCheck")
}

fn manifest_cache_path() -> std::path::PathBuf {
    app_data_dir().join("manifest_cache.json")
}

#[tauri::command(async)]
pub fn cache_manifest(manifest: serde_json::Value) -> Result<(), String> {
    let path = manifest_cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, manifest.to_string()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn load_cached_manifest() -> Result<serde_json::Value, String> {
    let path = manifest_cache_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|_| "Сохранённая копия каталога драйверов не найдена.".to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn create_restore_point() -> Result<(), String> {
    let (stdout, ok) = run_ps_with_status(
        "Checkpoint-Computer -Description 'Echips Hardware Check' -RestorePointType 'DEVICE_DRIVER_INSTALL'",
    );
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Не удалось создать точку восстановления (возможно, защита системы отключена \
            или точка уже создавалась в последние 24 часа). {stdout}"
        ))
    }
}

fn free_space_mb() -> u64 {
    run_ps("(Get-PSDrive -Name ((Get-Item $env:TEMP).PSDrive.Name)).Free")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|b| b / (1024 * 1024))
        .unwrap_or(u64::MAX)
}

fn log_dir() -> std::path::PathBuf {
    app_data_dir().join("logs")
}

fn unix_time() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write_log(lines: &[String]) -> String {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("install_{}.log", unix_time()));
    let _ = std::fs::write(&path, lines.join("\n"));
    path.to_string_lossy().to_string()
}

fn parse_installed_drivers(stdout: &str) -> Vec<String> {
    let mut result = vec![];
    for line in stdout.lines() {
        let lower = line.to_lowercase();
        if let Some(pos) = lower.find("oem") {
            let tail = &line[pos..];
            if let Some(end) = tail.to_lowercase().find(".inf") {
                let token = tail[..end + 4].to_string();
                if !result.contains(&token) {
                    result.push(token);
                }
            }
        }
    }
    result
}

#[tauri::command]
pub async fn download_and_install(
    window: Window,
    files: Vec<(String, Option<String>, String)>,
    create_restore: bool,
) -> Result<InstallResult, String> {
    let mut log_lines: Vec<String> = vec![format!(
        "[{}] Начало установки. Файлов к загрузке: {}",
        unix_time(),
        files.len()
    )];

    let free_mb = free_space_mb();
    log_lines.push(format!("[{}] Свободно места на диске: {free_mb} МБ", unix_time()));
    if free_mb < MIN_FREE_MB {
        log_lines.push("Недостаточно места — установка прервана до начала загрузки.".into());
        let log_path = write_log(&log_lines);
        return Err(format!(
            "Недостаточно свободного места на диске (доступно {free_mb} МБ, требуется не менее {MIN_FREE_MB} МБ).\n\nЛог: {log_path}"
        ));
    }

    if create_restore {
        let _ = window.emit(
            "install-progress",
            DownloadProgress { stage: "restore_point".into(), downloaded: 0, total: 0, file_label: "Создание точки восстановления...".into() },
        );
        match create_restore_point() {
            Ok(()) => log_lines.push(format!("[{}] Точка восстановления создана успешно.", unix_time())),
            Err(e) => log_lines.push(format!("[{}] Точка восстановления НЕ создана: {e}", unix_time())),
        }
    }

    let tmp_dir = std::env::temp_dir().join(format!("echips_hwcheck_drivers_{}", std::process::id()));
    let extract_dir = tmp_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;

    let total_files = files.len();
    for (i, (public_key, path, label)) in files.iter().enumerate() {
        log_lines.push(format!("[{}] Скачивание: {label}", unix_time()));
        let href = yandex_get_download_href(public_key, path.as_deref()).await.map_err(|e| e.to_string())?;

        let zip_path = tmp_dir.join(format!("part_{i}.zip"));
        download_file(&window, &href, &zip_path, label, i + 1, total_files).await.map_err(|e| e.to_string())?;

        extract_zip(&zip_path, &extract_dir).map_err(|e| e.to_string())?;
        log_lines.push(format!("[{}] Распаковано: {label}", unix_time()));
    }

    let _ = window.emit(
        "install-progress",
        DownloadProgress { stage: "installing".into(), downloaded: 0, total: 0, file_label: "Установка драйверов...".into() },
    );

    let inf_glob = extract_dir.join("*.inf");
    let inf_glob_str = inf_glob.to_string_lossy().replace('\'', "''");
    let pnputil_command = format!("pnputil /add-driver '{inf_glob_str}' /subdirs /install; exit $LASTEXITCODE");

    let (stdout, success) = run_ps_with_status(&pnputil_command);
    let _ = std::fs::remove_dir_all(&tmp_dir);

    let installed_drivers = parse_installed_drivers(&stdout);
    log_lines.push(format!("[{}] ---- Вывод pnputil ----", unix_time()));
    log_lines.push(stdout.clone());
    log_lines.push(format!("[{}] Установлено пакетов: {}", unix_time(), installed_drivers.len()));

    let log_path = write_log(&log_lines);

    let processed_something = stdout.contains("Опубликовано")
        || stdout.contains("Published")
        || stdout.contains("уже присутствует")
        || stdout.contains("already present")
        || stdout.contains("успешно добавлен")
        || stdout.contains("was successfully");

    if success {
        Ok(InstallResult {
            success: true,
            message: "Драйверы успешно установлены. Рекомендуется перезагрузить компьютер.".into(),
            installed_drivers,
            log_path,
        })
    } else if processed_something {
        Ok(InstallResult {
            success: true,
            message: "Часть драйверов установлена успешно. Некоторые пакеты могли быть пропущены \
                (например, неподписанные или несовместимые с этой моделью) — рекомендуем \
                перезагрузить компьютер и проверить Диспетчер устройств."
                .into(),
            installed_drivers,
            log_path,
        })
    } else {
        Err(format!("Установка завершилась с ошибками. Обратитесь в поддержку Echips.\n\nЛог: {log_path}"))
    }
}

async fn download_file(
    window: &Window,
    url: &str,
    dest: &std::path::Path,
    label: &str,
    file_index: usize,
    total_files: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;

    let resp = reqwest::get(url).await?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(dest)?;
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();

    let display_label = if total_files > 1 { format!("{label} ({file_index}/{total_files})") } else { label.to_string() };

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        let _ = window.emit(
            "install-progress",
            DownloadProgress { stage: "downloading".into(), downloaded, total, file_label: display_label.clone() },
        );
    }
    let _ = window.emit("file-progress", serde_json::json!({ "index": file_index - 1, "status": "done" }));
    Ok(())
}

fn extract_zip(zip_path: &std::path::Path, extract_to: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    archive.extract(extract_to)?;
    Ok(())
}

#[tauri::command(async)]
pub fn restart_system() {
    let mut cmd = Command::new("shutdown");
    cmd.args(["/r", "/t", "5"]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let _ = cmd.spawn();
}

#[tauri::command(async)]
pub fn open_log_folder() -> Result<(), String> {
    let dir = log_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку с логами ({}): {e}", dir.display()))?;
    Command::new("explorer").arg(dir.to_string_lossy().to_string()).spawn().map_err(|e| e.to_string())?;
    Ok(())
}
