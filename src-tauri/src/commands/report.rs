// Сохранение итогового отчёта диагностики — .txt (для акта), .json (для
// базы) и .pdf (для отправки клиенту/заказчику).

use printpdf::path::{PaintMode, WindingOrder};
use printpdf::{
    Color, IndirectFontRef, Image, ImageTransform, Line, Mm, PdfDocument, PdfDocumentReference,
    PdfLayerReference, Point, Polygon, Rgb,
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
    /// Общий комментарий инженера по итогам диагностики (не привязан к
    /// конкретному тесту) — заполняется на экране «Отчёт», попадает во
    /// все три формата экспорта.
    #[serde(default)]
    pub summary_comment: String,
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
    out.push_str(&format!("Окончание: {}\n", report.finished_at));
    if !report.summary_comment.trim().is_empty() {
        out.push_str(&format!("\nКомментарий инженера: {}\n", report.summary_comment.trim()));
    }
    out.push_str("\nРезультаты проверок:\n---------------------\n");
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
   Переверстано под референс-дизайн, присланный пользователем (карточки
   тестов вместо построчного текста) — светлая «бумага» с тёмной обложкой
   сверху вместо прежнего полностью тёмного фона (v0.10.0). Шрифт PT Sans
   (OFL, кириллица) вшит из src-tauri/assets/fonts. Ширина символов не
   измеряется через метрики шрифта (printpdf 0.7 их наружу не отдаёт) —
   перенос строк по эвристике "средний символ ~0.52 кегля", этого
   достаточно для служебного отчёта и не считается точной вёрсткой.

   Углы карточек/бейджей/пилюль/чипов скруглены по-настоящему — через
   кубические кривые Безье, которые printpdf 0.7 честно поддерживает в
   Line/Polygon (см. rounded_rect_points) — а не прямые, как в первой
   версии переверстки (главная причина, по которой она "выглядела не
   так, как шаблон").

   Строки `details` — плоский Vec<String> уже готовых строк из app.js
   (recordDetail), а не структурированные пары ключ-значение, поэтому
   настоящую 2-колоночную kv-сетку и таблицу журнала (зебра-строки,
   отдельная колонка времени) без изменения модели данных во всех
   тестах не сделать — это осталось как есть, одной колонкой текста.
   Но списки вида "имя — статус" (USB/Bluetooth/Wi-Fi-сети и т.п. —
   самый частый формат в recordDetail по всему app.js) распознаются
   эвристикой `classify_detail()` и рисуются как в референсе: имя
   слева, значение справа, тонкий разделитель под строкой (см.
   draw_row) — без единой правки на стороне JS.

   Упрощение, оставленное сознательно: карточка не имеет заливки фона
   (только рамка + цветная полоса слева) — страница и так белая, доп.
   заливка не нужна и не мешает переносу карточки между страницами
   (см. draw_card). Таблица журнала сбоев (зебра-строки с отдельной
   мело-колонкой времени) и плашка диагноза (жёлтый alert-бокс) —
   тоже одной колонкой текста, не выделены отдельным стилем. */

const PDF_PAGE_W: f32 = 210.0; // A4, мм
const PDF_PAGE_H: f32 = 297.0;
const PDF_MARGIN_L: f32 = 18.0;
const PDF_MARGIN_R: f32 = 18.0;
const PDF_MARGIN_TOP: f32 = 20.0;
const PDF_MARGIN_BOTTOM: f32 = 18.0;
const PDF_CONTENT_W: f32 = PDF_PAGE_W - PDF_MARGIN_L - PDF_MARGIN_R;
/// Отступ содержимого карточки от общей рамки (цветная полоса + зазор).
const CARD_INDENT: f32 = 6.0;
/// Отступ содержимого карточки от правого края (место под рамку).
const CARD_RIGHT_PAD: f32 = 4.0;
/// Зазор между карточками.
const CARD_GAP: f32 = 3.0;
/// Радиусы скругления — приблизительно как --radius/--radius-sm в референсе
/// (14px/9px при печатном разрешении ≈ 3.7/2.4мм; взято чуть меньше, чтобы
/// с толщиной обводки в 0.5-0.7pt не появлялось видимых артефактов на
/// небольших карточках).
const RADIUS_BOX: f32 = 3.2;
const RADIUS_BOX_SM: f32 = 2.4;

const FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/PTSans-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../assets/fonts/PTSans-Bold.ttf");
const LOGO_PNG: &[u8] = include_bytes!("../../assets/logo.png");
const LOGO_PX_W: f32 = 194.0;
const LOGO_PX_H: f32 = 256.0;

fn draw_rule(layer: &PdfLayerReference, color: Rgb, x0: f32, x1: f32, y0: f32, y1: f32) {
    layer.set_fill_color(Color::Rgb(color));
    let poly = Polygon {
        rings: vec![vec![
            (Point::new(Mm(x0), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y1)), false),
            (Point::new(Mm(x0), Mm(y1)), false),
        ]],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    };
    layer.add_polygon(poly);
}

/// Прямоугольник только обводкой (без заливки) — рамки карточек/бейджей.
fn draw_stroke_rect(layer: &PdfLayerReference, color: Rgb, thickness_pt: f32, x0: f32, y0: f32, x1: f32, y1: f32) {
    layer.set_outline_color(Color::Rgb(color));
    layer.set_outline_thickness(thickness_pt);
    let line = Line {
        points: vec![
            (Point::new(Mm(x0), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y1)), false),
            (Point::new(Mm(x0), Mm(y1)), false),
        ],
        is_closed: true,
    };
    layer.add_line(line);
}

/// Обход прямоугольника со скруглёнными углами через кубические кривые
/// Безье (printpdf 0.7 честно поддерживает bezier в Line/Polygon через
/// пары точек с флагом "это ручка кривой" — см. line.rs::into_stream_op:
/// последовательность [конец_ребра(true), ручка1(true), ручка2, конец_дуги]
/// заставляет два подряд идущих true-флага собраться в один кубический
/// bezier по следующим 3 точкам). radius=0 даёт обычный прямой угол.
/// Референс (echips_report_template.html) использует border-radius
/// повсюду — плоские углы были одной из причин, почему первая версия
/// переверстки «выглядела не так, как шаблон».
fn rounded_rect_points(x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> Vec<(Point, bool)> {
    let r = radius.max(0.0).min((x1 - x0).abs() / 2.0).min((y1 - y0).abs() / 2.0);
    if r < 0.05 {
        return vec![
            (Point::new(Mm(x0), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y1)), false),
            (Point::new(Mm(x0), Mm(y1)), false),
        ];
    }
    const K: f32 = 0.5522847498; // магическая константа для аппроксимации четверти окружности кубическим bezier
    let kr = K * r;
    let pt = |x: f32, y: f32| Point::new(Mm(x), Mm(y));
    vec![
        (pt(x0 + r, y0), false),
        (pt(x1 - r, y0), true),
        (pt(x1 - r + kr, y0), true),
        (pt(x1, y0 + r - kr), false),
        (pt(x1, y0 + r), false),
        (pt(x1, y1 - r), true),
        (pt(x1, y1 - r + kr), true),
        (pt(x1 - r + kr, y1), false),
        (pt(x1 - r, y1), false),
        (pt(x0 + r, y1), true),
        (pt(x0 + r - kr, y1), true),
        (pt(x0, y1 - r + kr), false),
        (pt(x0, y1 - r), false),
        (pt(x0, y0 + r), true),
        (pt(x0, y0 + r - kr), true),
        (pt(x0 + r - kr, y0), false),
        (pt(x0 + r, y0), false),
    ]
}

fn draw_rounded_fill(layer: &PdfLayerReference, color: Rgb, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
    layer.set_fill_color(Color::Rgb(color));
    let poly = Polygon {
        rings: vec![rounded_rect_points(x0, y0, x1, y1, radius)],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    };
    layer.add_polygon(poly);
}

fn draw_rounded_stroke(layer: &PdfLayerReference, color: Rgb, thickness_pt: f32, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
    layer.set_outline_color(Color::Rgb(color));
    layer.set_outline_thickness(thickness_pt);
    let line = Line { points: rounded_rect_points(x0, y0, x1, y1, radius), is_closed: true };
    layer.add_line(line);
}

/// Цвета отчёта — светлая «бумага» с тёмной обложкой сверху (см. заметку
/// выше) и статусными цветами, взятыми из присланного референс-шаблона
/// (echips_report_template.html), а не из тёмной темы интерфейса — так
/// пожелал пользователь после проверки первой (полностью тёмной) версии.
struct PdfPalette {
    cover_bg: Rgb,
    cover_text: Rgb,
    cover_text_muted: Rgb,
    text: Rgb,
    text_muted: Rgb,
    line: Rgb,
    line_soft: Rgb,
    accent: Rgb,
    ok_fg: Rgb,
    ok_bg: Rgb,
    ok_border: Rgb,
    bad_fg: Rgb,
    bad_bg: Rgb,
    bad_border: Rgb,
    na_fg: Rgb,
    na_bg: Rgb,
    na_border: Rgb,
}
fn palette() -> PdfPalette {
    PdfPalette {
        cover_bg: Rgb::new(0.0824, 0.0902, 0.1059, None),        // #15171B
        cover_text: Rgb::new(0.949, 0.953, 0.961, None),         // #F2F3F5
        cover_text_muted: Rgb::new(0.784, 0.796, 0.816, None),   // #C8CBD0
        text: Rgb::new(0.102, 0.110, 0.125, None),               // #1A1C20
        text_muted: Rgb::new(0.416, 0.431, 0.463, None),         // #6A6E76
        line: Rgb::new(0.894, 0.882, 0.855, None),               // #E4E1DA
        line_soft: Rgb::new(0.929, 0.922, 0.898, None),          // #EDEBE5
        accent: Rgb::new(0.914, 0.463, 0.0, None),               // #E97600
        ok_fg: Rgb::new(0.0824, 0.502, 0.239, None),             // #15803D
        ok_bg: Rgb::new(0.914, 0.969, 0.933, None),              // #E9F7EE
        ok_border: Rgb::new(0.745, 0.902, 0.796, None),          // #BEE6CB
        bad_fg: Rgb::new(0.753, 0.157, 0.106, None),             // #C0281B
        bad_bg: Rgb::new(0.992, 0.925, 0.918, None),             // #FDECEA
        bad_border: Rgb::new(0.961, 0.749, 0.718, None),         // #F5BFB7
        na_fg: Rgb::new(0.357, 0.392, 0.447, None),              // #5B6472
        na_bg: Rgb::new(0.945, 0.941, 0.925, None),              // #F1F0EC
        na_border: Rgb::new(0.882, 0.871, 0.839, None),          // #E1DED6
    }
}

/// Три «ведра» статуса — как в референсе: не применимо/вне профиля/не
/// проверено объединены в одно серое, чтобы не городить 5 разных цветов.
enum Bucket {
    Ok,
    Bad,
    Na,
}
fn bucket_of(status: &str) -> Bucket {
    match status {
        "pass" => Bucket::Ok,
        "fail" => Bucket::Bad,
        _ => Bucket::Na,
    }
}
fn bucket_colors(p: &PdfPalette, b: &Bucket) -> (Rgb, Rgb, Rgb) {
    match b {
        Bucket::Ok => (p.ok_fg.clone(), p.ok_bg.clone(), p.ok_border.clone()),
        Bucket::Bad => (p.bad_fg.clone(), p.bad_bg.clone(), p.bad_border.clone()),
        Bucket::Na => (p.na_fg.clone(), p.na_bg.clone(), p.na_border.clone()),
    }
}
fn badge_label(r: &TestResult) -> &'static str {
    if r.status == "idle" && r.in_profile == Some(false) {
        "ВНЕ ПРОФИЛЯ"
    } else {
        status_label(&r.status)
    }
}

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}
fn line_h(size_pt: f32) -> f32 {
    pt_to_mm(size_pt) * 1.35
}

/// Сколько символов помещается в ширину `width_mm` при кегле `size_pt`.
fn max_chars_width(size_pt: f32, width_mm: f32) -> usize {
    let avg_char_w_mm = pt_to_mm(size_pt) * 0.52;
    let usable = width_mm.max(20.0);
    ((usable / avg_char_w_mm).floor() as usize).max(10)
}
/// То же самое, но ширина считается от общего содержимого страницы с отступом
/// `indent_mm` слева (используется для «полноширинных» блоков — шапка, итог).
fn max_chars(size_pt: f32, indent_mm: f32) -> usize {
    max_chars_width(size_pt, PDF_CONTENT_W - indent_mm)
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

/// Длительность прогона в человекочитаемом виде ("5 мин 35 с") — парсит
/// ISO 8601 из report.started_at/finished_at; None, если не распозналось.
fn format_duration(report: &DiagnosticReport) -> Option<String> {
    let start = chrono::DateTime::parse_from_rfc3339(&report.started_at).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(&report.finished_at).ok()?;
    let secs = (end - start).num_seconds().max(0);
    let (h, rem) = (secs / 3600, secs % 3600);
    let (m, s) = (rem / 60, rem % 60);
    Some(if h > 0 {
        format!("{h} ч {m} мин")
    } else if m > 0 {
        format!("{m} мин {s} с")
    } else {
        format!("{s} с")
    })
}

struct PdfWriter {
    layer: PdfLayerReference,
    y: f32,
    font_regular: IndirectFontRef,
    font_bold: IndirectFontRef,
    /// Печатается внизу каждой страницы, кроме первой (там обложка).
    footer: String,
    footer_color: Rgb,
    /// Счётчик страниц — нужен, чтобы понять, разорвалась ли карточка
    /// между страницами (тогда рамку вокруг неё не рисуем, см. draw_card).
    page_no: u32,
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
            self.page_no += 1;
            self.draw_footer();
        }
    }

    fn draw_footer(&self) {
        if self.footer.trim().is_empty() {
            return;
        }
        self.layer.set_fill_color(Color::Rgb(self.footer_color.clone()));
        self.layer.use_text(
            self.footer.clone(),
            8.0,
            Mm(PDF_MARGIN_L),
            Mm(PDF_MARGIN_BOTTOM - 6.0),
            &self.font_regular,
        );
    }

    /// Строка списка «имя — значение»: имя слева, значение справа
    /// (правый край — общее поле минус `right_pad_mm`), тонкий разделитель
    /// под строкой — как .check-list .row в референсе.
    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &mut self,
        doc: &PdfDocumentReference,
        name: &str,
        value: &str,
        size_pt: f32,
        name_color: &Rgb,
        value_color: &Rgb,
        line_color: &Rgb,
        indent_mm: f32,
        right_pad_mm: f32,
        row_h: f32,
    ) {
        // Базовая линия текста — на self.y, как в draw_line (иначе при
        // чередовании строк-списков с обычными строками получается наезд,
        // т.к. они бы мерили высоту от разных опорных точек).
        self.ensure_space(doc, row_h);
        let baseline = self.y;
        self.layer.set_fill_color(Color::Rgb(name_color.clone()));
        self.layer.use_text(name.to_string(), size_pt, Mm(PDF_MARGIN_L + indent_mm), Mm(baseline), &self.font_regular);
        let value_w = pt_to_mm(size_pt) * 0.52 * value.chars().count() as f32;
        let value_x = (PDF_PAGE_W - PDF_MARGIN_R - right_pad_mm - value_w).max(PDF_MARGIN_L + indent_mm);
        self.layer.set_fill_color(Color::Rgb(value_color.clone()));
        self.layer.use_text(value.to_string(), size_pt, Mm(value_x), Mm(baseline), &self.font_regular);
        let sep_y = baseline - row_h + 1.3;
        draw_rule(&self.layer, line_color.clone(), PDF_MARGIN_L + indent_mm, PDF_PAGE_W - PDF_MARGIN_R - right_pad_mm, sep_y, sep_y + 0.15);
        self.y -= row_h;
    }

    /// Одна уже перенесённая строка — с проверкой места на странице.
    fn draw_line(&mut self, doc: &PdfDocumentReference, line: &str, size_pt: f32, bold: bool, color: &Rgb, indent_mm: f32) {
        let lh = line_h(size_pt);
        self.ensure_space(doc, lh);
        let font = if bold { self.font_bold.clone() } else { self.font_regular.clone() };
        self.layer.set_fill_color(Color::Rgb(color.clone()));
        self.layer.use_text(line.to_string(), size_pt, Mm(PDF_MARGIN_L + indent_mm), Mm(self.y), &font);
        self.y -= lh;
    }

    fn gap(&mut self, mm: f32) {
        self.y -= mm;
    }
}

/// Карточка одного теста: рамка + цветная полоса слева (по статусу),
/// бейдж статуса сверху справа, заголовок, краткое описание, при наличии —
/// причина смены автовердикта и подробности. Высота карточки заранее не
/// известна (зависит от переноса строк), поэтому сначала «примеряем»
/// (считаем те же wrap_line, что при рисовании), чтобы решить, стоит ли
/// начать новую страницу — как CSS break-inside:avoid, но только когда
/// вся карточка целиком помещается на свежую страницу; если карточка сама
/// больше страницы (длинный журнал сбоев и т.п.) — не пытаемся её сберечь
/// от разрыва, а рамку вокруг нет смысла рисовать (см. ниже page_no).
/// Одна строка подробностей — либо распознанная как «имя — значение»
/// (как .check-list в референсе: имя слева, значение справа, тонкая
/// линия-разделитель между строками), либо обычный перенесённый текст.
/// Распознаём по эвристике, без изменения модели данных app.js: почти
/// все списочные подробности (USB/Bluetooth/Wi-Fi и т.п., см. recordDetail
/// по всему app.js) идут в формате "название — статус" с ОДНИМ тире и
/// коротким значением справа — описательные предложения с тем же тире
/// внутри длиннее и/или встречаются с ним не один раз, так что ложных
/// срабатываний почти нет.
enum DetailLine {
    Row(String, String),
    Text(Vec<String>),
}
fn classify_detail(line: &str, card_w: f32, detail_sz: f32) -> DetailLine {
    let line = line.trim_start();
    if line.matches(" — ").count() == 1 {
        if let Some((name, value)) = line.split_once(" — ") {
            let value = value.trim();
            if !name.is_empty() && !value.is_empty() && value.chars().count() <= 20 {
                let value_w = pt_to_mm(detail_sz) * 0.52 * value.chars().count() as f32;
                let name_max = max_chars_width(detail_sz, (card_w - value_w - 6.0).max(20.0));
                if name.chars().count() <= name_max {
                    return DetailLine::Row(name.to_string(), value.to_string());
                }
            }
        }
    }
    DetailLine::Text(wrap_line(line, max_chars_width(detail_sz, card_w)))
}

fn draw_card(w: &mut PdfWriter, doc: &PdfDocumentReference, p: &PdfPalette, r: &TestResult) {
    let card_w = PDF_CONTENT_W - CARD_INDENT - CARD_RIGHT_PAD;
    const TITLE_SZ: f32 = 12.5;
    const SUMMARY_SZ: f32 = 11.0;
    const DETAIL_SZ: f32 = 9.5;
    const ROW_H: f32 = 6.0;

    let summary = r.comment.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let override_line = match (&r.auto_status, &r.auto_note, &r.override_reason) {
        (Some(st), Some(note), Some(reason)) => Some(format!("Автооценка: {st} — {note}; изменено техником: {reason}")),
        _ => None,
    };
    let summary_lines: Vec<String> = summary.map(|s| wrap_line(s, max_chars_width(SUMMARY_SZ, card_w))).unwrap_or_default();
    let override_lines: Vec<String> = override_line
        .as_deref()
        .map(|s| wrap_line(s, max_chars_width(DETAIL_SZ, card_w)))
        .unwrap_or_default();
    let detail_lines: Vec<DetailLine> = r.details.iter().filter(|l| !l.trim().is_empty()).map(|line| classify_detail(line, card_w, DETAIL_SZ)).collect();
    let has_body = !override_lines.is_empty() || !detail_lines.is_empty();

    let mut measured = 5.0_f32; // верхний паддинг
    measured += line_h(TITLE_SZ);
    measured += summary_lines.len() as f32 * line_h(SUMMARY_SZ);
    if has_body {
        measured += 3.0; // разделитель
        measured += override_lines.len() as f32 * line_h(DETAIL_SZ);
        for d in &detail_lines {
            measured += match d {
                DetailLine::Row(..) => ROW_H,
                DetailLine::Text(lines) => lines.len() as f32 * line_h(DETAIL_SZ),
            };
        }
    }
    measured += 5.0; // нижний паддинг

    let page_h_usable = (PDF_PAGE_H - PDF_MARGIN_TOP) - PDF_MARGIN_BOTTOM;
    if measured <= page_h_usable {
        w.ensure_space(doc, measured);
    }

    let start_page = w.page_no;
    let start_y = w.y;
    let bucket = bucket_of(&r.status);
    let (fg, bg, border) = bucket_colors(p, &bucket);
    let label = badge_label(r);

    w.y -= 5.0;

    // Бейдж — фиксированная позиция у верхнего края карточки, рисуется до
    // заголовка (не зависит от финальной высоты карточки).
    let badge_w = pt_to_mm(8.5) * 0.6 * label.chars().count() as f32 + 6.0;
    let badge_h = 6.0;
    let badge_x1 = PDF_PAGE_W - PDF_MARGIN_R;
    let badge_x0 = badge_x1 - badge_w;
    let badge_y1 = w.y + 1.5;
    let badge_y0 = badge_y1 - badge_h;
    draw_rounded_fill(&w.layer, bg, badge_x0, badge_y0, badge_x1, badge_y1, badge_h / 2.0);
    draw_rounded_stroke(&w.layer, border, 0.5, badge_x0, badge_y0, badge_x1, badge_y1, badge_h / 2.0);
    w.layer.set_fill_color(Color::Rgb(fg.clone()));
    w.layer.use_text(label.to_string(), 8.5, Mm(badge_x0 + 3.0), Mm(badge_y0 + 1.9), &w.font_bold);

    w.draw_line(doc, &r.title, TITLE_SZ, true, &p.text, CARD_INDENT);
    for line in &summary_lines {
        w.draw_line(doc, line, SUMMARY_SZ, false, &p.text_muted, CARD_INDENT);
    }

    if has_body {
        w.y -= 1.0;
        draw_rule(&w.layer, p.line_soft.clone(), PDF_MARGIN_L + CARD_INDENT, PDF_PAGE_W - PDF_MARGIN_R, w.y - 0.1, w.y + 0.1);
        w.y -= 2.0;
        for line in &override_lines {
            w.draw_line(doc, line, DETAIL_SZ, false, &p.text_muted, CARD_INDENT);
        }
        for d in &detail_lines {
            match d {
                DetailLine::Row(name, value) => {
                    w.draw_row(doc, name, value, DETAIL_SZ, &p.text, &p.text_muted, &p.line_soft, CARD_INDENT, CARD_RIGHT_PAD, ROW_H);
                }
                DetailLine::Text(lines) => {
                    for line in lines {
                        w.draw_line(doc, line, DETAIL_SZ, false, &p.text, CARD_INDENT);
                    }
                }
            }
        }
    }

    w.y -= 5.0;

    // Рамка и цветная полоса слева — только если карточка не разорвалась
    // между страницами (иначе они бы обрамляли только часть содержимого).
    if w.page_no == start_page {
        let end_y = w.y;
        draw_rounded_stroke(&w.layer, p.line.clone(), 0.7, PDF_MARGIN_L, end_y, PDF_PAGE_W - PDF_MARGIN_R, start_y, RADIUS_BOX);
        draw_rounded_fill(&w.layer, fg, PDF_MARGIN_L, end_y + 1.5, PDF_MARGIN_L + 1.1, start_y - 1.5, 0.5);
    }

    w.y -= CARD_GAP;
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
    let p = palette();

    let mut w = PdfWriter {
        layer,
        y: PDF_PAGE_H - PDF_MARGIN_TOP,
        font_regular,
        font_bold,
        footer: format!("Echips Hardware Check · {}", report.device_serial),
        footer_color: p.text_muted.clone(),
        page_no: 0,
    };

    let ok_n = report.results.iter().filter(|r| r.status == "pass").count();
    let bad_n = report.results.iter().filter(|r| r.status == "fail").count();
    let na_n = report.results.len().saturating_sub(ok_n + bad_n);

    // ---- обложка: тёмная плашка с лого, заголовком, метаданными и вердиктом ----
    let cover_top = w.y;
    let cover_h = if bad_n > 0 { 78.0 } else { 72.0 };
    draw_rule(&w.layer, p.cover_bg.clone(), 0.0, PDF_PAGE_W, cover_top - cover_h, cover_top + PDF_MARGIN_TOP);

    let logo_h = 13.0_f32;
    let logo_w = logo_h * (LOGO_PX_W / LOGO_PX_H);
    let logo_y = cover_top - 3.0 - logo_h;
    if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(std::io::Cursor::new(LOGO_PNG)) {
        if let Ok(logo) = Image::try_from(decoder) {
            let natural_h_mm = LOGO_PX_H / 300.0 * 25.4;
            let scale = logo_h / natural_h_mm;
            logo.add_to_layer(
                w.layer.clone(),
                ImageTransform {
                    translate_x: Some(Mm(PDF_MARGIN_L)),
                    translate_y: Some(Mm(logo_y)),
                    scale_x: Some(scale),
                    scale_y: Some(scale),
                    ..Default::default()
                },
            );
        }
    }
    w.layer.set_fill_color(Color::Rgb(p.cover_text.clone()));
    w.layer
        .use_text("ECHIPS HARDWARE CHECK", 14.0, Mm(PDF_MARGIN_L + logo_w + 5.0), Mm(logo_y + logo_h / 2.0 - 2.0), &w.font_bold);

    w.y = logo_y - 10.0;
    w.layer.set_fill_color(Color::Rgb(p.cover_text.clone()));
    w.layer.use_text("Отчёт диагностики", 19.0, Mm(PDF_MARGIN_L), Mm(w.y), &w.font_bold);
    w.y -= 7.0;
    w.layer.set_fill_color(Color::Rgb(p.cover_text_muted.clone()));
    w.layer
        .use_text("Полная аппаратная проверка устройства", 10.5, Mm(PDF_MARGIN_L), Mm(w.y), &w.font_regular);

    w.y -= 9.0;
    let meta_col_w = PDF_CONTENT_W / 3.0;
    let duration = format_duration(report);
    let meta: Vec<(&str, String)> = vec![
        ("УСТРОЙСТВО", report.device_model.clone()),
        ("СЕРИЙНЫЙ НОМЕР", report.device_serial.clone()),
        (
            "НАЧАЛО / ДЛИТЕЛЬНОСТЬ",
            duration.map(|d| format!("{} · {}", report.started_at, d)).unwrap_or_else(|| report.started_at.clone()),
        ),
    ];
    for (i, (label, value)) in meta.iter().enumerate() {
        let x = PDF_MARGIN_L + meta_col_w * i as f32;
        w.layer.set_fill_color(Color::Rgb(p.cover_text_muted.clone()));
        w.layer.use_text(label.to_string(), 8.0, Mm(x), Mm(w.y), &w.font_regular);
        w.layer.set_fill_color(Color::Rgb(p.cover_text.clone()));
        let value_lines = wrap_line(value, max_chars_width(9.5, meta_col_w - 6.0));
        let mut vy = w.y - 5.0;
        for line in value_lines.iter().take(2) {
            w.layer.use_text(line.clone(), 9.5, Mm(x), Mm(vy), &w.font_regular);
            vy -= line_h(9.5);
        }
    }
    w.y -= 16.0;

    // вердикт-пилюля + счётчики
    let (verdict_text, verdict_fg, verdict_bg) = if bad_n > 0 {
        (format!("Выявлено неисправностей: {bad_n}"), p.bad_fg.clone(), Rgb::new(0.35, 0.15, 0.12, None))
    } else {
        ("Неисправностей не выявлено".to_string(), p.ok_fg.clone(), Rgb::new(0.10, 0.24, 0.16, None))
    };
    let verdict_w = pt_to_mm(11.0) * 0.55 * verdict_text.chars().count() as f32 + 14.0;
    draw_rounded_fill(&w.layer, verdict_bg, PDF_MARGIN_L, w.y - 6.0, PDF_MARGIN_L + verdict_w, w.y + 3.0, 4.5);
    w.layer.set_fill_color(Color::Rgb(verdict_fg));
    w.layer.use_text(verdict_text, 10.5, Mm(PDF_MARGIN_L + 6.0), Mm(w.y - 3.2), &w.font_bold);

    // Референс красит эти чипы полупрозрачным белым поверх тёмной обложки
    // (rgba(255,255,255,.06) фон, .12 рамка) — printpdf 0.7 не поддерживает
    // альфа-смешение per-объект без лишней возни с graphics state, поэтому
    // приближаем тот же эффект «прозрачного белого на тёмном» через готовый
    // смешанный цвет (cover_bg + белый × 6%/12%).
    let chip_bg = Rgb::new(0.1558, 0.1630, 0.1774, None);
    let chip_border = Rgb::new(0.1925, 0.1994, 0.2132, None);
    let mut chip_x = PDF_MARGIN_L + verdict_w + 8.0;
    for (n, label) in [(ok_n, "ОК"), (na_n, "не проверялось"), (bad_n, "неисправно")] {
        let chip_text = format!("{n} {label}");
        let chip_w = pt_to_mm(9.5) * 0.52 * chip_text.chars().count() as f32 + 10.0;
        draw_rounded_fill(&w.layer, chip_bg.clone(), chip_x, w.y - 6.0, chip_x + chip_w, w.y + 3.0, 4.5);
        draw_rounded_stroke(&w.layer, chip_border.clone(), 0.4, chip_x, w.y - 6.0, chip_x + chip_w, w.y + 3.0, 4.5);
        w.layer.set_fill_color(Color::Rgb(p.cover_text_muted.clone()));
        w.layer.use_text(chip_text, 9.5, Mm(chip_x + 5.0), Mm(w.y - 3.2), &w.font_regular);
        chip_x += chip_w + 5.0;
    }

    w.y = cover_top - cover_h - 10.0;

    // ---- комментарий инженера ----
    if !report.summary_comment.trim().is_empty() {
        let comment = report.summary_comment.trim();
        let lines = wrap_line(comment, max_chars(10.5, 8.0));
        let box_h = 6.0 + line_h(8.5) + lines.len() as f32 * line_h(10.5) + 5.0;
        w.ensure_space(&doc, box_h);
        let box_top = w.y;
        let box_bottom = box_top - box_h;
        draw_rounded_fill(&w.layer, p.na_bg.clone(), PDF_MARGIN_L, box_bottom, PDF_PAGE_W - PDF_MARGIN_R, box_top, RADIUS_BOX_SM);
        draw_rounded_fill(&w.layer, p.accent.clone(), PDF_MARGIN_L, box_bottom, PDF_MARGIN_L + 1.0, box_top, 0.5);
        w.y -= 5.0;
        w.draw_line(&doc, "КОММЕНТАРИЙ ИНЖЕНЕРА", 8.5, true, &p.accent, 8.0);
        for line in &lines {
            w.draw_line(&doc, line, 10.5, false, &p.text, 8.0);
        }
        w.y = box_bottom - 8.0;
    }

    // ---- сводная строка чипов ----
    w.draw_line(&doc, "Результаты проверок", 13.0, true, &p.text, 0.0);
    draw_rule(&w.layer, p.accent.clone(), PDF_MARGIN_L, PDF_MARGIN_L + 7.0, w.y + line_h(13.0) - 1.5, w.y + line_h(13.0) - 0.8);
    w.gap(3.0);

    let chip_w3 = (PDF_CONTENT_W - 8.0) / 3.0;
    let chips_h = 20.0;
    w.ensure_space(&doc, chips_h);
    let chips_top = w.y;
    for (i, (n, label, bucket)) in [(ok_n, "Пройдено без замечаний", Bucket::Ok), (na_n, "Вне профиля / не применимо", Bucket::Na), (bad_n, "Обнаружена неисправность", Bucket::Bad)]
        .into_iter()
        .enumerate()
    {
        let (fg, bg, border) = bucket_colors(&p, &bucket);
        let x0 = PDF_MARGIN_L + (chip_w3 + 4.0) * i as f32;
        draw_rounded_fill(&w.layer, bg, x0, chips_top - chips_h, x0 + chip_w3, chips_top, RADIUS_BOX_SM);
        draw_rounded_stroke(&w.layer, border, 0.6, x0, chips_top - chips_h, x0 + chip_w3, chips_top, RADIUS_BOX_SM);
        w.layer.set_fill_color(Color::Rgb(fg));
        w.layer.use_text(n.to_string(), 16.0, Mm(x0 + 4.0), Mm(chips_top - 8.5), &w.font_bold);
        w.layer.set_fill_color(Color::Rgb(p.text_muted.clone()));
        for line in wrap_line(label, max_chars_width(9.0, chip_w3 - 8.0)).iter().take(1) {
            w.layer.use_text(line.clone(), 9.0, Mm(x0 + 4.0), Mm(chips_top - 15.5), &w.font_regular);
        }
    }
    w.y = chips_top - chips_h - 6.0;

    // ---- карточки тестов ----
    for r in &report.results {
        draw_card(&mut w, &doc, &p, r);
    }

    // ---- итог ----
    w.gap(3.0);
    w.draw_line(&doc, "Итог", 13.0, true, &p.text, 0.0);
    draw_rule(&w.layer, p.accent.clone(), PDF_MARGIN_L, PDF_MARGIN_L + 7.0, w.y + line_h(13.0) - 1.5, w.y + line_h(13.0) - 0.8);
    w.gap(3.0);

    // Референс кладёт заголовок и цветные плашки неисправностей в один
    // ряд (flex, title слева / чипы справа) — printpdf не даёт готового
    // flex-лейаута, поэтому раскладываем чипы построчно сами: считаем,
    // сколько чипов помещается в оставшуюся ширину строки, переносим
    // остаток на следующую.
    let failed: Vec<&TestResult> = report.results.iter().filter(|r| r.status == "fail").collect();
    let (vb_fg, vb_bg, vb_border) = if failed.is_empty() { (p.ok_fg.clone(), p.ok_bg.clone(), p.ok_border.clone()) } else { (p.bad_fg.clone(), p.bad_bg.clone(), p.bad_border.clone()) };
    let verdict_title = if failed.is_empty() { "Неисправностей не выявлено".to_string() } else { format!("Выявлено неисправностей: {}", failed.len()) };
    let title_lines = wrap_line(&verdict_title, max_chars(15.0, 8.0));
    const CHIP_H: f32 = 8.0;
    let chip_rows: usize = if failed.is_empty() {
        0
    } else {
        let mut rows = 1usize;
        let mut x = PDF_MARGIN_L + 8.0;
        for f in &failed {
            let w_chip = pt_to_mm(10.0) * 0.55 * f.title.chars().count() as f32 + 12.0;
            if x + w_chip > PDF_PAGE_W - PDF_MARGIN_R - 8.0 && x > PDF_MARGIN_L + 8.0 {
                rows += 1;
                x = PDF_MARGIN_L + 8.0;
            }
            x += w_chip + 5.0;
        }
        rows
    };
    let vb_h = 10.0 + title_lines.len() as f32 * line_h(15.0) + if chip_rows > 0 { chip_rows as f32 * (CHIP_H + 3.0) + 3.0 } else { 0.0 } + 6.0;
    w.ensure_space(&doc, vb_h);
    let vb_top = w.y;
    let vb_bottom = vb_top - vb_h;
    draw_rounded_fill(&w.layer, vb_bg, PDF_MARGIN_L, vb_bottom, PDF_PAGE_W - PDF_MARGIN_R, vb_top, RADIUS_BOX);
    draw_rounded_stroke(&w.layer, vb_border.clone(), 0.7, PDF_MARGIN_L, vb_bottom, PDF_PAGE_W - PDF_MARGIN_R, vb_top, RADIUS_BOX);
    w.y -= 8.0;
    for line in &title_lines {
        w.draw_line(&doc, line, 15.0, true, &vb_fg, 8.0);
    }
    if !failed.is_empty() {
        w.gap(2.0);
        let mut x = PDF_MARGIN_L + 8.0;
        let mut row_top = w.y;
        for f in &failed {
            let w_chip = pt_to_mm(10.0) * 0.55 * f.title.chars().count() as f32 + 12.0;
            if x + w_chip > PDF_PAGE_W - PDF_MARGIN_R - 8.0 && x > PDF_MARGIN_L + 8.0 {
                x = PDF_MARGIN_L + 8.0;
                row_top -= CHIP_H + 3.0;
            }
            draw_rounded_fill(&w.layer, Rgb::new(1.0, 1.0, 1.0, None), x, row_top - CHIP_H, x + w_chip, row_top, CHIP_H / 2.0);
            draw_rounded_stroke(&w.layer, vb_border.clone(), 0.4, x, row_top - CHIP_H, x + w_chip, row_top, CHIP_H / 2.0);
            w.layer.set_fill_color(Color::Rgb(vb_fg.clone()));
            w.layer.use_text(f.title.clone(), 10.0, Mm(x + 6.0), Mm(row_top - CHIP_H + 2.3), &w.font_bold);
            x += w_chip + 5.0;
        }
        w.y = row_top - CHIP_H;
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
