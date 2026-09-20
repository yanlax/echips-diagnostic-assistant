// Сохранение итогового отчёта диагностики на диск (txt) — в паре с текущим
// repair-doc воркфлоу: выявленная неисправность / выполненные работы / результат.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestResult {
    pub id: String,
    pub title: String,
    /// "pass" | "fail" | "skipped" | "not_run"
    pub status: String,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiagnosticReport {
    pub device_model: String,
    pub device_serial: String,
    pub engineer: String,
    pub started_at: String,
    pub finished_at: String,
    pub results: Vec<TestResult>,
}

fn status_label(status: &str) -> &'static str {
    match status {
        "pass" => "OK",
        "fail" => "НЕИСПРАВНО",
        "skipped" => "ПРОПУЩЕНО",
        _ => "НЕ ПРОВЕРЕНО",
    }
}

fn render_report(report: &DiagnosticReport) -> String {
    let mut out = String::new();
    out.push_str("ECHIPS — ОТЧЁТ ДИАГНОСТИКИ\n");
    out.push_str("==========================\n\n");
    out.push_str(&format!("Устройство: {}\n", report.device_model));
    out.push_str(&format!("Серийный номер: {}\n", report.device_serial));
    out.push_str(&format!("Инженер: {}\n", report.engineer));
    out.push_str(&format!("Начало: {}\n", report.started_at));
    out.push_str(&format!("Окончание: {}\n\n", report.finished_at));
    out.push_str("Результаты проверок:\n");
    out.push_str("---------------------\n");
    for r in &report.results {
        out.push_str(&format!("[{}] {}", status_label(&r.status), r.title));
        if let Some(note) = &r.note {
            if !note.trim().is_empty() {
                out.push_str(&format!(" — {}", note));
            }
        }
        out.push('\n');
    }

    let failed: Vec<&TestResult> = report.results.iter().filter(|r| r.status == "fail").collect();
    out.push_str("\n---------------------\n");
    if failed.is_empty() {
        out.push_str("Итог: неисправностей не выявлено.\n");
    } else {
        out.push_str(&format!("Итог: выявлено неисправностей — {}.\n", failed.len()));
        for f in failed {
            out.push_str(&format!("  • {}\n", f.title));
        }
    }
    out
}

#[tauri::command]
pub fn save_report(app: tauri::AppHandle, report: DiagnosticReport) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Не удалось определить папку данных приложения: {e}"))?
        .join("reports");

    fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку отчётов: {e}"))?;

    let safe_serial = report
        .device_serial
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let filename = format!(
        "{}_{}.txt",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        if safe_serial.is_empty() { "device".to_string() } else { safe_serial }
    );
    let path = dir.join(&filename);

    let mut file = fs::File::create(&path).map_err(|e| format!("Не удалось создать файл отчёта: {e}"))?;
    file.write_all(render_report(&report).as_bytes())
        .map_err(|e| format!("Не удалось записать отчёт: {e}"))?;

    Ok(path.to_string_lossy().to_string())
}
