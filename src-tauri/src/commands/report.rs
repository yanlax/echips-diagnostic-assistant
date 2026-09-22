// Сохранение итогового отчёта диагностики — .txt (для акта), .json (для
// базы) и .pdf (для отправки клиенту/заказчику).

use printpdf::{
    Color, IndirectFontRef, Mm, PdfDocument, PdfDocumentReference, PdfLayerReference, Rgb,
};
use serde::{Deserialize, Serialize};
use std::fs;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestResult {
    pub id: String,
    pub title: String,
    /// "pass" | "fail" | "idle"
    pub status: String,
    pub comment: Option<String>,
    /// Автоматическая оценка теста ("pass" | "fail" | "na") и её пояснение
    #[serde(default)]
    pub auto_status: Option<String>,
    #[serde(default)]
    pub auto_note: Option<String>,
    /// Причина, если техник изменил автоматический вердикт
    #[serde(default)]
    pub override_reason: Option<String>,
    /// Подробности теста (строки лога, измерения)
    #[serde(default)]
    pub details: Vec<String>,
    /// Входит ли тест в профиль модели (вне профиля — не «не проверено»)
    #[serde(default)]
    pub in_profile: Option<bool>,
    #[serde(default)]
    pub finished_at: Option<String>,
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
        "na" => "НЕ ПРИМЕНИМО",
        _ => "НЕ ПРОВЕРЕНО",
    }
}

fn render_txt(report: &DiagnosticReport) -> String {
    let mut out = String::new();
    out.push_str("ECHIPS HARDWARE CHECK — ОТЧЁТ ДИАГНОСТИКИ\n");
    out.push_str("===========================================\n\n");
    out.push_str(&format!("Устройство: {}\n", report.device_model));
    out.push_str(&format!("Серийный номер: {}\n", report.device_serial));
    out.push_str(&format!("Инженер: {}\n", report.engineer));
    out.push_str(&format!("Начало: {}\n", report.started_at));
    out.push_str(&format!("Окончание: {}\n\n", report.finished_at));
    out.push_str("Результаты проверок:\n---------------------\n");
    for r in &report.results {
        let label = if r.status == "idle" && r.in_profile == Some(false) { "ВНЕ ПРОФИЛЯ" } else { status_label(&r.status) };
        out.push_str(&format!("[{}] {}", label, r.title));
        if let Some(c) = &r.comment {
            if !c.trim().is_empty() {
                out.push_str(&format!(" — {c}"));
            }
        }
        out.push('\n');
        if let (Some(st), Some(note)) = (&r.auto_status, &r.auto_note) {
            if let Some(reason) = &r.override_reason {
                out.push_str(&format!("    Автооценка: {st} — {note}; изменено техником: {reason}\n"));
            }
        }
        for line in &r.details {
            out.push_str(&format!("    {line}\n"));
        }
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

/* ---------- PDF ----------
   Шрифт PT Sans (OFL, кириллица) вшит из src-tauri/assets/fonts. Ширина
   символов не измеряется через метрики шрифта (printpdf 0.7 их наружу не
   отдаёт) — перенос строк по эвристике "средний символ ~0.52 кегля", этого
   достаточно для служебного отчёта и не считается точной вёрсткой. */

const PDF_PAGE_W: f32 = 210.0; // A4, мм
const PDF_PAGE_H: f32 = 297.0;
const PDF_MARGIN_L: f32 = 18.0;
const PDF_MARGIN_R: f32 = 18.0;
const PDF_MARGIN_TOP: f32 = 20.0;
const PDF_MARGIN_BOTTOM: f32 = 18.0;
const PDF_CONTENT_W: f32 = PDF_PAGE_W - PDF_MARGIN_L - PDF_MARGIN_R;

const FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/PTSans-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../assets/fonts/PTSans-Bold.ttf");

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

/// Сколько символов помещается в ширину `PDF_CONTENT_W - indent_mm` при кегле `size_pt`.
fn max_chars(size_pt: f32, indent_mm: f32) -> usize {
    let avg_char_w_mm = pt_to_mm(size_pt) * 0.52;
    let usable = (PDF_CONTENT_W - indent_mm).max(20.0);
    ((usable / avg_char_w_mm).floor() as usize).max(10)
}

/// Простой перенос по словам; слово длиннее строки режется жёстко.
fn wrap_line(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut cur = String::new();
        for word in raw_line.split(' ') {
            let mut rest: String = word.to_string();
            loop {
                let extra = if cur.is_empty() { 0 } else { 1 };
                if cur.chars().count() + extra + rest.chars().count() <= max {
                    if !cur.is_empty() {
                        cur.push(' ');
                    }
                    cur.push_str(&rest);
                    break;
                }
                if cur.is_empty() && rest.chars().count() > max {
                    // Слово само по себе длиннее строки — режем жёстко.
                    let chars: Vec<char> = rest.chars().collect();
                    out.push(chars[..max].iter().collect());
                    rest = chars[max..].iter().collect();
                    continue;
                }
                out.push(std::mem::take(&mut cur));
            }
        }
        out.push(cur);
    }
    out
}

struct PdfWriter {
    layer: PdfLayerReference,
    y: f32,
    font_regular: IndirectFontRef,
    font_bold: IndirectFontRef,
}

impl PdfWriter {
    fn new_page(doc: &PdfDocumentReference) -> (PdfLayerReference, f32) {
        let (page, layer) = doc.add_page(Mm(PDF_PAGE_W), Mm(PDF_PAGE_H), "Layer");
        (doc.get_page(page).get_layer(layer), PDF_PAGE_H - PDF_MARGIN_TOP)
    }

    fn ensure_space(&mut self, doc: &PdfDocumentReference, needed_mm: f32) {
        if self.y - needed_mm < PDF_MARGIN_BOTTOM {
            let (layer, y) = Self::new_page(doc);
            self.layer = layer;
            self.y = y;
        }
    }

    /// Пишет текст с переносом по словам, возвращает индент для след. блока не меняет.
    fn text(
        &mut self,
        doc: &PdfDocumentReference,
        text: &str,
        size_pt: f32,
        bold: bool,
        color: Rgb,
        indent_mm: f32,
    ) {
        let font = if bold { self.font_bold.clone() } else { self.font_regular.clone() };
        let line_h = pt_to_mm(size_pt) * 1.35;
        for line in wrap_line(text, max_chars(size_pt, indent_mm)) {
            self.ensure_space(doc, line_h);
            self.layer.set_fill_color(Color::Rgb(color.clone()));
            self.layer
                .use_text(line, size_pt, Mm(PDF_MARGIN_L + indent_mm), Mm(self.y), &font);
            self.y -= line_h;
        }
    }

    fn gap(&mut self, mm: f32) {
        self.y -= mm;
    }
}

fn render_pdf(report: &DiagnosticReport) -> Result<Vec<u8>, String> {
    let (doc, page1, layer1) =
        PdfDocument::new("Echips Hardware Check — отчёт", Mm(PDF_PAGE_W), Mm(PDF_PAGE_H), "Layer");
    let font_regular = doc
        .add_external_font(FONT_REGULAR)
        .map_err(|e| format!("Не удалось встроить шрифт: {e}"))?;
    let font_bold = doc
        .add_external_font(FONT_BOLD)
        .map_err(|e| format!("Не удалось встроить шрифт: {e}"))?;
    let layer = doc.get_page(page1).get_layer(layer1);

    let black = Rgb::new(0.12, 0.12, 0.12, None);
    let gray = Rgb::new(0.42, 0.42, 0.42, None);
    let red = Rgb::new(0.72, 0.14, 0.14, None);
    let green = Rgb::new(0.11, 0.45, 0.2, None);

    let mut w = PdfWriter {
        layer,
        y: PDF_PAGE_H - PDF_MARGIN_TOP,
        font_regular,
        font_bold,
    };

    w.text(&doc, "ECHIPS HARDWARE CHECK", 18.0, true, black.clone(), 0.0);
    w.text(&doc, "Отчёт диагностики", 12.0, false, gray.clone(), 0.0);
    w.gap(4.0);
    w.text(&doc, &format!("Устройство: {}", report.device_model), 10.5, false, black.clone(), 0.0);
    w.text(&doc, &format!("Серийный номер: {}", report.device_serial), 10.5, false, black.clone(), 0.0);
    if !report.engineer.trim().is_empty() {
        w.text(&doc, &format!("Инженер: {}", report.engineer), 10.5, false, black.clone(), 0.0);
    }
    w.text(&doc, &format!("Начало: {}", report.started_at), 10.5, false, black.clone(), 0.0);
    w.text(&doc, &format!("Окончание: {}", report.finished_at), 10.5, false, black.clone(), 0.0);
    w.gap(6.0);

    w.text(&doc, "Результаты проверок", 13.0, true, black.clone(), 0.0);
    w.gap(2.0);
    for r in &report.results {
        let label = if r.status == "idle" && r.in_profile == Some(false) {
            "ВНЕ ПРОФИЛЯ"
        } else {
            status_label(&r.status)
        };
        let color = match r.status.as_str() {
            "pass" => green.clone(),
            "fail" => red.clone(),
            _ => black.clone(),
        };
        let mut head = format!("[{}] {}", label, r.title);
        if let Some(c) = &r.comment {
            if !c.trim().is_empty() {
                head.push_str(&format!(" — {c}"));
            }
        }
        w.gap(1.5);
        w.text(&doc, &head, 10.5, true, color, 0.0);
        if let (Some(st), Some(note)) = (&r.auto_status, &r.auto_note) {
            if let Some(reason) = &r.override_reason {
                w.text(
                    &doc,
                    &format!("Автооценка: {st} — {note}; изменено техником: {reason}"),
                    9.0,
                    false,
                    gray.clone(),
                    5.0,
                );
            }
        }
        for line in &r.details {
            w.text(&doc, line.trim_start(), 9.0, false, gray.clone(), 5.0);
        }
    }

    let failed: Vec<&TestResult> = report.results.iter().filter(|r| r.status == "fail").collect();
    w.gap(6.0);
    w.text(&doc, "Итог", 13.0, true, black.clone(), 0.0);
    w.gap(2.0);
    if failed.is_empty() {
        w.text(&doc, "Неисправностей не выявлено.", 10.5, false, green, 0.0);
    } else {
        w.text(&doc, &format!("Выявлено неисправностей: {}.", failed.len()), 10.5, true, red.clone(), 0.0);
        for f in failed {
            w.text(&doc, &format!("• {}", f.title), 10.0, false, red.clone(), 5.0);
        }
    }

    doc.save_to_bytes().map_err(|e| format!("Не удалось сформировать PDF: {e}"))
}

fn safe_filename_part(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if cleaned.is_empty() {
        "device".to_string()
    } else {
        cleaned
    }
}

fn reports_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Не удалось определить папку данных приложения: {e}"))?.join("reports");
    fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку отчётов: {e}"))?;
    Ok(dir)
}

#[tauri::command(async)]
pub fn save_report_txt(app: tauri::AppHandle, report: DiagnosticReport) -> Result<String, String> {
    let dir = reports_dir(&app)?;
    let filename = format!(
        "{}_{}.txt",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        safe_filename_part(&report.device_serial)
    );
    let path = dir.join(&filename);
    fs::write(&path, render_txt(&report)).map_err(|e| format!("Не удалось записать отчёт: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(async)]
pub fn save_report_json(app: tauri::AppHandle, report: DiagnosticReport) -> Result<String, String> {
    let dir = reports_dir(&app)?;
    let filename = format!(
        "{}_{}.json",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        safe_filename_part(&report.device_serial)
    );
    let path = dir.join(&filename);
    let json = serde_json::to_string_pretty(&report).map_err(|e| format!("Не удалось сериализовать отчёт: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Не удалось записать отчёт: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(async)]
pub fn save_report_pdf(app: tauri::AppHandle, report: DiagnosticReport) -> Result<String, String> {
    let dir = reports_dir(&app)?;
    let filename = format!(
        "{}_{}.pdf",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        safe_filename_part(&report.device_serial)
    );
    let path = dir.join(&filename);
    let bytes = render_pdf(&report)?;
    fs::write(&path, bytes).map_err(|e| format!("Не удалось записать отчёт: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// Открывает Проводник с уже подсвеченным файлом отчёта (после экспорта
/// TXT/JSON/PDF) — тех. на 3 рабочей ссылке было бы неудобно каждый раз
/// вручную идти в `%LOCALAPPDATA%\Echips\HardwareCheck\reports`.
#[tauri::command(async)]
pub fn open_containing_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("Доступно только в Windows-сборке".to_string())
    }
}
