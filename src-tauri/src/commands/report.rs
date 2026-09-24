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
   сверху вместо прежнего полностью тёмного фона (v0.10.0). Два шрифта
   (OFL, оба с кириллицей — вшиты из src-tauri/assets/fonts): PT Sans —
   основной текст, JetBrains Mono — значения там, где референс задаёт
   font-family:var(--font-mono) (.kv .v, .check-list .row .s, .log-entry
   .t, серийник/диапазон времени в шапке). Ширина символов не измеряется
   через метрики шрифта (printpdf 0.7 их наружу не отдаёт) — перенос
   строк по эвристике "средний символ ~0.52 кегля у PT Sans / ~0.62 у
   моноширинного JetBrains Mono" (max_chars_width/max_chars_width_mono),
   этого достаточно для служебного отчёта и не считается точной вёрсткой.

   Углы карточек/бейджей/пилюль/чипов скруглены по-настоящему — через
   кубические кривые Безье, которые printpdf 0.7 честно поддерживает в
   Line/Polygon (см. rounded_rect_points) — а не прямые, как в первой
   версии переверстки (главная причина, по которой она "выглядела не
   так, как шаблон").

   Строки `details` — плоский Vec<String> уже готовых строк из app.js
   (recordDetail), а не структурированные пары ключ-значение — но почти
   весь app.js и так собирает их по шаблону "Метка: значение" (через
   `chk()` и аналогичные хелперы) или "имя — статус" (списки устройств),
   поэтому вместо смены модели данных во всех тестах используется
   эвристика `classify_detail()`, которая по каждой строке распознаёт:
   - "Метка: значение" → двухколоночная kv-сетка (`DetailRun::KvGrid`,
     `draw_kv_pair`) — как .check-details в референсе, по два элемента
     в ряд; метка над значением, а не инлайн как в CSS (printpdf не даёт
     готового inline-flex, а длинные значения почти всегда переносятся
     на новую строку в любом случае);
   - "имя — статус" → список-таблица (`DetailRun::Row`, `draw_row`) — имя
     слева, значение справа, тонкий разделитель под строкой, как
     .check-list в референсе (USB/Bluetooth/Wi-Fi-сети и т.п.);
   - всё остальное → обычный перенесённый текст (`DetailRun::Text`).
   Порог длины метки (KV_LABEL_MAX_CHARS) отсекает случайные
   двоеточия внутри предложений (диагнозы, пояснения) от настоящих пар
   ключ-значение. Без единой правки на стороне JS.

   Не сделано (осталось одной колонкой текста через DetailRun::Text):
   таблица журнала сбоев с зеброй и отдельной колонкой времени, и жёлтая
   плашка-алерт диагноза — эти два элемента визуально сложнее одной
   эвристики (нужна декомпозиция строки на время/сообщение/код и
   заголовок/пояснение диагноза), пока не делалось.

   Упрощение, оставленное сознательно: карточка не имеет заливки фона
   (только рамка + цветная полоса слева) — страница и так белая, доп.
   заливка не нужна и не мешает переносу карточки между страницами
   (см. draw_card). */

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
/// JetBrains Mono (OFL, полная поддержка кириллицы — проверено скриптом
/// через fontTools перед добавлением, не наугад) — для значений там, где
/// референс задаёт `font-family:var(--font-mono)`: .kv .v, .check-list
/// .row .s, .log-entry .t, серийный номер/диапазон времени в шапке.
/// Статический regular-инстанс релиза (не variable-font — printpdf/
/// ttf_parser надёжнее работают со статикой, как и с PT Sans).
const FONT_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
/// Фирменный знак шапки — растеризован из logo/echips-mark-orange.svg,
/// присланного пользователем отдельным пакетом (echips-report.zip):
/// оранжевый шестиугольник для тёмного фона, как в .brand .hex референса.
/// Раньше здесь использовался обычный логотип приложения (logo.png) —
/// пятиугольник с вписанным текстом ECHIPS — и рядом ещё раз рисовался
/// текст "ECHIPS HARDWARE CHECK": получалось два разных знака и дублирующая
/// подпись, отсюда и замечание "шапка отличается, логотип другой".
const MARK_PNG: &[u8] = include_bytes!("../../assets/mark_orange.png");
const MARK_PX: f32 = 256.0;

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

/// Круг (через rounded_rect_points на квадрате с radius = половина
/// стороны — при таком радиусе четыре угла превращаются в полную
/// окружность целиком, без прямых сегментов между дугами).
fn draw_circle_fill(layer: &PdfLayerReference, color: Rgb, cx: f32, cy: f32, r: f32) {
    draw_rounded_fill(layer, color, cx - r, cy - r, cx + r, cy + r, r);
}

/// Четверть круга («сектор», не «ломтик» — залитая площадь между двумя
/// радиусами и дугой между ними), растущая от точки (cx,cy) СТРОГО в
/// направлении (dx,dy) (каждый ±1.0). Тот же приём кубической аппроксимации
/// четверти окружности, что и в rounded_rect_points (см. константу K там) —
/// но здесь не угол прямоугольника, а самостоятельная фигура: 6 точек,
/// [центр → точка на первом радиусе (bezier-триггер) → 2 контрольные точки
/// → точка на втором радиусе → обратно в центр], с явным замыканием через
/// оба радиуса, а не через дугу целиком.
fn quarter_disc_points(cx: f32, cy: f32, r: f32, dx: f32, dy: f32) -> Vec<(Point, bool)> {
    const K: f32 = 0.5522847498;
    let pt = |x: f32, y: f32| Point::new(Mm(x), Mm(y));
    let p_a = (cx + dx * r, cy);
    let ctrl1 = (cx + dx * (r - K * r), cy);
    let ctrl2 = (cx, cy + dy * (r - K * r));
    let p_b = (cx, cy + dy * r);
    vec![
        (pt(cx, cy), false),
        (pt(p_a.0, p_a.1), true),
        (pt(ctrl1.0, ctrl1.1), true),
        (pt(ctrl2.0, ctrl2.1), false),
        (pt(p_b.0, p_b.1), false),
        (pt(cx, cy), false),
    ]
}
fn draw_quarter_disc_fill(layer: &PdfLayerReference, color: Rgb, cx: f32, cy: f32, r: f32, dx: f32, dy: f32) {
    layer.set_fill_color(Color::Rgb(color));
    let poly = Polygon { rings: vec![quarter_disc_points(cx, cy, r, dx, dy)], mode: PaintMode::Fill, winding_order: WindingOrder::NonZero };
    layer.add_polygon(poly);
}

/// Приближение radial-gradient из референса (.cover::after — тёплое
/// оранжевое свечение по углам тёмной обложки, `rgba(255,138,0,.2..0)`)
/// через несколько вложенных секторов от большого бледного к маленькому
/// насыщенному — printpdf 0.7 не поддерживает градиентные заливки, а
/// плоский без него цвет обложки был одной из причин, почему шапка
/// «выглядела не так» (референс объёмнее/теплее, у нас было плоско).
///
/// ВАЖНО: раньше здесь рисовались ПОЛНЫЕ круги с центром ровно в углу
/// обложки — у полного круга с центром на самой границе фигуры ПОЛОВИНА
/// всегда оказывается за её пределами. В браузере это скрыто через
/// `.cover{overflow:hidden}`, у printpdf готового клиппинга нет — в
/// реальном PDF круг физически протекал на белую страницу под обложкой
/// (обнаружено пользователем на реальном экспорте — "залез круг вниз").
/// Сектор (четверть круга), растущий СТРОГО в сторону (dx,dy) от угла
/// внутрь обложки, физически не может выйти за её границы — это не
/// подбор радиуса на глаз, а фигура, которая по построению не пересекает
/// границу.
/// `max_alpha` — насколько сильно цвет свечения смешивается с базовым
/// в центре (на краю всегда 0, т.е. чистый базовый цвет). `(dx,dy)` —
/// направление внутрь обложки от точки-угла (см. вызовы в render_pdf).
fn draw_radial_glow(layer: &PdfLayerReference, base: &Rgb, glow: &Rgb, cx: f32, cy: f32, max_r: f32, max_alpha: f32, dx: f32, dy: f32) {
    const STEPS: usize = 10;
    // Рисуем от большого бледного сектора (фон) к маленькому насыщенному
    // (поверх) — иначе более поздний слой перекрыл бы всё, что нарисовано
    // раньше, и результат выглядел бы как один сплошной бледный сектор без
    // видимого затухания к центру.
    for i in 0..STEPS {
        let t = i as f32 / (STEPS - 1) as f32; // 0.0 на первом (большом) секторе … 1.0 на последнем (маленьком)
        let r = max_r * (1.0 - 0.85 * t); // от max_r до 0.15×max_r — маленькое насыщенное ядро всегда видно
        let a = max_alpha * t * t; // квадратичное усиление к центру — мягче на глаз, чем линейное
        let color = Rgb::new(
            base.r * (1.0 - a) + glow.r * a,
            base.g * (1.0 - a) + glow.g * a,
            base.b * (1.0 - a) + glow.b * a,
            None,
        );
        draw_quarter_disc_fill(layer, color, cx, cy, r, dx, dy);
    }
}

fn draw_rounded_stroke(layer: &PdfLayerReference, color: Rgb, thickness_pt: f32, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
    layer.set_outline_color(Color::Rgb(color));
    layer.set_outline_thickness(thickness_pt);
    let line = Line { points: rounded_rect_points(x0, y0, x1, y1, radius), is_closed: true };
    layer.add_line(line);
}

/// Цвета отчёта — тема «Графит» (вариант A из макетов, выбран пользователем):
/// тёмный фон на всех страницах, чуть более тёмная обложка, карточки на
/// «поднятой» поверхности, оранжевый акцент; статусные цвета — яркий текст на
/// приглушённой тёмной плашке того же оттенка.
struct PdfPalette {
    page_bg: Rgb,
    card_bg: Rgb,
    cover_bg: Rgb,
    cover_text: Rgb,
    cover_text_muted: Rgb,
    text: Rgb,
    text_muted: Rgb,
    text_faint: Rgb,
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
    warn_fg: Rgb,
    warn_bg: Rgb,
    warn_border: Rgb,
    paper_soft: Rgb,
}
fn palette() -> PdfPalette {
    PdfPalette {
        page_bg: Rgb::new(0.0824, 0.0902, 0.1059, None),         // #15171B
        card_bg: Rgb::new(0.1137, 0.1255, 0.1451, None),         // #1D2025
        cover_bg: Rgb::new(0.0549, 0.0627, 0.0745, None),        // #0E1013
        cover_text: Rgb::new(0.949, 0.953, 0.961, None),         // #F2F3F5
        cover_text_muted: Rgb::new(0.604, 0.627, 0.659, None),   // #9AA0A8
        text: Rgb::new(0.914, 0.918, 0.925, None),               // #E9EAEC
        text_muted: Rgb::new(0.604, 0.627, 0.659, None),         // #9AA0A8
        text_faint: Rgb::new(0.486, 0.506, 0.537, None),         // #7C8189
        line: Rgb::new(0.169, 0.184, 0.212, None),               // #2B2F36
        line_soft: Rgb::new(0.169, 0.184, 0.212, None),          // #2B2F36
        accent: Rgb::new(1.0, 0.541, 0.0, None),                 // #FF8A00
        ok_fg: Rgb::new(0.373, 0.816, 0.553, None),              // #5FD08D
        ok_bg: Rgb::new(0.071, 0.188, 0.122, None),              // #12301F
        ok_border: Rgb::new(0.122, 0.322, 0.212, None),          // #1F5236
        bad_fg: Rgb::new(1.0, 0.561, 0.510, None),               // #FF8F82
        bad_bg: Rgb::new(0.227, 0.082, 0.071, None),             // #3A1512
        bad_border: Rgb::new(0.478, 0.169, 0.145, None),         // #7A2B25
        na_fg: Rgb::new(0.604, 0.627, 0.659, None),              // #9AA0A8
        na_bg: Rgb::new(0.137, 0.149, 0.173, None),              // #23262C
        na_border: Rgb::new(0.2, 0.216, 0.243, None),            // #33373E
        warn_fg: Rgb::new(0.941, 0.761, 0.294, None),            // #F0C24B
        warn_bg: Rgb::new(0.180, 0.149, 0.071, None),            // #2E2612
        warn_border: Rgb::new(0.420, 0.337, 0.125, None),        // #6B5620
        paper_soft: Rgb::new(0.141, 0.157, 0.180, None),         // #24282E
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
/// То же самое, но для моноширинного JetBrains Mono — все символы одной
/// ширины (~0.62 кегля), а не ~0.52 как в среднем у пропорционального
/// PT Sans, иначе перенос значений в .kv/.check-list/журнале был бы
/// то слишком щедрым, то слишком тесным.
fn max_chars_width_mono(size_pt: f32, width_mm: f32) -> usize {
    let char_w_mm = pt_to_mm(size_pt) * 0.62;
    let usable = width_mm.max(20.0);
    ((usable / char_w_mm).floor() as usize).max(8)
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

/// "23.09.2026 08:33 → 08:39" (или с датой на обоих концах, если сутки
/// разные) — как в референсе (.cover-meta "Начало / окончание"), вместо
/// сырых ISO-строк started_at/finished_at.
fn format_time_range(report: &DiagnosticReport) -> Option<String> {
    let start = chrono::DateTime::parse_from_rfc3339(&report.started_at).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(&report.finished_at).ok()?;
    let start_local = start.with_timezone(&chrono::Local);
    let end_local = end.with_timezone(&chrono::Local);
    Some(if start_local.date_naive() == end_local.date_naive() {
        format!("{} {} → {}", start_local.format("%d.%m.%Y"), start_local.format("%H:%M"), end_local.format("%H:%M"))
    } else {
        format!("{} → {}", start_local.format("%d.%m.%Y %H:%M"), end_local.format("%d.%m.%Y %H:%M"))
    })
}

/// "Выявлена 1 неисправность" / "Выявлены 3 неисправности" / "Выявлено
/// 5 неисправностей" — согласование глагола и числительного с count, как
/// в референсе (там пример на 1: "Выявлена 1 неисправность"; раньше у нас
/// всегда было "Выявлено неисправностей: N" без согласования).
fn ru_defects_verdict(n: usize) -> String {
    let (verb, noun) = match n % 100 {
        11..=14 => ("Выявлено", "неисправностей"),
        _ => match n % 10 {
            1 => ("Выявлена", "неисправность"),
            2..=4 => ("Выявлены", "неисправности"),
            _ => ("Выявлено", "неисправностей"),
        },
    };
    format!("{verb} {n} {noun}")
}

struct PdfWriter {
    layer: PdfLayerReference,
    y: f32,
    font_regular: IndirectFontRef,
    font_bold: IndirectFontRef,
    font_mono: IndirectFontRef,
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
        let layer = doc.get_page(page).get_layer(layer);
        draw_rule(&layer, palette().page_bg, 0.0, PDF_PAGE_W, 0.0, PDF_PAGE_H);
        (layer, PDF_PAGE_H - PDF_MARGIN_TOP)
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
        // Значение — моно-шрифтом, как .check-list .row .s в референсе.
        let value_w = pt_to_mm(size_pt) * 0.62 * value.chars().count() as f32;
        let value_x = (PDF_PAGE_W - PDF_MARGIN_R - right_pad_mm - value_w).max(PDF_MARGIN_L + indent_mm);
        self.layer.set_fill_color(Color::Rgb(value_color.clone()));
        self.layer.use_text(value.to_string(), size_pt, Mm(value_x), Mm(baseline), &self.font_mono);
        let sep_y = baseline - row_h + 1.3;
        draw_rule(&self.layer, line_color.clone(), PDF_MARGIN_L + indent_mm, PDF_PAGE_W - PDF_MARGIN_R - right_pad_mm, sep_y, sep_y + 0.15);
        self.y -= row_h;
    }

    /// Пара элементов kv-сетки бок о бок (левый/правый столбец) — метка
    /// мелким тусклым текстом, значение крупнее под ней (перенесено под
    /// ширину своей половины колонки заранее, в group_detail_runs). Как
    /// .check-details{grid-template-columns:repeat(2,1fr)} в референсе,
    /// только не инлайн (метка — значение на одной строке), а метка над
    /// значением: так не нужно измерять ширину метки для выравнивания
    /// внутренней «изгороди» с переносом — printpdf не даёт готового
    /// inline-flex, а длинные значения (например, строка процессора)
    /// всё равно почти всегда переносятся на новую строку.
    #[allow(clippy::too_many_arguments)]
    fn draw_kv_pair(
        &mut self,
        doc: &PdfDocumentReference,
        left: &(String, Vec<String>),
        right: Option<&(String, Vec<String>)>,
        label_sz: f32,
        value_sz: f32,
        label_color: &Rgb,
        value_color: &Rgb,
        indent_mm: f32,
        half_col_w: f32,
        gap_mm: f32,
    ) {
        let row_h = left.1.len().max(right.map(|r| r.1.len()).unwrap_or(0)).max(1) as f32 * line_h(value_sz) + line_h(label_sz);
        self.ensure_space(doc, row_h);
        let top = self.y;
        for (col, item) in [Some(left), right].into_iter().flatten().enumerate() {
            let x = PDF_MARGIN_L + indent_mm + col as f32 * (half_col_w + gap_mm);
            let mut y = top;
            self.layer.set_fill_color(Color::Rgb(label_color.clone()));
            self.layer.use_text(item.0.clone(), label_sz, Mm(x), Mm(y), &self.font_regular);
            y -= line_h(label_sz);
            // Значение — моно-шрифтом, как .kv .v в референсе.
            self.layer.set_fill_color(Color::Rgb(value_color.clone()));
            for line in &item.1 {
                self.layer.use_text(line.clone(), value_sz, Mm(x), Mm(y), &self.font_mono);
                y -= line_h(value_sz);
            }
        }
        self.y -= row_h;
    }

    /// Одна строка журнала сбоев — зебра-фон (чередование с paper_soft, как
    /// .log-entry:nth-child(even) в референсе), слева время моно-стилем,
    /// справа сообщение + при наличии — пояснение (человеческий "hint" из
    /// crash_logic.rs) более мелким тусклым текстом второй строкой. В
    /// референсе только одна строка на событие — у нас реальные данные
    /// всегда несут ещё и explain-текст, решили не терять его, а не
    /// добиваться точного 1:1 совпадения структуры.
    #[allow(clippy::too_many_arguments)]
    fn draw_log_item(&mut self, doc: &PdfDocumentReference, item: &LogItem, even: bool, detail_sz: f32, p: &PdfPalette, indent_mm: f32, time_w: f32, right_x: f32) {
        let h = log_item_height(item, detail_sz);
        self.ensure_space(doc, h);
        let top = self.y;
        if even {
            draw_rule(&self.layer, p.paper_soft.clone(), PDF_MARGIN_L + indent_mm, right_x, top - h, top);
        }
        let text_x = PDF_MARGIN_L + indent_mm + time_w;
        // Время — моно-шрифтом, как .log-entry .t в референсе.
        self.layer.set_fill_color(Color::Rgb(p.text_muted.clone()));
        self.layer.use_text(item.time.clone(), detail_sz - 1.0, Mm(PDF_MARGIN_L + indent_mm + 2.0), Mm(top - line_h(detail_sz) + 1.0), &self.font_mono);
        self.layer.set_fill_color(Color::Rgb(p.text.clone()));
        let mut y = top - line_h(detail_sz) + 1.0;
        for line in &item.message {
            self.layer.use_text(line.clone(), detail_sz, Mm(text_x), Mm(y), &self.font_regular);
            y -= line_h(detail_sz);
        }
        self.layer.set_fill_color(Color::Rgb(p.text_muted.clone()));
        for line in &item.note {
            self.layer.use_text(line.clone(), detail_sz - 1.0, Mm(text_x), Mm(y), &self.font_regular);
            y -= line_h(detail_sz - 1.0);
        }
        self.y -= h;
    }

    /// Элемент плашки диагноза — заголовок (⚠/ℹ по item.warn) + пояснение,
    /// как .ditem внутри .diagnosis в референсе (сама рамка/фон плашки
    /// рисуется один раз на весь блок вызывающим кодом).
    fn draw_diag_item(&mut self, doc: &PdfDocumentReference, item: &DiagItem, detail_sz: f32, p: &PdfPalette, indent_mm: f32) {
        let icon = if item.warn { "⚠ " } else { "ℹ " };
        let mut first = true;
        for line in &item.title {
            let text = if first { format!("{icon}{line}") } else { line.clone() };
            self.draw_line(doc, &text, detail_sz + 1.0, true, &p.warn_fg, indent_mm);
            first = false;
        }
        for line in &item.text {
            self.draw_line(doc, line, detail_sz, false, &p.text, indent_mm);
        }
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
/// Одна строка подробностей, распознанная по эвристике — без изменения
/// модели данных app.js (там `details` остаётся плоским Vec<String>):
/// - "Метка: значение" (после `chk()`/аналогичных хелперов почти весь
///   app.js собирает строки именно так — "Процессор: ...", "BIOS: ...",
///   "Статус лицензии: ..." и т.д.) — как .kv в референсе;
/// - "имя — статус" (USB/Bluetooth/Wi-Fi-сети и т.п.) — как .check-list;
/// - всё остальное — обычный перенесённый текст (узкие сентенции вроде
///   диагноза сбоев, где после первого совпадения по эвристике выходит
///   не пара ключ-значение, а начало предложения, отсекается порогом
///   длины метки — см. ниже).
enum DetailLine {
    Kv(String, String),
    Row(String, String),
    Text(Vec<String>),
}
/// Метки вида "Процессор"/"Статус лицензии" короткие (до ~2-3 слов) —
/// длиннее уже почти наверняка начало предложения с двоеточием внутри
/// (диагнозы, пояснения), а не ключ-значение.
const KV_LABEL_MAX_CHARS: usize = 34;

fn classify_detail(line: &str, card_w: f32, detail_sz: f32) -> DetailLine {
    let mut line = line.trim_start();
    for prefix in ["• ", "✓ ", "✗ "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            line = rest;
            break;
        }
    }
    if let Some((label, value)) = line.split_once(": ") {
        let value = value.trim();
        if !label.is_empty() && label.chars().count() <= KV_LABEL_MAX_CHARS && !value.is_empty() {
            return DetailLine::Kv(label.to_string(), value.to_string());
        }
    }
    // По ПОСЛЕДНЕМУ тире, а не по первому/единственному: у части реальных
    // названий устройств само тире уже встроено (например "AMD USB 3.10
    // — 1.10 (Майкрософт) — работает" от list_usb_devices) — единственный
    // способ отличить настоящий разделитель "имя — статус" от тире внутри
    // названия — искать его с конца, статус всегда короткий хвост строки.
    if let Some((name, value)) = line.rsplit_once(" — ") {
        let value = value.trim();
        // value, начинающееся с «·», — это продолжение строки счётчиков («… включений — · записано —»), а не статус
        if !name.is_empty() && !value.is_empty() && !value.starts_with('·') && value.chars().count() <= 20 {
            let value_w = pt_to_mm(detail_sz) * 0.62 * value.chars().count() as f32; // моно-шрифт, см. draw_row
            let name_max = max_chars_width(detail_sz, (card_w - value_w - 6.0).max(20.0));
            if name.chars().count() <= name_max {
                return DetailLine::Row(name.to_string(), value.to_string());
            }
        }
    }
    DetailLine::Text(wrap_line(line, max_chars_width(detail_sz, card_w)))
}

/// Подряд идущие Kv-строки группируются в один блок и рисуются двухколоночной
/// сеткой (по два элемента в ряд) — как .check-details в референсе.
/// Row/Text рисуются по одной строке, Log/Diagnosis — см. LogItem/DiagItem
/// ниже (журнал сбоев в crash.js — единственный тест с обоими форматами).
enum DetailRun {
    KvGrid(Vec<(String, Vec<String>)>), // (метка, перенесённые строки значения)
    Row(String, String),
    Text(Vec<String>),
    Log(Vec<LogItem>),
    Diagnosis(Vec<DiagItem>),
}
struct LogItem {
    time: String,
    message: Vec<String>,
    note: Vec<String>,
}
struct DiagItem {
    warn: bool,
    title: Vec<String>,
    text: Vec<String>,
}

/// Строка вида "2026-09-15 10:12:51 · сообщение" — журнал сбоев (crash.js)
/// собирает такие строки для каждого события; timestamp всегда ISO-подобный
/// с секундами, что легко проверить посимвольно без зависимости от regex
/// (в проекте её нет — CLAUDE.md про минимальные зависимости в Rust-коде).
fn parse_log_prefix(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    if b.len() < 21 {
        return None;
    }
    let is_digit = |i: usize| b.get(i).map(|c| c.is_ascii_digit()).unwrap_or(false);
    let is = |i: usize, c: u8| b.get(i) == Some(&c);
    let ok = (0..4).all(is_digit) && is(4, b'-') && (5..7).all(is_digit) && is(7, b'-') && (8..10).all(is_digit)
        && is(10, b' ') && (11..13).all(is_digit) && is(13, b':') && (14..16).all(is_digit) && is(16, b':') && (17..19).all(is_digit);
    if !ok {
        return None;
    }
    line.get(19..).and_then(|rest| rest.strip_prefix(" · "))
}
/// "— Диагноз по шаблону сбоев —" (crash.js) — маркер начала блока
/// диагноза; всё после него в подробностях этого теста — пары
/// заголовок(+⚠/ℹ)/пояснение, а не обычные строки.
const DIAGNOSIS_MARKER: &str = "— Диагноз по шаблону сбоев —";

#[allow(clippy::too_many_arguments)]
fn build_detail_runs(raw: &[String], card_w: f32, detail_sz: f32, half_col_w: f32, value_sz: f32, log_time_w: f32) -> Vec<DetailRun> {
    let lines: Vec<&str> = raw.iter().map(|s| s.as_str()).filter(|s| !s.trim().is_empty()).collect();
    let mut runs = Vec::new();
    let mut kv_buf: Vec<(String, Vec<String>)> = Vec::new();
    let mut log_buf: Vec<LogItem> = Vec::new();
    let mut diag_buf: Vec<DiagItem> = Vec::new();
    macro_rules! flush_kv {
        () => {
            if !kv_buf.is_empty() {
                runs.push(DetailRun::KvGrid(std::mem::take(&mut kv_buf)));
            }
        };
    }
    macro_rules! flush_log {
        () => {
            if !log_buf.is_empty() {
                runs.push(DetailRun::Log(std::mem::take(&mut log_buf)));
            }
        };
    }
    macro_rules! flush_diag {
        () => {
            if !diag_buf.is_empty() {
                runs.push(DetailRun::Diagnosis(std::mem::take(&mut diag_buf)));
            }
        };
    }

    let mut in_diagnosis = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim() == DIAGNOSIS_MARKER {
            flush_kv!();
            flush_log!();
            in_diagnosis = true;
            i += 1;
            continue;
        }
        if in_diagnosis {
            flush_kv!();
            flush_log!();
            let mut title = line.trim_start();
            let mut warn = true;
            for (prefix, is_warn) in [("⚠ ", true), ("ℹ ", false)] {
                if let Some(rest) = title.strip_prefix(prefix) {
                    title = rest;
                    warn = is_warn;
                    break;
                }
            }
            let mut text_lines = Vec::new();
            let mut consumed = 1;
            if let Some(next) = lines.get(i + 1) {
                if let Some(text) = next.strip_prefix("    ") {
                    text_lines = wrap_line(text.trim_start(), max_chars_width(detail_sz, card_w - 8.0));
                    consumed = 2;
                }
            }
            diag_buf.push(DiagItem { warn, title: wrap_line(title, max_chars_width(detail_sz + 1.0, card_w - 8.0)), text: text_lines });
            i += consumed;
            continue;
        }
        if let Some(message) = parse_log_prefix(line) {
            flush_kv!();
            let mut note_lines = Vec::new();
            let mut consumed = 1;
            if let Some(next) = lines.get(i + 1) {
                if let Some(note) = next.strip_prefix("    ") {
                    note_lines = wrap_line(note.trim_start(), max_chars_width(detail_sz - 1.0, card_w - log_time_w - 4.0));
                    consumed = 2;
                }
            }
            log_buf.push(LogItem {
                time: line[..19].to_string(),
                message: wrap_line(message, max_chars_width(detail_sz, card_w - log_time_w - 4.0)),
                note: note_lines,
            });
            i += consumed;
            continue;
        }
        // flush_kv здесь НЕ вызывается заранее: если строка сама окажется
        // Kv, она должна попасть в уже накопленный kv_buf, а не начать
        // новый буфер из одного элемента (иначе подряд идущие "Метка:
        // значение" никогда бы не собрались в общую сетку по 2 в ряд).
        flush_log!();
        match classify_detail(line, card_w, detail_sz) {
            DetailLine::Kv(label, value) => {
                // Моно-шрифт (draw_kv_pair) — перенос считаем по его ширине символа.
                let value_lines = wrap_line(&value, max_chars_width_mono(value_sz, half_col_w));
                kv_buf.push((label, value_lines));
            }
            DetailLine::Row(name, value) => {
                flush_kv!();
                runs.push(DetailRun::Row(name, value));
            }
            DetailLine::Text(wrapped) => {
                flush_kv!();
                runs.push(DetailRun::Text(wrapped));
            }
        }
        i += 1;
    }
    flush_kv!();
    flush_log!();
    flush_diag!();
    runs
}
fn kv_item_height(value_lines_len: usize, label_sz: f32, value_sz: f32) -> f32 {
    line_h(label_sz) + value_lines_len.max(1) as f32 * line_h(value_sz)
}
fn log_item_height(item: &LogItem, detail_sz: f32) -> f32 {
    line_h(detail_sz) + item.note.len() as f32 * line_h(detail_sz - 1.0) + 2.0
}
fn diag_item_height(item: &DiagItem, detail_sz: f32) -> f32 {
    item.title.len() as f32 * line_h(detail_sz + 1.0) + item.text.len() as f32 * line_h(detail_sz) + 3.0
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
    const KV_GAP: f32 = 6.0;
    const KV_LABEL_SZ: f32 = 8.0;
    // 32мм были откалиброваны под старый пропорциональный PT Sans (0.52
    // символа/кегль) — после перехода времени в журнале на моноширинный
    // JetBrains Mono (0.62 символа/кегль, см. draw_log_item) метка вида
    // "2026-09-15 10:12:51" (19 символов, кегль 8.5pt) стала шире колонки
    // (~35мм без отступов) и наезжала на текст события. Посчитано с
    // запасом под отступ слева (2мм) и зазор перед сообщением (2мм).
    const LOG_TIME_W: f32 = 40.0;
    const DIAG_PAD: f32 = 4.0;
    let half_col_w = (card_w - KV_GAP) / 2.0;
    let runs = build_detail_runs(&r.details, card_w, DETAIL_SZ, half_col_w, DETAIL_SZ, LOG_TIME_W);
    let has_body = !override_lines.is_empty() || !runs.is_empty();

    let run_height = |run: &DetailRun| -> f32 {
        match run {
            DetailRun::Row(..) => ROW_H,
            DetailRun::Text(lines) => lines.len() as f32 * line_h(DETAIL_SZ),
            DetailRun::KvGrid(items) => items
                .chunks(2)
                .map(|pair| pair.iter().map(|(_, v)| kv_item_height(v.len(), KV_LABEL_SZ, DETAIL_SZ)).fold(0.0_f32, f32::max))
                .sum(),
            DetailRun::Log(items) => items.iter().map(|it| log_item_height(it, DETAIL_SZ)).sum::<f32>() + 2.0,
            DetailRun::Diagnosis(items) => {
                line_h(DETAIL_SZ + 1.0) + items.iter().map(|it| diag_item_height(it, DETAIL_SZ)).sum::<f32>() + DIAG_PAD * 2.0
            }
        }
    };

    let mut measured = 5.0_f32; // верхний паддинг
    measured += line_h(TITLE_SZ);
    measured += summary_lines.len() as f32 * line_h(SUMMARY_SZ);
    if has_body {
        measured += 3.0; // разделитель
        measured += override_lines.len() as f32 * line_h(DETAIL_SZ);
        for run in &runs {
            measured += run_height(run);
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

    // Поверхность карточки — рисуется до содержимого (иначе перекроет текст);
    // высота известна из measured, если карточка целиком умещается на странице.
    if measured <= page_h_usable {
        draw_rounded_fill(&w.layer, p.card_bg.clone(), PDF_MARGIN_L, w.y - measured, PDF_PAGE_W - PDF_MARGIN_R, w.y, RADIUS_BOX);
    }

    w.y -= 5.0;

    // Бейдж — фиксированная позиция у верхнего края карточки, рисуется до
    // заголовка (не зависит от финальной высоты карточки).
    // Раньше правый край бейджа стоял вплотную к рамке карточки (badge_x1
    // = самому краю поля) — визуально «впритык», не как у остальных
    // элементов (draw_row/список/журнал уже давно отступают на
    // CARD_RIGHT_PAD). Выровнено на тот же отступ.
    let badge_w = pt_to_mm(8.5) * 0.6 * label.chars().count() as f32 + 6.0;
    let badge_h = 6.0;
    let badge_x1 = PDF_PAGE_W - PDF_MARGIN_R - CARD_RIGHT_PAD;
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
        draw_rule(&w.layer, p.line_soft.clone(), PDF_MARGIN_L + CARD_INDENT, PDF_PAGE_W - PDF_MARGIN_R - CARD_RIGHT_PAD, w.y - 0.1, w.y + 0.1);
        w.y -= 2.0;
        for line in &override_lines {
            w.draw_line(doc, line, DETAIL_SZ, false, &p.text_muted, CARD_INDENT);
        }
        for run in &runs {
            match run {
                DetailRun::Row(name, value) => {
                    w.draw_row(doc, name, value, DETAIL_SZ, &p.text, &p.text_muted, &p.line_soft, CARD_INDENT, CARD_RIGHT_PAD, ROW_H);
                }
                DetailRun::Text(lines) => {
                    for line in lines {
                        w.draw_line(doc, line, DETAIL_SZ, false, &p.text, CARD_INDENT);
                    }
                }
                DetailRun::KvGrid(items) => {
                    for pair in items.chunks(2) {
                        w.draw_kv_pair(doc, &pair[0], pair.get(1), KV_LABEL_SZ, DETAIL_SZ, &p.text_faint, &p.text, CARD_INDENT, half_col_w, KV_GAP);
                    }
                }
                DetailRun::Log(items) => {
                    // Рамка вокруг всего блока журнала — как .log в референсе;
                    // высота уже известна из run_height(), считаем так же.
                    let right_x = PDF_PAGE_W - PDF_MARGIN_R - CARD_RIGHT_PAD;
                    let box_h = run_height(run);
                    w.ensure_space(doc, box_h);
                    let box_top = w.y;
                    draw_rounded_stroke(&w.layer, p.line_soft.clone(), 0.5, PDF_MARGIN_L + CARD_INDENT, box_top - box_h, right_x, box_top, RADIUS_BOX_SM);
                    w.y -= 1.0;
                    for (i, item) in items.iter().enumerate() {
                        w.draw_log_item(doc, item, i % 2 == 0, DETAIL_SZ, p, CARD_INDENT, LOG_TIME_W, right_x);
                    }
                    w.y -= 1.0;
                }
                DetailRun::Diagnosis(items) => {
                    let right_x = PDF_PAGE_W - PDF_MARGIN_R - CARD_RIGHT_PAD;
                    let box_h = run_height(run);
                    w.ensure_space(doc, box_h);
                    let box_top = w.y;
                    let box_bottom = box_top - box_h;
                    draw_rounded_fill(&w.layer, p.warn_bg.clone(), PDF_MARGIN_L + CARD_INDENT, box_bottom, right_x, box_top, RADIUS_BOX_SM);
                    draw_rounded_stroke(&w.layer, p.warn_border.clone(), 0.5, PDF_MARGIN_L + CARD_INDENT, box_bottom, right_x, box_top, RADIUS_BOX_SM);
                    w.y -= DIAG_PAD;
                    w.draw_line(doc, "⚠ ДИАГНОЗ ПО ШАБЛОНУ СБОЕВ", DETAIL_SZ + 1.0, true, &p.warn_fg, CARD_INDENT + DIAG_PAD);
                    for item in items {
                        w.draw_diag_item(doc, item, DETAIL_SZ, p, CARD_INDENT + DIAG_PAD);
                    }
                    w.y = box_bottom;
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

pub(crate) fn render_pdf(report: &DiagnosticReport) -> Result<Vec<u8>, String> {
    let (doc, page1, layer1) =
        PdfDocument::new("Echips Hardware Check — отчёт", Mm(PDF_PAGE_W), Mm(PDF_PAGE_H), "Layer");
    let font_regular = doc
        .add_external_font(FONT_REGULAR)
        .map_err(|e| format!("Не удалось встроить шрифт: {e}"))?;
    let font_bold = doc
        .add_external_font(FONT_BOLD)
        .map_err(|e| format!("Не удалось встроить шрифт: {e}"))?;
    let font_mono = doc
        .add_external_font(FONT_MONO)
        .map_err(|e| format!("Не удалось встроить шрифт: {e}"))?;
    let layer = doc.get_page(page1).get_layer(layer1);
    let p = palette();
    draw_rule(&layer, p.page_bg.clone(), 0.0, PDF_PAGE_W, 0.0, PDF_PAGE_H);

    let mut w = PdfWriter {
        layer,
        y: PDF_PAGE_H - PDF_MARGIN_TOP,
        font_regular,
        font_bold,
        font_mono,
        footer: format!("Echips Hardware Check · {}", report.device_serial),
        footer_color: p.text_muted.clone(),
        page_no: 0,
    };

    let ok_n = report.results.iter().filter(|r| r.status == "pass").count();
    let bad_n = report.results.iter().filter(|r| r.status == "fail").count();
    let na_n = report.results.len().saturating_sub(ok_n + bad_n);

    // ---- обложка: тёмная плашка с лого, заголовком, метаданными и вердиктом ----
    let cover_top = w.y;
    // +7мм к прежним 78/72 — компенсация новой линии-разделителя над
    // метаданными (добавляет вертикальный отступ), чтобы вердикт-пилюля
    // внизу обложки не оказалась поджатой к самому краю тёмной плашки.
    let cover_h = if bad_n > 0 { 85.0 } else { 79.0 };
    draw_rule(&w.layer, p.cover_bg.clone(), 0.0, PDF_PAGE_W, cover_top - cover_h, cover_top + PDF_MARGIN_TOP);
    // Тёплое радиальное свечение по углам обложки, как .cover::after в
    // референсе (rgba(255,138,0,.20) сверху-справа, .10 снизу-слева) —
    // см. draw_radial_glow. Рисуется поверх плоской заливки, но до любого
    // текста/лого, поэтому ничего не перекрывает.
    let glow = Rgb::new(1.0, 0.541, 0.0, None); // #FF8A00
    // dx/dy — направление сектора СТРОГО внутрь обложки от угла: сверху-
    // справа растёт влево-вниз (-1,-1), снизу-слева — вправо-вверх (1,1).
    draw_radial_glow(&w.layer, &p.cover_bg, &glow, PDF_PAGE_W, cover_top + PDF_MARGIN_TOP, 60.0, 0.20, -1.0, -1.0);
    draw_radial_glow(&w.layer, &p.cover_bg, &glow, 0.0, cover_top - cover_h, 50.0, 0.10, 1.0, 1.0);

    // ---- .brand: знак + "ECHIPS" + плашка "HARDWARE CHECK", одной строкой ----
    let mark_h = 7.0_f32;
    let mark_y = cover_top - 3.0 - mark_h;
    if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(std::io::Cursor::new(MARK_PNG)) {
        if let Ok(mark) = Image::try_from(decoder) {
            let natural_h_mm = MARK_PX / 300.0 * 25.4;
            let scale = mark_h / natural_h_mm;
            mark.add_to_layer(
                w.layer.clone(),
                ImageTransform {
                    translate_x: Some(Mm(PDF_MARGIN_L)),
                    translate_y: Some(Mm(mark_y)),
                    scale_x: Some(scale),
                    scale_y: Some(scale),
                    ..Default::default()
                },
            );
        }
    }
    let word_x = PDF_MARGIN_L + mark_h + 4.0;
    let brand_baseline = mark_y + mark_h / 2.0 - 1.8;
    w.layer.set_fill_color(Color::Rgb(p.cover_text.clone()));
    w.layer.use_text("ECHIPS", 12.5, Mm(word_x), Mm(brand_baseline), &w.font_bold);
    let word_w = pt_to_mm(12.5) * 0.62 * "ECHIPS".chars().count() as f32;
    let kicker_text = "HARDWARE CHECK";
    let kicker_x0 = word_x + word_w + 4.0;
    let kicker_w = pt_to_mm(8.0) * 0.58 * kicker_text.chars().count() as f32 + 8.0;
    let kicker_h = 5.0;
    let kicker_y0 = brand_baseline - kicker_h / 2.0 + 0.8;
    draw_rounded_fill(&w.layer, Rgb::new(0.1742, 0.1353, 0.0953, None), kicker_x0, kicker_y0, kicker_x0 + kicker_w, kicker_y0 + kicker_h, kicker_h / 2.0);
    draw_rounded_stroke(&w.layer, Rgb::new(0.4036, 0.3029, 0.1910, None), 0.35, kicker_x0, kicker_y0, kicker_x0 + kicker_w, kicker_y0 + kicker_h, kicker_h / 2.0);
    w.layer.set_fill_color(Color::Rgb(Rgb::new(1.0, 0.698, 0.349, None)));
    w.layer.use_text(kicker_text.to_string(), 8.0, Mm(kicker_x0 + 4.0), Mm(kicker_y0 + 1.5), &w.font_bold);

    w.y = mark_y - 10.0;
    w.layer.set_fill_color(Color::Rgb(p.cover_text.clone()));
    w.layer.use_text("Отчёт диагностики", 19.0, Mm(PDF_MARGIN_L), Mm(w.y), &w.font_bold);
    w.y -= 7.0;
    w.layer.set_fill_color(Color::Rgb(p.cover_text_muted.clone()));
    w.layer
        .use_text("Полная аппаратная проверка устройства", 10.5, Mm(PDF_MARGIN_L), Mm(w.y), &w.font_regular);

    w.y -= 8.0;
    // Тонкая линия над метаданными — как border-top у .cover-meta в
    // референсе (rgba(255,255,255,.12) поверх тёмной обложки).
    draw_rule(&w.layer, Rgb::new(0.1925, 0.1994, 0.2132, None), PDF_MARGIN_L, PDF_PAGE_W - PDF_MARGIN_R, w.y, w.y + 0.15);
    w.y -= 8.0;
    let meta_col_w = PDF_CONTENT_W / 4.0;
    let time_range = format_time_range(report);
    let duration = format_duration(report);
    let meta: [(&str, String); 4] = [
        ("УСТРОЙСТВО", report.device_model.clone()),
        ("СЕРИЙНЫЙ НОМЕР", report.device_serial.clone()),
        ("НАЧАЛО / ОКОНЧАНИЕ", time_range.unwrap_or_else(|| report.started_at.clone())),
        ("ДЛИТЕЛЬНОСТЬ", duration.unwrap_or_else(|| "—".to_string())),
    ];
    for (i, (label, value)) in meta.iter().enumerate() {
        let x = PDF_MARGIN_L + meta_col_w * i as f32;
        w.layer.set_fill_color(Color::Rgb(p.cover_text_muted.clone()));
        w.layer.use_text(label.to_string(), 8.0, Mm(x), Mm(w.y), &w.font_regular);
        w.layer.set_fill_color(Color::Rgb(p.cover_text.clone()));
        // Серийный номер и диапазон времени — моно-шрифтом, как
        // .cover-meta .item .v.mono в референсе (у модели устройства и
        // длительности класса .mono в разметке нет).
        let mono = i == 1 || i == 2;
        let font = if mono { &w.font_mono } else { &w.font_regular };
        // Длинный серийный номер (26+ символов у реальных плат) уменьшаем по ширине колонки,
        // а не переносим посреди номера; остальное — как раньше.
        let mono_sz = if i == 1 {
            (9.0_f32).min((meta_col_w - 2.0) / (pt_to_mm(1.0) * 0.62 * value.chars().count().max(1) as f32)).max(6.0)
        } else {
            9.0
        };
        let value_lines = if mono { wrap_line(value, max_chars_width_mono(mono_sz, if i == 1 { meta_col_w - 2.0 } else { meta_col_w - 6.0 })) } else { wrap_line(value, max_chars_width(9.5, meta_col_w - 6.0)) };
        let mut vy = w.y - 5.0;
        for line in value_lines.iter().take(2) {
            w.layer.use_text(line.clone(), if mono { mono_sz } else { 9.5 }, Mm(x), Mm(vy), font);
            vy -= line_h(9.5);
        }
    }
    w.y -= 16.0;

    // Вердикт-пилюля — текст согласован с числом (ru_defects_verdict),
    // цветная точка-индикатор слева (.verdict .dot в референсе), цвета —
    // тот же приём смешивания с тёмным фоном, что и у чипов ниже (см.
    // заметку про rgba/альфа): rgba(224,68,50,.16)/.40 для bad,
    // rgba(21,163,101,.16)/.40 для ok, поверх ink-фона обложки.
    let (verdict_text, verdict_fg, verdict_bg, verdict_border) = if bad_n > 0 {
        (
            ru_defects_verdict(bad_n),
            Rgb::new(1.0, 0.620, 0.561, None),   // #FF9E8F
            Rgb::new(0.2097, 0.1185, 0.1204, None),
            Rgb::new(0.4008, 0.1608, 0.1420, None),
        )
    } else {
        (
            "Неисправностей не выявлено".to_string(),
            Rgb::new(0.498, 0.890, 0.667, None), // #7FE3AA
            Rgb::new(0.0824, 0.1781, 0.1524, None),
            Rgb::new(0.0824, 0.3098, 0.2219, None),
        )
    };
    let verdict_w = pt_to_mm(10.5) * 0.55 * verdict_text.chars().count() as f32 + 20.0;
    draw_rounded_fill(&w.layer, verdict_bg, PDF_MARGIN_L, w.y - 6.0, PDF_MARGIN_L + verdict_w, w.y + 3.0, 4.5);
    draw_rounded_stroke(&w.layer, verdict_border, 0.4, PDF_MARGIN_L, w.y - 6.0, PDF_MARGIN_L + verdict_w, w.y + 3.0, 4.5);
    draw_circle_fill(&w.layer, verdict_fg.clone(), PDF_MARGIN_L + 6.0, w.y - 1.5, 0.8);
    w.layer.set_fill_color(Color::Rgb(verdict_fg));
    w.layer.use_text(verdict_text, 10.5, Mm(PDF_MARGIN_L + 10.0), Mm(w.y - 3.2), &w.font_bold);

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
    let verdict_title = if failed.is_empty() { "Неисправностей не выявлено".to_string() } else { ru_defects_verdict(failed.len()) };
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
            draw_rounded_fill(&w.layer, p.page_bg.clone(), x, row_top - CHIP_H, x + w_chip, row_top, CHIP_H / 2.0);
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
