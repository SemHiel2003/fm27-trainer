//! Ontwerpsysteem: lettertype (Inter + Phosphor-iconen), kleuren en bouwstenen.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Response, RichText,
    Sense, Shadow, Stroke, TextStyle, Ui, Vec2, vec2,
};
use std::sync::Arc;

pub use egui_phosphor::regular as icon;

// ---- kleuren ------------------------------------------------------------------
pub const BG: Color32 = Color32::from_rgb(9, 10, 14);
pub const SIDEBAR: Color32 = Color32::from_rgb(12, 13, 18);
pub const SURFACE: Color32 = Color32::from_rgb(18, 20, 27);
pub const SURFACE2: Color32 = Color32::from_rgb(27, 30, 40);
pub const HOVER: Color32 = Color32::from_rgb(36, 40, 53);
pub const BORDER: Color32 = Color32::from_rgb(32, 36, 47);
pub const TRACK: Color32 = Color32::from_rgb(31, 35, 46);
pub const TEXT: Color32 = Color32::from_rgb(238, 240, 245);
pub const TEXT_DIM: Color32 = Color32::from_rgb(156, 163, 179);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(104, 111, 128);
pub const ACCENT: Color32 = Color32::from_rgb(126, 111, 255);
pub const ACCENT_HI: Color32 = Color32::from_rgb(150, 138, 255);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(33, 30, 64);
pub const SUCCESS: Color32 = Color32::from_rgb(61, 214, 140);
pub const WARNING: Color32 = Color32::from_rgb(245, 181, 68);
pub const DANGER: Color32 = Color32::from_rgb(255, 92, 108);
pub const INFO: Color32 = Color32::from_rgb(76, 178, 255);

/// Mengt twee kleuren (t = 0: a, t = 1: b).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

// ---- beweging -------------------------------------------------------------------
/// Duur van een hover-/keuze-overgang.
const QUICK: f32 = 0.14;

fn enter_id() -> egui::Id {
    egui::Id::new("fm27-page-enter")
}

/// Start de intrede-animatie opnieuw (bij het wisselen van pagina of na een nieuwe uitlezing).
pub fn restart_enter(ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(enter_id(), now));
}

/// Voortgang van de intrede-animatie over `secs` seconden, 0…1 met een zachte uitloop. Vraagt zelf
/// om een volgend beeld zolang hij loopt.
pub fn enter(ctx: &egui::Context, secs: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| *d.get_temp_mut_or(enter_id(), now));
    let t = (((now - start) as f32) / secs).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    1.0 - (1.0 - t).powi(3)
}

pub fn tint(c: Color32, a: f32) -> Color32 {
    // kleur gemengd met de kaartachtergrond (geen echte transparantie: rustiger in tabellen)
    let m = |x: u8, y: u8| (x as f32 * a + y as f32 * (1.0 - a)) as u8;
    Color32::from_rgb(m(c.r(), SURFACE.r()), m(c.g(), SURFACE.g()), m(c.b(), SURFACE.b()))
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

// ---- lettertypen ----------------------------------------------------------------
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// Icoon als tekst (in de iconenfamilie).
pub fn ic(s: &str) -> RichText {
    RichText::new(s).family(icons())
}

/// Icoon + tekst in één label (twee lettertypes).
pub fn icon_label(icon_s: &str, text: &str, font: FontId, color: Color32) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    let fmt = |f: FontId| TextFormat { font_id: f, color, valign: egui::Align::Center, ..Default::default() };
    if !icon_s.is_empty() {
        job.append(icon_s, 0.0, fmt(FontId::new(font.size + 2.0, icons())));
        job.append("  ", 0.0, fmt(font.clone()));
    }
    job.append(text, 0.0, fmt(font));
    job
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "Inter-Regular", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add(&mut fonts, "Inter-Medium", include_bytes!("../assets/fonts/Inter-Medium.ttf"));
    add(&mut fonts, "Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"));
    add(&mut fonts, "Inter-Bold", include_bytes!("../assets/fonts/Inter-Bold.ttf"));
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |main: &str| {
        let mut v = vec![main.to_string(), "phosphor".to_string()];
        v.extend(fallback.iter().filter(|f| *f != "phosphor").cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, family("Inter-Regular"));
    fonts.families.insert(medium(), family("Inter-Medium"));
    fonts.families.insert(semibold(), family("Inter-SemiBold"));
    fonts.families.insert(bold(), family("Inter-Bold"));
    // Iconen apart: Inter heeft eigen tekens in hetzelfde privégebied (U+E000…) als Phosphor,
    // en Phosphor heeft lege tekens voor letters. Dus een eigen familie met Phosphor voorop.
    fonts.families.insert(icons(), vec!["phosphor".to_string(), "Inter-Regular".to_string()]);
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(24.0, bold())),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, medium())),
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = vec2(8.0, 8.0);
        s.button_padding = vec2(14.0, 8.0);
        s.interact_size = vec2(36.0, 32.0);
        s.slider_width = 160.0;
        s.combo_width = 160.0;
        s.window_margin = Margin::same(16);
        s.menu_margin = Margin::same(8);
        s.scroll.bar_width = 8.0;
        s.scroll.floating = true;
        style.animation_time = 0.14;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = Color32::from_rgb(8, 9, 12);
        v.faint_bg_color = Color32::from_rgb(22, 25, 33);
        v.code_bg_color = SURFACE2;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_shadow = Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(100) };
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = ACCENT;
        v.striped = false;
        let r = CornerRadius::same(9);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = SURFACE;
        w.noninteractive.weak_bg_fill = SURFACE;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = SURFACE2;
        w.inactive.weak_bg_fill = SURFACE2;
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(58, 64, 82));
        w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = ACCENT_SOFT;
        w.active.weak_bg_fill = ACCENT_SOFT;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.active.corner_radius = r;
        w.active.expansion = 0.0;
        w.open = w.active;
    });
}

// ---- tekst ----------------------------------------------------------------------
pub fn title(s: impl Into<String>) -> RichText {
    RichText::new(s).font(FontId::new(27.0, bold())).color(TEXT)
}
pub fn h2(s: impl Into<String>) -> RichText {
    RichText::new(s).font(FontId::new(16.0, semibold())).color(TEXT)
}
pub fn label_strong(s: impl Into<String>) -> RichText {
    RichText::new(s).font(FontId::new(14.0, semibold())).color(TEXT)
}
pub fn dim(s: impl Into<String>) -> RichText {
    RichText::new(s).color(TEXT_DIM)
}
pub fn faint(s: impl Into<String>) -> RichText {
    RichText::new(s).size(12.0).color(TEXT_FAINT)
}
pub fn caps(s: impl Into<String>) -> RichText {
    RichText::new(s.into().to_uppercase()).font(FontId::new(11.0, semibold())).color(TEXT_FAINT)
}

// ---- bouwstenen -----------------------------------------------------------------
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Paginakop: titel + ondertitel links, acties rechts.
pub fn page_header(ui: &mut Ui, title_text: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(title(title_text));
            if !subtitle.is_empty() {
                ui.label(dim(subtitle));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
    });
    ui.add_space(18.0);
}

pub fn progress(ui: &mut Ui, value: f32, height: f32, color: Color32) {
    let w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let p = ui.painter();
    let r = CornerRadius::same((height / 2.0) as u8);
    p.rect_filled(rect, r, TRACK);
    // de balk groeit bij het openen van de pagina en glijdt mee als de waarde verandert
    let shown = ui.ctx().animate_value_with_time(ui.next_auto_id().with("bar"), value, 0.35) * enter(ui.ctx(), 0.6);
    let mut f = rect;
    f.set_width((rect.width() * (shown / 100.0).clamp(0.0, 1.0)).max(if value > 0.0 { height } else { 0.0 }));
    p.rect_filled(f, r, color);
}

/// Ring die met de klok mee vult vanaf 12 uur (0…100).
pub fn ring(ui: &mut Ui, value: f32, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    let (c, w) = (rect.center(), (size * 0.11).max(3.0));
    let r = size / 2.0 - w / 2.0;
    p.circle_stroke(c, r, Stroke::new(w, TRACK));
    let shown = ui.ctx().animate_value_with_time(ui.next_auto_id().with("ring"), value, 0.35) * enter(ui.ctx(), 0.8);
    let sweep = (shown / 100.0).clamp(0.0, 1.0) * std::f32::consts::TAU;
    if sweep > 0.001 {
        let n = ((sweep / std::f32::consts::TAU) * 96.0).ceil().max(2.0) as usize;
        let at = |a: f32| c + vec2(a.sin(), -a.cos()) * r;
        let pts: Vec<egui::Pos2> = (0..=n).map(|i| at(sweep * i as f32 / n as f32)).collect();
        p.circle_filled(pts[0], w / 2.0, color);
        p.circle_filled(*pts.last().unwrap(), w / 2.0, color);
        p.add(egui::Shape::line(pts, Stroke::new(w, color)));
    }
}

/// Tegel met een groot getal; met `bar` een ring rechts die de waarde toont.
pub fn stat_tile(ui: &mut Ui, icon_s: &str, label: &str, value: &str, sub: &str, color: Color32, bar: Option<f32>) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(ic(icon_s).size(15.0).color(color));
            ui.add(egui::Label::new(caps(label)).truncate());
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).font(FontId::new(32.0, bold())).color(TEXT));
                if !sub.is_empty() {
                    ui.label(RichText::new(sub).size(12.5).color(if bar.is_some() { color } else { TEXT_FAINT }));
                }
            });
            if let Some(b) = bar {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ring(ui, b, 54.0, color));
            }
        });
    });
}

/// Klein gekleurd label.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) -> Response {
    Frame::new()
        .fill(tint(color, 0.18))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| ui.add(egui::Label::new(RichText::new(text).font(FontId::new(12.0, medium())).color(color)).extend()))
        .response
}

pub fn primary_button(ui: &mut Ui, icon_s: &str, text: &str, enabled: bool) -> Response {
    let job = icon_label(icon_s, text, FontId::new(14.0, semibold()), if enabled { Color32::WHITE } else { TEXT_FAINT });
    let galley = ui.fonts(|f| f.layout_job(job));
    let size = vec2(galley.size().x + 32.0, 36.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), enabled && resp.hovered(), QUICK);
    let down = enabled && resp.is_pointer_button_down_on();
    let fill = if !enabled { SURFACE2 } else if down { mix(ACCENT, BG, 0.18) } else { mix(ACCENT, ACCENT_HI, hover) };
    let p = ui.painter();
    if enabled {
        // zachte gloed onder de knop
        p.rect_filled(rect.translate(vec2(0.0, 3.0)).shrink2(vec2(6.0, 2.0)), CornerRadius::same(12), mix(BG, ACCENT, 0.16 + 0.12 * hover));
    }
    p.rect_filled(rect, CornerRadius::same(10), fill);
    p.galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

pub fn button(ui: &mut Ui, icon_s: &str, text: &str, enabled: bool) -> Response {
    let col = if enabled { TEXT } else { TEXT_FAINT };
    let b = egui::Button::new(icon_label(icon_s, text, FontId::new(14.0, medium()), col))
        .corner_radius(CornerRadius::same(10))
        .min_size(vec2(0.0, 36.0));
    ui.add_enabled(enabled, b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn small_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let b = egui::Button::new(RichText::new(text).size(12.5).font(FontId::new(12.5, medium())))
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 26.0));
    ui.add_enabled(enabled, b)
}

/// Item in de zijbalk.
pub fn nav_item(ui: &mut Ui, icon_s: &str, label: &str, selected: bool) -> Response {
    let size = vec2(ui.available_width(), 38.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let sel = ui.ctx().animate_bool_with_time(resp.id.with("s"), selected, 0.2);
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered() && !selected, QUICK);
    let p = ui.painter();
    let fill = mix(mix(SIDEBAR, SURFACE2, hov), ACCENT_SOFT, sel);
    p.rect_filled(rect, CornerRadius::same(10), fill);
    if sel > 0.01 {
        let h = 18.0 * sel;
        let bar = egui::Rect::from_center_size(rect.left_center() + vec2(1.5, 0.0), vec2(3.0, h));
        p.rect_filled(bar, CornerRadius::same(2), ACCENT);
    }
    let col = mix(mix(TEXT_DIM, TEXT, hov), Color32::WHITE, sel);
    p.text(rect.left_center() + vec2(16.0, 0.0), egui::Align2::LEFT_CENTER, icon_s, FontId::new(18.0, icons()), mix(col, ACCENT_HI, sel));
    p.text(rect.left_center() + vec2(46.0, 0.0), egui::Align2::LEFT_CENTER, label, FontId::new(14.0, medium()), col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Gesegmenteerde keuze (bv. NL | EN, Week | Seizoen). Zelf getekend op een vaste maat, zodat
/// hij in elke omringende layout (ook rechts-naar-links) hetzelfde uitziet.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let font = FontId::new(13.0, medium());
    let (pad, h, gap) = (12.0, 32.0, 2.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| ui.fonts(|f| f.layout_no_wrap(l.to_string(), font.clone(), TEXT).size().x) + 2.0 * pad)
        .collect();
    let total = widths.iter().sum::<f32>() + 6.0 + gap * (options.len().saturating_sub(1)) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::same(10), SURFACE2);
    let mut x = rect.left() + 3.0;
    let mut changed = false;
    for ((v, label), w) in options.iter().zip(widths) {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), vec2(w, h - 6.0));
        let resp = ui.interact(r, ui.id().with(("segmented", *label)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let sel = *value == *v;
        if sel {
            p.rect_filled(r, CornerRadius::same(8), ACCENT);
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(8), HOVER);
        }
        p.text(r.center(), egui::Align2::CENTER_CENTER, *label, font.clone(), if sel { Color32::WHITE } else { TEXT_DIM });
        if resp.clicked() && !sel {
            *value = *v;
            changed = true;
        }
        x += w + gap;
    }
    changed
}

/// Sessie als gekleurde chip.
pub fn session_chip(ui: &mut Ui, text: &str, color: Color32, highlight: bool) -> Response {
    Frame::new()
        .fill(tint(color, if highlight { 0.30 } else { 0.14 }))
        .stroke(if highlight { Stroke::new(1.0, color) } else { Stroke::NONE })
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).font(FontId::new(11.5, medium())).color(TEXT)).truncate())
        })
        .response
}

pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 5.0, mix(SURFACE, color, 0.28));
    ui.painter().circle_filled(r.center(), 3.2, color);
}

/// Rij met naam, waarde en een dunne balk (voor de factors).
pub fn metric_row(ui: &mut Ui, name: &str, value: f32, color: Color32) {
    let w = ui.available_width();
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2((w - 44.0).max(40.0), 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(name).size(13.0).color(TEXT_DIM)).truncate()).on_hover_text(name);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("{value:.0}")).font(FontId::new(13.5, semibold())).color(color));
        });
    });
    progress(ui, value, 5.0, color);
    ui.add_space(4.0);
}

/// Aan/uit-schakelaar.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(46.0, 26.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let p = ui.painter();
    let bg = mix(Color32::from_rgb(52, 57, 72), SUCCESS, t);
    p.rect_filled(rect, CornerRadius::same(13), bg);
    let x = egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), t);
    p.circle_filled(egui::pos2(x, rect.center().y), 10.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Kleine statistiek: label boven, waarde eronder.
pub fn mini_stat(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(caps(label));
        ui.label(RichText::new(value).font(FontId::new(17.0, semibold())).color(TEXT));
    });
}
