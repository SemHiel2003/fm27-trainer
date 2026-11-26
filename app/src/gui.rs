//! De app: zijbalk + pagina's. Alle teksten via `t!` (NL/EN), alle stijl via `theme`.

use anyhow::{Result, anyhow};
use chrono::{Duration, NaiveDate, Weekday};
use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, Ui, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::agent;
use crate::export;
use crate::i18n::{self, Lang};
use crate::live::LiveState;
use crate::live_training;
use crate::model::{Component, Effectiveness, Factor, category_of, level_color, level_name, session_by_code};
use crate::planner::{self, CalendarOptions, CalendarWeek, PlannerSettings, WeekKind, WeekPlan, unique_schedules};
use crate::t;
use crate::theme::{self, icon};
use crate::vision;

pub fn run() -> Result<()> {
    // testhulp: FM27_WINDOW=1320x1400 voor hogere schermafbeeldingen
    let size = std::env::var("FM27_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?])
        })
        .unwrap_or([1320.0, 880.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FM27 Trainer")
            .with_inner_size(size)
            .with_min_inner_size([1040.0, 660.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon_256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "FM27 Trainer",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
    .map_err(|e| anyhow!("{e}"))
}

// ------------------------------------------------------------------------ instellingen

#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    lang: Lang,
    rate: f32,
    capture_delay: bool,
    #[serde(default = "yes")]
    auto_training: bool,
    /// Aanpassen in FM toestaan op een build die nog niet live getest is.
    #[serde(default)]
    allow_untested: bool,
    /// Tactisch begrip op 100 % houden zolang de app open is (FM laat het wekelijks zakken).
    #[serde(default)]
    keep_understanding: bool,
    /// De Breakdown elke zoveel seconden zelf opnieuw uitlezen zolang hij in FM open staat
    /// (idee en eerste versie: arhxmmmmm, fork forked-fm27-trainer).
    #[serde(default)]
    auto_read: bool,
    #[serde(default = "ten")]
    auto_read_secs: u32,
    /// Bij meer dan één manager in de save: de UID van de club van de manager die de app bedient.
    #[serde(default)]
    manager_club: Option<u32>,
    /// Het team waarvan de app de training plant (plek in de teamlijst van de club; None = eigen team).
    #[serde(default)]
    training_team: Option<usize>,
    planner: PlannerSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { lang: i18n::detect(), rate: 1.17, capture_delay: true, auto_training: true, allow_untested: false, keep_understanding: false, auto_read: false, auto_read_secs: 10, manager_club: None, training_team: None, planner: PlannerSettings::default() }
    }
}

fn yes() -> bool {
    true
}

fn ten() -> u32 {
    10
}

/// Zelfde waarden (op een halve punt na) als bij de vorige uitlezing.
fn same_values(a: &Effectiveness, b: &Effectiveness) -> bool {
    Factor::ALL.iter().all(|f| (a.get(*f) - b.get(*f)).abs() < 0.5) && Component::ALL.iter().all(|c| (a.component(*c) - b.component(*c)).abs() < 0.5)
}

fn settings_file() -> PathBuf {
    crate::data_dir().join("settings.json")
}

fn calendar_file() -> PathBuf {
    crate::data_dir().join("calendar.json")
}

// ------------------------------------------------------------------------ app

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Overview,
    Training,
    Planning,
    Club,
    Squad,
    Tactics,
    Sell,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum PlanTab {
    Week,
    Season,
}

type AnalysisResult = Result<Reading, String>;

/// Uitkomst van een uitlezing.
enum Reading {
    Fresh(vision::Analysis, String),
    /// automatisch uitlezen: FM zag er hetzelfde uit als de vorige keer, er is niets gelezen
    /// (`true`: de vorige keer was een volledige Breakdown)
    Unchanged(bool),
}

struct App {
    page: Page,
    /// de pagina die het vorige beeld toonde (voor de intrede-animatie)
    shown_page: Option<Page>,
    settings: Settings,
    eff: Effectiveness,
    status: String,
    status_error: bool,
    analysis_rx: Option<mpsc::Receiver<AnalysisResult>>,
    /// de lopende uitlezing is door automatisch uitlezen gestart (stil, haalt FM niet naar voren)
    auto_pending: bool,
    auto_note: String,
    last_auto: std::time::Instant,
    image_path: String,
    show_file: bool,
    edit_mode: bool,
    plan_tab: PlanTab,
    week_start: String,
    week_matches: [bool; 7],
    prev_sunday: bool,
    next_monday: bool,
    week_kind: WeekKind,
    week_plan: Option<WeekPlan>,
    cal_opts: CalendarOptions,
    year_start: String,
    calendar: Vec<CalendarWeek>,
    year: Vec<WeekPlan>,
    live: LiveState,
    /// de tactiek die de tactiekpagina toont (None: de tactiek die in FM in gebruik is)
    tactic_pick: Option<usize>,
    tactics_asked: bool,
    frames: u32,
}

impl App {
    fn new() -> Self {
        let mut settings: Settings = std::fs::read_to_string(settings_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        match std::env::var("FM27_LANG").as_deref() {
            Ok("nl") => settings.lang = Lang::Nl,
            Ok("en") => settings.lang = Lang::En,
            _ => {}
        }
        i18n::set(settings.lang);
        let cal_opts = CalendarOptions::default();
        let calendar = std::fs::read_to_string(calendar_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| planner::default_calendar(&cal_opts));
        let monday = planner::monday_of(chrono::Local::now().date_naive());
        let mut week_matches = [false; 7];
        week_matches[6] = true;
        // FM27_PAGE=training|planning|club|squad|sell|buy|settings opent direct die pagina (handig bij testen)
        let page = match std::env::var("FM27_PAGE").unwrap_or_default().as_str() {
            "training" => Page::Training,
            "planning" => Page::Planning,
            "club" => Page::Club,
            "squad" => Page::Squad,
            "tactics" => Page::Tactics,
            "sell" | "buy" => Page::Sell,
            "settings" => Page::Settings,
            _ => Page::Overview,
        };
        App {
            page,
            shown_page: None,
            eff: crate::load_effectiveness().unwrap_or_default(),
            settings,
            status: String::new(),
            status_error: false,
            analysis_rx: None,
            auto_pending: false,
            auto_note: String::new(),
            last_auto: std::time::Instant::now(),
            image_path: String::new(),
            show_file: false,
            edit_mode: false,
            plan_tab: PlanTab::Week,
            week_start: monday.format("%Y-%m-%d").to_string(),
            week_matches,
            prev_sunday: true,
            next_monday: false,
            week_kind: WeekKind::Season,
            week_plan: None,
            year_start: cal_opts.start.format("%Y-%m-%d").to_string(),
            cal_opts,
            calendar,
            year: Vec::new(),
            tactic_pick: None,
            tactics_asked: false,
            live: {
                let mut l = LiveState::default();
                l.sell.buy = std::env::var("FM27_PAGE").as_deref() == Ok("buy");
                l
            },
            frames: 0,
        }
    }

    /// Testhulp: FM27_SNAPSHOT=pad.png legt na het laden het eigen venster vast en sluit af.
    fn snapshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("FM27_SNAPSHOT") else { return };
        self.frames += 1;
        if self.frames == 90 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = buf.save(&path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    fn save_settings(&self) {
        let _ = std::fs::create_dir_all(crate::data_dir());
        if let Ok(s) = serde_json::to_string_pretty(&self.settings) {
            let _ = std::fs::write(settings_file(), s);
        }
    }

    fn save_calendar(&self) {
        let _ = std::fs::create_dir_all(crate::data_dir());
        if let Ok(s) = serde_json::to_string_pretty(&self.calendar) {
            let _ = std::fs::write(calendar_file(), s);
        }
    }

    fn set_status(&mut self, msg: impl Into<String>, error: bool) {
        self.status = msg.into();
        self.status_error = error;
    }

    fn has_data(&self) -> bool {
        !self.eff.source.is_empty()
    }

    // -------------------------------------------------------------------- analyse

    fn start_analysis(&mut self, ctx: &egui::Context, file: Option<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        let auto = self.auto_pending;
        let delay = self.settings.capture_delay && file.is_none() && !auto;
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            if delay && cfg!(target_os = "macos") {
                let _ = std::process::Command::new("open").args(["-b", "com.SportsInteractive.FootballManager27"]).status();
                std::thread::sleep(std::time::Duration::from_millis(1500));
            }
            let screen = || t!("FM27-scherm", "FM27 screen").to_string();
            let res: AnalysisResult = match file {
                Some(p) => vision::analyze_file(&p).map(|(a, _)| Reading::Fresh(a, p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())).map_err(|e| format!("{e:#}")),
                // automatisch: zonder focus te verplaatsen, en alleen lezen als het scherm veranderd is
                None if auto => vision::analyze_screen_auto()
                    .map(|r| match r {
                        vision::AutoRead::Unchanged { complete } => Reading::Unchanged(complete),
                        vision::AutoRead::Read(a) => Reading::Fresh(a, screen()),
                    })
                    .map_err(|e| format!("{e:#}")),
                None => vision::analyze_screen().map(|(a, _)| Reading::Fresh(a, screen())).map_err(|e| format!("{e:#}")),
            };
            let _ = tx.send(res);
            ctx2.request_repaint();
        });
        self.analysis_rx = Some(rx);
        if !self.auto_pending {
            self.set_status(t!("Bezig met uitlezen…", "Reading…"), false);
        }
    }

    fn poll_analysis(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.analysis_rx else { return };
        if let Ok(res) = rx.try_recv() {
            self.analysis_rx = None;
            let _ = ctx;
            let auto = std::mem::take(&mut self.auto_pending);
            match res {
                // automatisch: alleen een volledige, zekere uitlezing telt, en alleen als er iets veranderd is
                Ok(Reading::Unchanged(true)) => {
                    self.auto_note = t!(format!("geen verandering ({})", chrono::Local::now().format("%H:%M:%S")), format!("no change ({})", chrono::Local::now().format("%H:%M:%S"))).to_string();
                }
                Ok(Reading::Unchanged(false)) => {
                    self.auto_note = t!("wacht op de Breakdown in FM", "waiting for the Breakdown in FM").to_string();
                }
                Ok(Reading::Fresh(a, _)) if auto && (a.gauges.len() < 17 || a.components.len() < 4 || a.gauges.iter().any(|g| g.doubt)) => {
                    self.auto_note = t!("wacht op de Breakdown in FM", "waiting for the Breakdown in FM").to_string();
                }
                Ok(Reading::Fresh(a, _)) if auto && same_values(&self.eff, &a.to_effectiveness("")) => {
                    self.auto_note = t!(format!("geen verandering ({})", chrono::Local::now().format("%H:%M:%S")), format!("no change ({})", chrono::Local::now().format("%H:%M:%S"))).to_string();
                }
                Err(_) if auto => {
                    self.auto_note = t!("wacht op de Breakdown in FM", "waiting for the Breakdown in FM").to_string();
                }
                Ok(Reading::Fresh(a, src)) => {
                    if auto {
                        self.auto_note = t!(format!("bijgewerkt ({})", chrono::Local::now().format("%H:%M:%S")), format!("updated ({})", chrono::Local::now().format("%H:%M:%S"))).to_string();
                    }
                    let src_is_screen = src == t!("FM27-scherm", "FM27 screen");
                    let when = chrono::Local::now().format("%d-%m %H:%M");
                    self.eff = a.to_effectiveness(&format!("{src} · {when}"));
                    // spel-datum erbij, zodat de app weet hoe oud de meting in speltijd is
                    if let Ok(v) = agent::request(json!({"cmd": "game_date"})) {
                        let (y, d) = (v.get("year").and_then(|x| x.as_i64()), v.get("doy").and_then(|x| x.as_u64()));
                        if let (Some(y), Some(d)) = (y, d) {
                            self.eff.game_date = NaiveDate::from_yo_opt(y as i32, d as u32);
                        }
                    }
                    let _ = crate::save_effectiveness(&self.eff);
                    let mut msg = t!(format!("{} van 17 meters gelezen.", a.gauges.len()), format!("Read {} of 17 gauges.", a.gauges.len())).to_string();
                    if !a.lang.starts_with("english") {
                        msg.push_str(&format!(" (FM: {})", a.lang));
                    }
                    // een meting die niet bij de kleur van de meter past: laten nakijken
                    let doubts: Vec<&str> = a.gauges.iter().filter(|g| g.doubt).map(|g| g.factor.label()).collect();
                    if !doubts.is_empty() {
                        msg.push(' ');
                        msg.push_str(&t!(
                            format!("Kijk na (kleur en vulling kloppen niet): {}. Pas aan met 'Waarden handmatig aanpassen'.", doubts.join(", ")),
                            format!("Please check (colour and fill disagree): {}. Fix them with 'Adjust values manually'.", doubts.join(", "))
                        ));
                    }
                    let incomplete = a.gauges.len() < 17 || a.components.len() < 4 || !doubts.is_empty();
                    if incomplete && src_is_screen {
                        // zonder de opname is een verkeerde uitlezing niet na te spelen
                        msg.push(' ');
                        msg.push_str(t!(
                            "Klopt het niet? Stuur last-breakdown.png en last-analysis.txt uit de app-map (Instellingen → Open app-map) mee met je melding.",
                            "Not right? Attach last-breakdown.png and last-analysis.txt from the app folder (Settings → Open app folder) to your report."
                        ));
                    }
                    self.set_status(msg, incomplete);
                    self.week_plan = None;
                    self.year.clear();
                }
                Err(e) => self.set_status(e, true),
            }
        }
    }

    // -------------------------------------------------------------------- shell

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(236.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::SIDEBAR).inner_margin(Margin { left: 14, right: 14, top: 20, bottom: 16 }))
            .show(ctx, |ui| {
                // haarlijn tussen zijbalk en inhoud
                let edge = ui.max_rect().expand2(vec2(14.0, 20.0));
                ui.painter().vline(edge.right() - 0.5, edge.y_range(), Stroke::new(1.0, theme::BORDER));
                ui.horizontal(|ui| {
                    Frame::new().fill(theme::ACCENT).corner_radius(CornerRadius::same(11)).inner_margin(Margin::same(7)).show(ui, |ui| {
                        ui.label(theme::ic(icon::SOCCER_BALL).size(20.0).color(Color32::WHITE));
                    });
                    ui.vertical(|ui| {
                        ui.add_space(1.0);
                        ui.label(RichText::new("FM27 Trainer").font(egui::FontId::new(16.0, theme::bold())).color(theme::TEXT));
                        ui.label(theme::faint("Tactical Effectiveness"));
                    });
                });
                ui.add_space(22.0);
                ui.label(theme::caps("Menu"));
                ui.add_space(4.0);
                let items = [
                    (Page::Overview, icon::GAUGE, t!("Overzicht", "Overview")),
                    (Page::Training, icon::BARBELL, "Training"),
                    (Page::Planning, icon::CALENDAR_DOTS, "Planning"),
                    (Page::Club, icon::BANK, "Club"),
                    (Page::Squad, icon::USERS_THREE, t!("Selectie", "Squad")),
                    (Page::Tactics, icon::STRATEGY, t!("Tactiek", "Tactics")),
                    (Page::Sell, icon::HANDSHAKE, "Transfers"),
                    (Page::Settings, icon::GEAR_SIX, t!("Instellingen", "Settings")),
                ];
                for (p, ic, label) in items {
                    if theme::nav_item(ui, ic, label, self.page == p).clicked() {
                        self.page = p;
                    }
                    ui.add_space(2.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(theme::faint(format!("v{}", env!("CARGO_PKG_VERSION"))));
                    ui.add_space(6.0);
                    let mut lang = self.settings.lang;
                    if theme::segmented(ui, &mut lang, &[(Lang::Nl, "Nederlands"), (Lang::En, "English")]) {
                        self.settings.lang = lang;
                        i18n::set(lang);
                        self.save_settings();
                    }
                    ui.add_space(10.0);
                    Frame::new().fill(theme::SURFACE).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (c, txt) = if !agent::LIVE_SUPPORTED {
                                (theme::TEXT_FAINT, t!("Live: alleen macOS", "Live: macOS only"))
                            } else if self.live.connected {
                                (theme::SUCCESS, t!("Verbonden met FM27", "Connected to FM27"))
                            } else {
                                (theme::TEXT_FAINT, t!("Niet verbonden", "Not connected"))
                            };
                            theme::status_dot(ui, c);
                            ui.label(RichText::new(txt).size(13.0).color(theme::TEXT));
                        });
                        if let Some(n) = self.live.club_name() {
                            ui.label(theme::faint(n));
                        }
                    });
                });
            });
    }

    fn status_line(&self, ui: &mut Ui, msg: &str, error: bool) {
        if msg.is_empty() {
            return;
        }
        let (c, ic) = if error { (theme::DANGER, icon::WARNING) } else { (theme::INFO, icon::CHECK_CIRCLE) };
        Frame::new().fill(theme::tint(c, 0.12)).stroke(Stroke::new(1.0, theme::tint(c, 0.4))).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::ic(ic).color(c));
                ui.label(RichText::new(msg).color(theme::TEXT));
            });
        });
        ui.add_space(12.0);
    }

    /// Kaart met stappen om FM met de module te starten.
    fn connect_card(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        theme::card(ui, |ui| {
            if !agent::LIVE_SUPPORTED {
                ui.label(theme::h2(t!("Live in FM is niet beschikbaar op dit systeem", "Live in FM is not available on this system")));
                ui.label(theme::dim(t!("Uitlezen, planning en export werken wel.", "Reading, planning and export do work.")));
                return;
            }
            let windows = cfg!(windows);
            ui.horizontal(|ui| {
                ui.label(theme::ic(icon::PLUGS_CONNECTED).size(28.0).color(theme::ACCENT));
                ui.vertical(|ui| {
                    ui.label(theme::h2(t!("Verbind met Football Manager", "Connect to Football Manager")));
                    ui.label(theme::dim(if windows {
                        t!(
                            "De app leest FM27 rechtstreeks terwijl het draait. Je hoeft niets te installeren.",
                            "The app reads FM27 directly while it runs. Nothing to install."
                        )
                    } else {
                        t!(
                            "De app praat rechtstreeks met FM27 via een kleine module die met FM meestart.",
                            "The app talks to FM27 directly through a small module that starts with FM."
                        )
                    }));
                });
            });
            ui.add_space(10.0);
            let steps = if windows {
                [
                    (1, t!("Start FM27 op de pc: Steam (of de knop hieronder), Xbox-app / Game Pass of Epic.", "Start FM27 on your PC: Steam (or the button below), the Xbox app / Game Pass or Epic.")),
                    (2, t!("Laad je save.", "Load your save.")),
                    (3, t!("De app verbindt vanzelf.", "The app connects automatically.")),
                ]
            } else {
                [
                    (1, t!("Sla je spel op en sluit FM27.", "Save your game and quit FM27.")),
                    (2, t!("Klik op 'Start FM met module' (Steam moet open zijn).", "Click 'Start FM with module' (Steam must be open).")),
                    (3, t!("Laad je save. De app verbindt vanzelf.", "Load your save. The app connects automatically.")),
                ]
            };
            for (n, s) in steps {
                ui.horizontal(|ui| {
                    Frame::new().fill(theme::ACCENT_SOFT).corner_radius(CornerRadius::same(20)).inner_margin(Margin::symmetric(8, 2)).show(ui, |ui| {
                        ui.label(RichText::new(n.to_string()).color(theme::ACCENT).strong());
                    });
                    ui.label(s);
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let start = if windows { t!("Start FM27", "Start FM27") } else { t!("Start FM met module", "Start FM with module") };
                if theme::primary_button(ui, icon::PLAY, start, !self.live.busy()).clicked() {
                    self.live.start_fm(ctx);
                }
                if theme::button(ui, icon::ARROW_COUNTER_CLOCKWISE, t!("Opnieuw verbinden", "Reconnect"), !self.live.busy()).clicked() {
                    self.live.refresh(ctx, t!("verbinden", "connect"), None);
                }
                if self.live.busy() {
                    ui.spinner();
                }
            });
        });
    }

    // -------------------------------------------------------------------- overzicht

    fn page_overview(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let subtitle = if self.has_data() {
            t!(format!("Laatst uitgelezen: {}", self.eff.source), format!("Last read: {}", self.eff.source))
        } else {
            t!("Open in FM27 de Breakdown van Tactical Effectiveness en lees hem uit.", "Open the Tactical Effectiveness Breakdown in FM27 and read it.").to_string()
        };
        let busy = self.analysis_rx.is_some();
        let mut read = false;
        let mut file = false;
        theme::page_header(ui, "Tactical Effectiveness", &subtitle, |ui| {
            if theme::button(ui, icon::EXPORT, t!("Uit bestand", "From file"), !busy).clicked() {
                file = true;
            }
            if theme::primary_button(ui, icon::CAMERA, t!("Lees FM-scherm", "Read FM screen"), !busy).clicked() {
                read = true;
            }
            if busy {
                ui.spinner();
            }
        });
        if read {
            self.last_auto = std::time::Instant::now();
            self.start_analysis(ctx, None);
        }
        if file {
            self.show_file = !self.show_file;
        }
        ui.horizontal(|ui| {
            if theme::toggle(ui, &mut self.settings.auto_read).changed() {
                self.last_auto = std::time::Instant::now() - std::time::Duration::from_secs(600);
                self.auto_note.clear();
                self.save_settings();
            }
            ui.label(theme::label_strong(t!("Automatisch uitlezen", "Auto-read")));
            if self.settings.auto_read {
                ui.label(theme::dim(t!("elke", "every")));
                if ui.add(egui::DragValue::new(&mut self.settings.auto_read_secs).range(5..=120).suffix(" s")).changed() {
                    self.save_settings();
                }
                if !self.auto_note.is_empty() {
                    theme::pill(ui, &self.auto_note, theme::INFO);
                }
            } else {
                ui.label(theme::faint(t!(
                    "Laat de Breakdown in FM open staan en de app leest hem zelf opnieuw.",
                    "Leave the Breakdown open in FM and the app re-reads it by itself."
                )));
            }
        });
        ui.add_space(12.0);
        if self.show_file {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("Screenshot (PNG):", "Screenshot (PNG):"));
                    ui.add(egui::TextEdit::singleline(&mut self.image_path).hint_text("/…/breakdown.png").desired_width(420.0));
                    if theme::button(ui, "", t!("Lees", "Read"), !busy && !self.image_path.trim().is_empty()).clicked() {
                        let p = PathBuf::from(self.image_path.trim().trim_matches('\''));
                        self.start_analysis(ctx, Some(p));
                    }
                });
            });
            ui.add_space(10.0);
        }
        let (msg, err) = (self.status.clone(), self.status_error);
        self.status_line(ui, &msg, err);

        // onderdelen
        ui.columns(4, |cols| {
            for (i, c) in Component::ALL.iter().enumerate() {
                let v = self.eff.component(*c);
                let col = theme::rgb(level_color(v));
                let ic = [icon::TARGET, icon::SOCCER_BALL, icon::USERS_THREE, icon::EYE][i];
                theme::stat_tile(&mut cols[i], ic, c.label(), &format!("{v:.0}"), level_name(v), col, Some(v));
            }
        });
        ui.add_space(14.0);

        // factors per onderdeel
        let mut changed = false;
        ui.columns(4, |cols| {
            for (i, c) in Component::ALL.iter().enumerate() {
                let ui = &mut cols[i];
                theme::card(ui, |ui| {
                    ui.label(theme::caps(c.label()));
                    ui.add_space(6.0);
                    for f in Factor::ALL.iter().filter(|f| f.component() == *c) {
                        let v = self.eff.get(*f);
                        theme::metric_row(ui, f.label(), v, theme::rgb(level_color(v)));
                        if self.edit_mode {
                            let mut nv = v;
                            if ui.add(egui::Slider::new(&mut nv, 0.0..=100.0).show_value(false)).changed() && (nv - v).abs() > 0.01 {
                                self.eff.set(*f, nv);
                                changed = true;
                            }
                        }
                    }
                });
            }
        });
        if changed {
            self.eff.components.clear();
            if !self.eff.source.ends_with('*') {
                self.eff.source = format!("{} *", if self.eff.source.is_empty() { t!("handmatig", "manual") } else { &self.eff.source });
            }
            let _ = crate::save_effectiveness(&self.eff);
            self.week_plan = None;
            self.year.clear();
        }
        ui.add_space(4.0);
        ui.checkbox(&mut self.edit_mode, t!("Waarden handmatig aanpassen", "Adjust values manually"));
        ui.add_space(10.0);

        // prioriteiten
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::ic(icon::LIGHTNING).color(theme::WARNING).size(18.0));
                ui.label(theme::h2(t!("Prioriteiten", "Priorities")));
            });
            ui.add_space(6.0);
            for (f, v) in self.eff.weakest().into_iter().take(6) {
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(250.0, 20.0), egui::Label::new(theme::label_strong(f.label())));
                    theme::pill(ui, &format!("{v:.0} · {}", level_name(v)), theme::rgb(level_color(v)));
                    ui.add(egui::Label::new(theme::dim(f.advice())).wrap());
                });
                ui.add_space(4.0);
            }
        });
    }

    // -------------------------------------------------------------------- training

    fn page_training(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let connected = agent::LIVE_SUPPORTED && self.live.connected;
        let can = connected && !self.live.busy();
        let (mut view, mut write) = (false, false);
        theme::page_header(
            ui,
            "Training",
            t!(
                "FM27 Trainer plant je training op je zwakste gebieden en houdt hem in FM's kalender vast.",
                "FM27 Trainer plans your training on your weakest areas and keeps it in FM's calendar."
            ),
            |ui| {
                if theme::primary_button(ui, icon::LIGHTNING, t!("Zet training nu in FM", "Apply training now"), can).clicked() {
                    write = true;
                }
                if theme::button(ui, icon::EYE, t!("Bekijk plan", "Preview"), can).clicked() {
                    view = true;
                }
                if self.live.busy() {
                    ui.spinner();
                }
            },
        );
        if write {
            self.live.training(ctx, &self.eff, &self.settings.planner, true, false);
        }
        if view {
            self.live.training(ctx, &self.eff, &self.settings.planner, false, false);
        }
        let (m, e) = (self.live.train_status.clone(), self.live.train_error);
        self.status_line(ui, &m, e);
        // testhulp: FM27_TEAMS_JSON=<bestand> toont de keuze zonder verbinding
        let demo = std::env::var_os("FM27_TEAMS_JSON");
        if let (Some(p), None) = (&demo, &self.live.teams) {
            self.live.teams = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok());
        }
        if connected || demo.is_some() {
            self.team_picker(ui, ctx);
        }

        // ---- statusblok: automatisch bijhouden
        let auto = self.settings.auto_training;
        let accent = if auto && connected { theme::SUCCESS } else { theme::WARNING };
        Frame::new()
            .fill(theme::tint(accent, 0.10))
            .stroke(Stroke::new(1.0, theme::tint(accent, 0.45)))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(18))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(theme::ic(if auto { icon::CHECK_CIRCLE } else { icon::WARNING }).size(30.0).color(accent));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(theme::h2(t!("Training automatisch bijhouden", "Keep training on plan automatically")));
                            ui.add_space(8.0);
                            if theme::toggle(ui, &mut self.settings.auto_training).changed() {
                                self.save_settings();
                            }
                            ui.label(RichText::new(if auto { t!("AAN", "ON") } else { t!("UIT", "OFF") }).strong().color(accent));
                        });
                        let text = if !connected {
                            t!(
                                "Werkt zodra FM met de module draait (zie hieronder).",
                                "Works once FM runs with the module (see below)."
                            )
                        } else if auto {
                            t!(
                                "FM's assistent plant elke week de komende 1–2 weken opnieuw in. FM27 Trainer controleert elke 8 seconden en zet afwijkingen terug naar jouw plan. Laat de app open terwijl je speelt.",
                                "FM's assistant re-plans the next 1–2 weeks every week. FM27 Trainer checks every 8 seconds and puts any change back on your plan. Keep the app open while you play."
                            )
                        } else {
                            t!(
                                "Uit: FM overschrijft je training elke week met zijn eigen planning. Zet dit aan, of klik na elke paar speelweken op 'Zet training nu in FM'.",
                                "Off: FM overwrites your training with its own plan every week. Turn this on, or click 'Apply training now' every few game weeks."
                            )
                        };
                        ui.add(egui::Label::new(theme::dim(text)).wrap());
                    });
                });
                if connected {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 36.0;
                        let last = self.live.last_check.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_else(|| "—".into());
                        theme::mini_stat(ui, t!("Laatste controle", "Last check"), &last);
                        theme::mini_stat(ui, t!("Teruggezet (deze sessie)", "Restored (this session)"), &t!(format!("{} dagen", self.live.fixed_total), format!("{} days", self.live.fixed_total)));
                        let d = self.live.training.as_ref().and_then(|o| o.today).map(i18n::date).unwrap_or_else(|| "—".into());
                        theme::mini_stat(ui, t!("Spel-datum", "Game date"), &d);
                    });
                }
            });
        ui.add_space(14.0);

        // ---- zo werkt het: meten → plannen → bijhouden
        let today = self.live.training.as_ref().and_then(|o| o.today);
        let age_weeks = match (self.eff.game_date, today) {
            (Some(d), Some(t)) => Some((t - d).num_days() / 7),
            _ => None,
        };
        let stale = !self.has_data() || age_weeks.is_none_or(|w| w >= 3);
        let mut go_overview = false;
        ui.columns(3, |cols| {
            step_card(&mut cols[0], 1, icon::CAMERA, t!("Meten", "Measure"), |ui| {
                let txt = if !self.has_data() {
                    t!("Nog niet uitgelezen.", "Not read yet.").to_string()
                } else {
                    match age_weeks {
                        Some(0) => t!("Uitgelezen deze week (speltijd).", "Read this week (game time).").to_string(),
                        Some(w) => t!(format!("Uitgelezen {w} weken geleden (speltijd)."), format!("Read {w} weeks ago (game time).")),
                        None => t!(format!("Uitgelezen: {}", self.eff.source), format!("Read: {}", self.eff.source)),
                    }
                };
                ui.label(if stale { RichText::new(txt).color(theme::WARNING) } else { theme::dim(txt) });
                ui.add(egui::Label::new(theme::faint(t!(
                    "Open in FM de Breakdown en lees hem uit; doe dit elke 2–3 speelweken.",
                    "Open the Breakdown in FM and read it; do this every 2–3 game weeks."
                ))).wrap());
                if theme::small_button(ui, t!("Naar Overzicht", "Go to Overview"), true).clicked() {
                    go_overview = true;
                }
            });
            step_card(&mut cols[1], 2, icon::TARGET, t!("Plannen", "Plan"), |ui| {
                ui.label(theme::dim(t!("Extra aandacht voor:", "Extra focus on:")));
                for (f, v) in self.eff.weakest().into_iter().filter(|(f, _)| f.is_tactical_area()).take(3) {
                    theme::pill(ui, &format!("{} · {v:.0}", f.label()), theme::rgb(level_color(v)));
                }
                ui.add(egui::Label::new(theme::faint(t!(
                    "Het ritme rond wedstrijden volgt SI's presets; de focus schuift per week mee.",
                    "The rhythm around matches follows SI's presets; the focus shifts week by week."
                ))).wrap());
            });
            step_card(&mut cols[2], 3, icon::SPARKLE, t!("Bijhouden", "Keep"), |ui| {
                for l in [
                    t!("Alleen dagen ná vandaag worden aangepast.", "Only days after today are changed."),
                    t!("Wedstrijd- en reisdagen blijven van FM.", "Match and travel days stay with FM."),
                    t!("Rustweken (vakantie) blijven van FM.", "Rest weeks (holidays) stay with FM."),
                    t!("Opslaan in FM bewaart alles in je save.", "Saving in FM keeps it in your save."),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(theme::ic(icon::CHECK_CIRCLE).color(theme::SUCCESS));
                        ui.add(egui::Label::new(theme::dim(l)).wrap());
                    });
                }
            });
        });
        if go_overview {
            self.page = Page::Overview;
        }
        ui.add_space(14.0);

        if !connected {
            self.connect_card(ui, ctx);
            return;
        }

        // ---- logboek
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::ic(icon::CALENDAR_DOTS).color(theme::INFO).size(16.0));
                ui.label(theme::h2(t!("Logboek", "Activity")));
            });
            ui.add_space(4.0);
            if self.live.log.is_empty() {
                ui.label(theme::faint(t!(
                    "Nog geen aanpassingen. Zodra FM een week anders inplant, zie je hier wat er is teruggezet.",
                    "No changes yet. When FM re-plans a week, you'll see here what was put back."
                )));
            }
            for e in self.live.log.iter().take(6) {
                ui.horizontal(|ui| {
                    ui.label(theme::faint(e.at.format("%H:%M:%S").to_string()));
                    theme::pill(ui, if e.auto { t!("automatisch", "automatic") } else { t!("handmatig", "manual") }, if e.auto { theme::SUCCESS } else { theme::ACCENT });
                    ui.add(egui::Label::new(RichText::new(&e.text).color(theme::TEXT)).wrap());
                });
            }
        });
        ui.add_space(14.0);

        let Some(o) = self.live.training.clone() else {
            if !self.live.busy() {
                self.live.training(ctx, &self.eff, &self.settings.planner, false, false);
            }
            return;
        };
        ui.horizontal(|ui| {
            ui.label(theme::h2(t!("Komende weken", "Coming weeks")));
            ui.add_space(10.0);
            legend(ui);
        });
        ui.add_space(6.0);
        for w in &o.weeks {
            week_card(ui, w, o.today, &self.live.fm_had);
            ui.add_space(10.0);
        }
    }

    // -------------------------------------------------------------------- planning

    /// Keuze van manager (bij meer dan één in de save) en van het team waarvan de training gepland
    /// wordt. Alleen zichtbaar als er iets te kiezen valt.
    fn team_picker(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let Some(data) = self.live.teams.clone() else { return };
        let list = |k: &str| data.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let (managers, teams) = (list("managers"), list("teams"));
        if managers.len() < 2 && teams.len() < 2 {
            return;
        }
        let team_name = |t: &Value| -> String {
            let n = |k: &str| t.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
            let name = match (n("kind"), n("age_limit")) {
                (0, _) => t!("Eerste elftal", "First team").to_string(),
                (_, a) if (1..100).contains(&a) => t!(format!("Onder {a}"), format!("Under {a}")),
                _ => t!(format!("Team {}", n("index") + 1), format!("Team {}", n("index") + 1)),
            };
            format!("{name} · {} {}", n("players"), t!("spelers", "players"))
        };
        let sel = |v: &Value| v.get("selected").and_then(|b| b.as_bool()) == Some(true);
        let mut pick: Option<(Option<u32>, Option<usize>)> = None;
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::ic(icon::USERS_THREE).color(theme::ACCENT));
                if managers.len() > 1 {
                    ui.label(t!("Manager van", "Manager of"));
                    let cur = managers.iter().find(|m| sel(m)).and_then(|m| m["club"].as_str()).unwrap_or("?").to_string();
                    egui::ComboBox::from_id_salt("manager_pick").selected_text(cur).show_ui(ui, |ui| {
                        for m in &managers {
                            let uid = m["club_uid"].as_u64().unwrap_or(0) as u32;
                            if ui.selectable_label(sel(m), m["club"].as_str().unwrap_or("?")).clicked() && !sel(m) {
                                pick = Some((Some(uid), None));
                            }
                        }
                    });
                    ui.add_space(14.0);
                }
                if teams.len() > 1 {
                    ui.label(t!("Training van", "Training of"));
                    let cur = teams.iter().find(|x| sel(x)).map(&team_name).unwrap_or_else(|| "?".into());
                    egui::ComboBox::from_id_salt("team_pick").selected_text(cur).width(210.0).show_ui(ui, |ui| {
                        for x in &teams {
                            if ui.selectable_label(sel(x), team_name(x)).clicked() && !sel(x) {
                                let own = x.get("own").and_then(|b| b.as_bool()) == Some(true);
                                let idx = x["index"].as_u64().unwrap_or(0) as usize;
                                pick = Some((self.settings.manager_club, if own { None } else { Some(idx) }));
                            }
                        }
                    });
                }
            });
            if teams.iter().any(|x| sel(x) && x.get("own").and_then(|b| b.as_bool()) != Some(true)) {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(theme::dim(t!(
                        "Dit is niet je eigen team. De app kan zijn training alleen plannen als FM er een eigen kalender voor bijhoudt (jij regelt de training van dit team). Het plan gebruikt de waarden die je het laatst hebt uitgelezen.",
                        "This is not your own team. The app can only plan its training if FM keeps a calendar of its own for it (you run this team's training). The plan uses the values you read last."
                    )))
                    .wrap(),
                );
            } else if managers.len() > 1 {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(theme::dim(t!(
                        "Club, selectie, transfers en training horen bij de gekozen manager. Lees de Breakdown van dit team opnieuw uit voordat je de training laat plannen.",
                        "Club, squad, transfers and training follow the chosen manager. Read this team's Breakdown again before you let the app plan its training."
                    )))
                    .wrap(),
                );
            }
        });
        ui.add_space(14.0);
        if let Some((club, team)) = pick {
            self.settings.manager_club = club;
            self.settings.training_team = team;
            self.save_settings();
            self.live.choose(ctx, (club, team));
        }
    }

    fn page_planning(&mut self, ui: &mut Ui) {
        let mut tab = self.plan_tab;
        theme::page_header(
            ui,
            "Planning",
            t!("Week- en seizoensplan als overzicht en export (werkt ook zonder module).", "Weekly and season plan as overview and export (works without the module)."),
            |ui| {
                theme::segmented(ui, &mut tab, &[(PlanTab::Week, "Week"), (PlanTab::Season, t!("Seizoen", "Season"))]);
            },
        );
        self.plan_tab = tab;
        let (m, e) = (self.status.clone(), self.status_error);
        self.status_line(ui, &m, e);
        match self.plan_tab {
            PlanTab::Week => self.planning_week(ui),
            PlanTab::Season => self.planning_season(ui),
        }
    }

    fn planning_week(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(t!("Week vanaf", "Week from"));
                ui.add(egui::TextEdit::singleline(&mut self.week_start).desired_width(96.0));
                ui.separator();
                ui.label(t!("Wedstrijd op", "Match on"));
                for i in 0..7 {
                    ui.toggle_value(&mut self.week_matches[i], i18n::weekday_short(i));
                }
                ui.separator();
                ui.checkbox(&mut self.prev_sunday, t!("wedstrijd zondag ervoor", "match previous Sunday"));
                ui.checkbox(&mut self.next_monday, t!("wedstrijd maandag erna", "match next Monday"));
                kind_combo(ui, "weekkind", &mut self.week_kind);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                // bij een testopname (FM27_SNAPSHOT) meteen een plan laten zien
                let auto = self.week_plan.is_none() && std::env::var("FM27_SNAPSHOT").is_ok();
                if theme::primary_button(ui, icon::SPARKLE, t!("Maak weekplan", "Build week plan"), true).clicked() || auto {
                    let start = NaiveDate::parse_from_str(self.week_start.trim(), "%Y-%m-%d")
                        .map(planner::monday_of)
                        .unwrap_or_else(|_| planner::monday_of(chrono::Local::now().date_naive()));
                    let week = CalendarWeek { start, kind: self.week_kind, matches: self.week_matches, note: String::new() };
                    self.week_plan = Some(planner::plan_week(&week, self.prev_sunday, self.next_monday, &self.eff, &self.settings.planner, 1));
                }
                if let Some(p) = self.week_plan.clone() {
                    if theme::button(ui, icon::EXPORT, t!("Exporteer & open", "Export & open"), true).clicked() {
                        let r = export::write_all(&export::output_dir(), "weekplan", t!("FM27 weekplan", "FM27 week plan"), &self.eff, std::slice::from_ref(&p));
                        self.after_export(r);
                    }
                }
            });
        });
        ui.add_space(12.0);
        if let Some(p) = &self.week_plan {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::h2(&p.schedule_name));
                    theme::pill(ui, &format!("Focus: {}", p.focus), theme::ACCENT);
                });
                ui.add_space(8.0);
                plan_grid(ui, p);
                if !p.notes.is_empty() {
                    ui.add_space(8.0);
                    ui.label(theme::caps(t!("Buiten de training", "Beyond training")));
                    for n in &p.notes {
                        ui.add(egui::Label::new(theme::dim(format!("• {n}"))).wrap());
                    }
                }
            });
            ui.add_space(8.0);
            legend(ui);
        }
    }

    fn planning_season(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Start");
                ui.add(egui::TextEdit::singleline(&mut self.year_start).desired_width(96.0));
                ui.label(t!("Competitie op", "League on"));
                weekday_combo(ui, "league", &mut self.cal_opts.league_day);
                ui.label(t!("Europees", "Europe"));
                let mut e = self.cal_opts.europe;
                egui::ComboBox::from_id_salt("europe")
                    .selected_text(match e {
                        None => t!("geen", "none"),
                        Some(Weekday::Tue) => t!("di (CL)", "Tue (CL)"),
                        Some(Weekday::Wed) => t!("wo (CL)", "Wed (CL)"),
                        _ => t!("do (EL/ECL)", "Thu (EL/ECL)"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut e, None, t!("geen", "none"));
                        ui.selectable_value(&mut e, Some(Weekday::Tue), t!("di (CL)", "Tue (CL)"));
                        ui.selectable_value(&mut e, Some(Weekday::Wed), t!("wo (CL)", "Wed (CL)"));
                        ui.selectable_value(&mut e, Some(Weekday::Thu), t!("do (EL/ECL)", "Thu (EL/ECL)"));
                    });
                self.cal_opts.europe = e;
                ui.checkbox(&mut self.cal_opts.cup, t!("beker", "cup"));
                ui.add(egui::DragValue::new(&mut self.cal_opts.weeks).range(4..=60).suffix(t!(" weken", " weeks")));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::button(ui, icon::CALENDAR_BLANK, t!("Standaardkalender", "Default calendar"), true).clicked() {
                    if let Ok(d) = NaiveDate::parse_from_str(self.year_start.trim(), "%Y-%m-%d") {
                        self.cal_opts.start = d;
                    }
                    self.calendar = planner::default_calendar(&self.cal_opts);
                    self.year.clear();
                    self.save_calendar();
                }
                if theme::primary_button(ui, icon::SPARKLE, t!("Maak seizoensplan", "Build season plan"), true).clicked() {
                    self.year = planner::plan_year(&self.calendar, &self.eff, &self.settings.planner);
                    self.save_calendar();
                }
                if !self.year.is_empty() {
                    if theme::button(ui, icon::EXPORT, t!("Exporteer & open", "Export & open"), true).clicked() {
                        let r = export::write_all(&export::output_dir(), "seasonplan", t!("FM27 seizoensplan", "FM27 season plan"), &self.eff, &self.year);
                        self.after_export(r);
                    }
                    let (n, u) = (self.year.len(), unique_schedules(&self.year).len());
                    ui.label(theme::faint(t!(format!("{n} weken · {u} verschillende weekschema's"), format!("{n} weeks · {u} distinct weekly schedules"))));
                }
            });
        });
        ui.add_space(12.0);
        let mut edited = false;
        theme::card(ui, |ui| {
            egui::Grid::new("cal").num_columns(5).spacing([14.0, 8.0]).striped(true).show(ui, |ui| {
                for h in [t!("Week van", "Week of"), t!("Periode", "Period"), t!("Wedstrijden", "Matches"), "Focus", t!("Tactisch", "Tactical")] {
                    ui.label(theme::caps(h));
                }
                ui.end_row();
                for (wi, week) in self.calendar.iter_mut().enumerate() {
                    ui.label(i18n::date(week.start));
                    let before = week.kind;
                    kind_combo(ui, &format!("k{wi}"), &mut week.kind);
                    edited |= week.kind != before;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for d in 0..7 {
                            if ui.toggle_value(&mut week.matches[d], &i18n::weekday_short(d)[..1]).changed() {
                                edited = true;
                            }
                        }
                        if !week.note.is_empty() {
                            ui.label(theme::faint(&week.note));
                        }
                    });
                    if let Some(p) = self.year.get(wi) {
                        theme::pill(ui, p.focus, theme::ACCENT);
                        let t: Vec<String> = p.targets.iter().map(|(f, n)| format!("{}×{}", planner::short(*f), n)).collect();
                        ui.label(theme::faint(t.join("  ")));
                    } else {
                        ui.label("");
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        });
        if edited {
            self.year.clear();
            self.save_calendar();
        }
    }

    fn after_export(&mut self, res: Result<PathBuf>) {
        match res {
            Ok(p) => {
                open_path(&p);
                self.set_status(t!(format!("Opgeslagen: {}", p.display()), format!("Saved: {}", p.display())), false);
            }
            Err(e) => self.set_status(format!("{e:#}"), true),
        }
    }

    // -------------------------------------------------------------------- club

    fn page_club(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let busy = self.live.busy();
        let connected = self.live.connected;
        let name = self.live.club_name().unwrap_or_else(|| "Club".into());
        let mut action: Option<(String, Value)> = None;
        let mut backup = false;
        theme::page_header(
            ui,
            &name,
            t!("Financiën live aanpassen. Wijzigingen blijven bewaard als je in FM opslaat.", "Edit finances live. Changes persist once you save in FM."),
            |ui| {
                if theme::button(ui, icon::ARROW_COUNTER_CLOCKWISE, t!("Ongedaan maken", "Undo"), connected && !busy).clicked() {
                    action = Some((t!("ongedaan maken", "undo").into(), json!({"cmd": "undo"})));
                }
                if theme::button(ui, icon::FLOPPY_DISK, t!("Backup saves", "Back up saves"), !busy).clicked() {
                    backup = true;
                }
            },
        );
        if backup {
            match agent::backup_saves() {
                Ok(d) => self.live.status = t!(format!("Backup gemaakt: {}", d.display()), format!("Backup created: {}", d.display())),
                Err(e) => self.live.status = format!("{e:#}"),
            }
        }
        if let Some((l, r)) = action.take() {
            self.live.refresh(ctx, &l, Some(r));
        }
        let (m, e) = (self.live.status.clone(), self.live.status_error);
        self.status_line(ui, &m, e);
        if !connected {
            self.connect_card(ui, ctx);
            return;
        }
        let Some(c) = self.live.club.clone() else { return };
        let g = |k: &str| c.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let rate = self.settings.rate;
        let eur = |v: i64| format!("€ {}", money((v as f32 * rate) as i64));
        let gbp = |v: i64| format!("£ {}", money(v));
        ui.columns(3, |cols| {
            theme::stat_tile(&mut cols[0], icon::COINS, t!("Saldo", "Balance"), &eur(g("balance")), &gbp(g("balance")), theme::INFO, None);
            theme::stat_tile(&mut cols[1], icon::BANK, t!("Transferbudget", "Transfer budget"), &eur(g("transfer_budget")), &gbp(g("transfer_budget")), theme::ACCENT, None);
            theme::stat_tile(&mut cols[2], icon::USERS_THREE, t!("Loonbudget p/w", "Wage budget p/w"), &eur(g("wage_budget")), &gbp(g("wage_budget")), theme::SUCCESS, None);
        });
        ui.add_space(14.0);
        ui.columns(2, |cols| {
            theme::card(&mut cols[0], |ui| {
                ui.label(theme::h2(t!("Transferbudget", "Transfer budget")));
                ui.label(theme::faint(t!("Bedrag in miljoen euro", "Amount in million euro")));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.live.transfer_input).hint_text("50").desired_width(90.0));
                    ui.label(t!("mln €", "M €"));
                    if theme::primary_button(ui, icon::CHECK_CIRCLE, t!("Zet", "Set"), !busy).clicked() {
                        if let Ok(m) = self.live.transfer_input.replace(',', ".").trim().parse::<f64>() {
                            action = Some((t!("transferbudget", "transfer budget").into(), json!({"cmd": "set_finances", "transfer_budget": (m * 1e6 / rate as f64) as i64})));
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for add in [10, 25, 50, 100] {
                        if theme::small_button(ui, &format!("+€{add}M"), !busy).clicked() {
                            let v = g("transfer_budget") + (add as f64 * 1e6 / rate as f64) as i64;
                            action = Some((t!("transferbudget", "transfer budget").into(), json!({"cmd": "set_finances", "transfer_budget": v})));
                        }
                    }
                });
            });
            theme::card(&mut cols[1], |ui| {
                ui.label(theme::h2(t!("Loonbudget", "Wage budget")));
                ui.label(theme::faint(t!("Bedrag in duizend euro per week", "Amount in thousand euro per week")));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.live.wage_input).hint_text("900").desired_width(90.0));
                    ui.label("k € p/w");
                    if theme::primary_button(ui, icon::CHECK_CIRCLE, t!("Zet", "Set"), !busy).clicked() {
                        if let Ok(k) = self.live.wage_input.replace(',', ".").trim().parse::<f64>() {
                            action = Some((t!("loonbudget", "wage budget").into(), json!({"cmd": "set_finances", "wage_budget": (k * 1e3 / rate as f64) as i64})));
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for add in [50, 100, 250] {
                        if theme::small_button(ui, &format!("+€{add}k"), !busy).clicked() {
                            let v = g("wage_budget") + (add as f64 * 1e3 / rate as f64) as i64;
                            action = Some((t!("loonbudget", "wage budget").into(), json!({"cmd": "set_finances", "wage_budget": v})));
                        }
                    }
                });
            });
        });
        ui.add_space(8.0);
        ui.label(theme::faint(t!(
            format!("FM rekent intern in ponden; euro's zijn omgerekend met koers {rate:.2} (aan te passen bij Instellingen)."),
            format!("FM stores pounds internally; euros use rate {rate:.2} (change it in Settings).")
        )));
        ui.add_space(12.0);
        // de vlaggen van FM's in-game editor op je manager: registratie en rugnummers altijd
        let hf = self.live.human_flags.clone();
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Spelregels", "Rules")));
            ui.label(theme::faint(t!(
                "Dezelfde schakelaars als in FM's in-game editor, direct op je manager gezet. Controleer het in FM op het registratiescherm van de competitie.",
                "The same switches as FM's in-game editor, set straight on your manager. Check it in FM on the competition's registration screen."
            )));
            ui.add_space(6.0);
            match hf.as_ref().and_then(|v| v.get("supported")).and_then(|v| v.as_bool()) {
                Some(true) => {
                    let rows = [
                        ("registration_anytime", t!("Spelers altijd registreren", "Register players anytime"), t!("Selectieregistratie ook buiten de registratieperiode (competitie, UEFA).", "Squad registration outside the registration window too (league, UEFA).")),
                        ("numbers_anytime", t!("Rugnummers altijd aanpassen", "Change squad numbers anytime"), t!("Rugnummers ook wijzigen als de competitie ze heeft vastgezet.", "Change squad numbers even when the competition has locked them.")),
                    ];
                    for (key, label, hint) in rows {
                        let mut on = hf.as_ref().and_then(|v| v.get(key)).and_then(|v| v.as_bool()).unwrap_or(false);
                        ui.horizontal(|ui| {
                            if theme::toggle(ui, &mut on).changed() && connected && !busy {
                                let mut m = serde_json::Map::new();
                                m.insert("cmd".into(), Value::String("set_human_flags".into()));
                                m.insert(key.into(), Value::Bool(on));
                                action = Some((label.to_string(), Value::Object(m)));
                            }
                            ui.label(theme::label_strong(label));
                            ui.label(theme::faint(hint));
                        });
                    }
                }
                Some(false) => {
                    ui.label(theme::faint(t!("Op deze FM-versie of dit platform nog niet beschikbaar.", "Not available on this FM build or platform yet.")));
                }
                None => {
                    ui.label(theme::faint(t!("Nog niet uitgelezen.", "Not read yet.")));
                }
            }
        });
        if let Some((l, r)) = action {
            self.live.refresh(ctx, &l, Some(r));
        }
    }

    // -------------------------------------------------------------------- selectie

    fn page_squad(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let busy = self.live.busy();
        let connected = self.live.connected;
        let injured = self.live.squad.iter().filter(|p| p.get("injuries").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty())).count();
        let banned = self.live.squad.iter().filter(|p| p.get("bans").and_then(|v| v.as_u64()).unwrap_or(0) > 0).count();
        let mut action: Option<(String, Value)> = None;
        let mut sell: Option<u32> = None;
        let n = self.live.squad.len();
        let subtitle = t!(format!("{n} spelers · {injured} geblesseerd · {banned} geschorst"), format!("{n} players · {injured} injured · {banned} suspended"));
        theme::page_header(ui, t!("Selectie", "Squad"), &subtitle, |ui| {
            if theme::button(ui, icon::SMILEY, t!("Moraal top voor iedereen", "Max morale for all"), connected && !busy).clicked() {
                action = Some((t!("moraal", "morale").into(), json!({"cmd": "inspire", "morale": 20, "clear_unhappiness": true})));
            }
            if theme::button(ui, icon::WARNING, t!("Schorsingen opheffen", "Lift suspensions"), connected && !busy && banned > 0).clicked() {
                action = Some((t!("schorsingen", "suspensions").into(), json!({"cmd": "lift_bans"})));
            }
            let heal = t!(format!("Genees iedereen ({injured})"), format!("Heal all ({injured})"));
            if theme::primary_button(ui, icon::FIRST_AID, &heal, connected && !busy && injured > 0).clicked() {
                action = Some((t!("genezen", "healing").into(), json!({"cmd": "heal"})));
            }
        });
        let (m, e) = (self.live.status.clone(), self.live.status_error);
        self.status_line(ui, &m, e);
        if !connected {
            self.connect_card(ui, ctx);
            return;
        }
        // tactisch begrip (FM: Tactical Understanding), te maximaliseren zoals FM's in-game editor dat kan
        let tu = self.live.understanding.clone();
        let tu_players: std::collections::HashMap<u64, f32> = tu
            .as_ref()
            .and_then(|u| u.get("players"))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        let uid = p.get("uid")?.as_u64()?;
                        let levels: Vec<f32> = p.get("levels")?.as_array()?.iter().filter_map(|x| x.as_u64()).map(|x| x as f32 / 100.0).collect();
                        (!levels.is_empty()).then(|| (uid, levels.iter().sum::<f32>() / levels.len() as f32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(theme::h2(t!("Tactisch begrip", "Tactical understanding")));
                    ui.add(
                        egui::Label::new(theme::faint(t!(
                            "Hoe goed de selectie je huidige tactiek kent, per onderdeel (gemiddelde van de spelers). Maximaliseren doet wat de in-game editor van FM doet: alles naar 100 %.",
                            "How well the squad knows your current tactic, per area (average over the players). Maximise does what FM's in-game editor does: everything to 100%."
                        )))
                        .wrap(),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if theme::primary_button(ui, icon::BRAIN, t!("Maximaliseer tactisch begrip", "Maximise tactical understanding"), connected && !busy).clicked() {
                        action = Some((t!("tactisch begrip", "tactical understanding").into(), json!({"cmd": "max_understanding"})));
                    }
                });
            });
            match tu.as_ref().and_then(|u| u.get("team")).and_then(|v| v.as_array()) {
                Some(team) if !team.is_empty() => {
                    ui.add_space(6.0);
                    // twee rijen van vier: in bezit, dan zonder bal
                    for row in team.iter().take(UNDERSTANDING_AREAS.len()).enumerate().collect::<Vec<_>>().chunks(4) {
                        ui.horizontal(|ui| {
                            for (i, v) in row {
                                let pct = v.as_u64().unwrap_or(0) as f32 / 100.0;
                                theme::pill(ui, &format!("{} · {pct:.0}%", UNDERSTANDING_AREAS[*i]), theme::rgb(level_color(pct)));
                            }
                        });
                    }
                    let without = tu.as_ref().and_then(|u| u.get("without_record")).and_then(|v| v.as_u64()).unwrap_or(0);
                    if without > 0 {
                        ui.label(theme::faint(t!(
                            format!("{without} spelers hebben nog geen begrip-record (nog niet met dit team getraind); die doen pas na een week training mee."),
                            format!("{without} players have no understanding record yet (not trained with this team yet); they join after a week of training.")
                        )));
                    }
                }
                _ => {
                    ui.label(theme::faint(t!("Nog niet uitgelezen.", "Not read yet.")));
                }
            }
            // FM laat het begrip elke speelweek een stuk zakken: op verzoek houdt de app het op 100 %
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let keep = self.settings.keep_understanding;
                if theme::toggle(ui, &mut self.settings.keep_understanding).changed() {
                    self.save_settings();
                }
                ui.label(theme::label_strong(t!("Op 100 % houden", "Keep at 100%")));
                ui.label(RichText::new(if keep { t!("AAN", "ON") } else { t!("UIT", "OFF") }).strong().color(if keep { theme::SUCCESS } else { theme::TEXT_FAINT }));
                ui.add(
                    egui::Label::new(theme::faint(t!(
                        "Zolang de app open is, zet hij elke 20 seconden terug wat FM heeft laten zakken (FM Live Editor doet dit niet vanzelf).",
                        "While the app is open, every 20 seconds it puts back whatever FM has let drop (FM Live Editor does not do this by itself)."
                    )))
                    .wrap(),
                );
            });
        });
        theme::card(ui, |ui| {
            egui::Grid::new("squad").num_columns(8).spacing([18.0, 10.0]).striped(true).show(ui, |ui| {
                for h in [t!("Speler", "Player"), t!("Moraal", "Morale"), t!("Conditie", "Condition"), t!("Begrip", "Understanding"), t!("Blessure", "Injury"), t!("Schorsing", "Ban"), t!("Ontevreden", "Unhappy"), ""] {
                    ui.label(theme::caps(h));
                }
                ui.end_row();
                for p in &self.live.squad {
                    let uid = p.get("uid").and_then(|v| v.as_u64()).unwrap_or(0);
                    let m = p.get("morale").and_then(|v| v.as_u64()).unwrap_or(0);
                    ui.label(theme::label_strong(p.get("name").and_then(|v| v.as_str()).unwrap_or("?")));
                    theme::pill(ui, &format!("{m} · {}", morale_label(m)), theme::rgb(level_color((m as f32 - 1.0) / 19.0 * 100.0)));
                    let cond = p.get("condition").and_then(|v| v.as_u64()).unwrap_or(0) as f32 / 100.0;
                    ui.scope(|ui| {
                        ui.set_width(90.0);
                        theme::progress(ui, cond, 6.0, theme::rgb(level_color(cond)));
                    });
                    match tu_players.get(&uid) {
                        Some(pct) => {
                            theme::pill(ui, &format!("{pct:.0}%"), theme::rgb(level_color(*pct)));
                        }
                        None => {
                            ui.label(theme::faint("—"));
                        }
                    }
                    let inj: Vec<String> = p
                        .get("injuries")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().map(|i| i.get("name").and_then(|n| n.as_str()).unwrap_or("injury").to_string()).collect())
                        .unwrap_or_default();
                    if inj.is_empty() {
                        ui.label(theme::faint("—"));
                    } else {
                        theme::pill(ui, &inj.join(", "), theme::DANGER);
                    }
                    let bans = p.get("bans").and_then(|v| v.as_u64()).unwrap_or(0);
                    if bans > 0 {
                        theme::pill(ui, t!("geschorst", "suspended"), theme::WARNING);
                    } else {
                        ui.label(theme::faint("—"));
                    }
                    let unhappy = p.get("unhappy_reasons").and_then(|v| v.as_bool()).unwrap_or(false);
                    if unhappy {
                        theme::pill(ui, t!("ja", "yes"), theme::WARNING);
                    } else {
                        ui.label(theme::faint("—"));
                    }
                    ui.horizontal(|ui| {
                        if !inj.is_empty() && theme::small_button(ui, t!("Genees", "Heal"), !busy).clicked() {
                            action = Some((t!("genezen", "healing").into(), json!({"cmd": "heal", "uids": [uid]})));
                        }
                        if (m < 20 || unhappy) && theme::small_button(ui, t!("Inspireer", "Inspire"), !busy).clicked() {
                            action = Some((t!("moraal", "morale").into(), json!({"cmd": "inspire", "uids": [uid], "morale": 20, "clear_unhappiness": true})));
                        }
                        let coach = p.get("player_coach").and_then(|v| v.as_bool()).unwrap_or(false);
                        if !coach && theme::small_button(ui, t!("Verkoop", "Sell"), !busy).clicked() {
                            sell = Some(uid as u32);
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if let Some((l, r)) = action {
            self.live.refresh(ctx, &l, Some(r));
        }
        if let Some(uid) = sell {
            self.start_sale(uid);
        }
    }

    /// Naar de verkooppagina met deze speler gekozen; de vraagprijs begint bij zijn waarde.
    fn start_sale(&mut self, uid: u32) {
        let value = self.live.squad.iter().find(|p| p.get("uid").and_then(|v| v.as_u64()) == Some(uid as u64)).and_then(|p| p.get("value")).and_then(|v| v.as_i64()).unwrap_or(0);
        self.live.sell.player = Some(uid);
        self.live.sell.buy = false;
        self.live.sell.plan = None;
        self.live.sell.status.clear();
        self.live.sell.fee_input = format!("{:.1}", value as f64 * self.settings.rate as f64 / 1e6);
        self.page = Page::Sell;
    }

    // -------------------------------------------------------------------- verkopen

    // -------------------------------------------------------------------- tactiek

    fn page_tactics(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        use crate::tactics as tx;
        // testhulp: FM27_TACTICS_JSON=<bestand> toont de pagina met bewaarde gegevens, zonder FM
        let from_file = std::env::var_os("FM27_TACTICS_JSON");
        if let (Some(f), None) = (&from_file, &self.live.tactics) {
            self.live.tactics = std::fs::read_to_string(f).ok().and_then(|t| serde_json::from_str(&t).ok());
        }
        let connected = self.live.connected || (from_file.is_some() && self.live.tactics.is_some());
        let busy = self.live.tactics_busy();
        // bij het openen één keer vanzelf ophalen
        if connected && !self.tactics_asked && from_file.is_none() {
            self.tactics_asked = true;
            self.live.load_tactics(ctx);
        }
        if !connected {
            self.tactics_asked = false;
        }
        let mut reload = false;
        theme::page_header(
            ui,
            t!("Tactiek", "Tactics"),
            t!("Hoe goed je spelers bij de rollen van je tactiek passen", "How well your players fit the roles of your tactic"),
            |ui| {
                if theme::primary_button(ui, icon::ARROWS_CLOCKWISE, if busy { t!("Bezig…", "Reading…") } else { t!("Opnieuw uitlezen", "Read again") }, connected && !busy).clicked() {
                    reload = true;
                }
            },
        );
        if reload {
            self.live.load_tactics(ctx);
        }
        if !connected {
            self.connect_card(ui, ctx);
            return;
        }
        let err = self.live.tactics_error.clone();
        self.status_line(ui, &err, true);
        let Some(data) = self.live.tactics.clone() else {
            if err.is_empty() {
                theme::card(ui, |ui| {
                    ui.label(theme::faint(t!("Tactiek uitlezen…", "Reading your tactic…")));
                });
            }
            return;
        };
        let pick = self.tactic_pick.filter(|i| *i < data.tactics.len()).unwrap_or(data.current.min(data.tactics.len() - 1));
        let tactic = &data.tactics[pick];
        let in_use = pick == data.current.min(data.tactics.len() - 1);
        let mut places = tx::places(tactic);
        // FM bewaart de basiself in de volgorde van de tactiek in gebruik: bij een andere tactiek
        // zegt die volgorde niets over wie waar zou staan
        if !in_use {
            for p in &mut places {
                p.uid = None;
            }
        }
        let by_uid: std::collections::HashMap<u32, &tx::Player> = data.players.iter().map(|p| (p.uid, p)).collect();
        let in_eleven: std::collections::HashSet<u32> = places.iter().filter_map(|p| p.uid).collect();
        // veldvolgorde: keeper eerst, dan per linie van rechts naar links
        let mut order: Vec<usize> = (0..places.len()).collect();
        order.sort_by_key(|&i| places[i].oop_order);
        let fits: Vec<Option<f32>> = places.iter().map(|pl| pl.uid.and_then(|u| by_uid.get(&u)).map(|p| tx::fit(p, pl))).collect();
        let known: Vec<f32> = fits.iter().flatten().copied().collect();
        let avg = if known.is_empty() { 0.0 } else { known.iter().sum::<f32>() / known.len() as f32 };
        let weakest = order.iter().filter_map(|&i| fits[i].map(|f| (i, f))).min_by(|a, b| a.1.total_cmp(&b.1));
        let best = tx::best_eleven(&places, &data.players);
        let best_avg = if best.len() == places.len() { places.iter().zip(&best).map(|(pl, &j)| tx::fit(&data.players[j], pl)).sum::<f32>() / places.len() as f32 } else { avg };

        // welke tactiek
        ui.horizontal(|ui| {
            let labels: Vec<String> = data
                .tactics
                .iter()
                .map(|t| {
                    let mut l = format!("{} {}", t!("Tactiek", "Tactic"), t.index + 1);
                    if t.index == data.tactics[data.current.min(data.tactics.len() - 1)].index {
                        l.push_str(t!(" · in gebruik", " · in use"));
                    }
                    l
                })
                .collect();
            let options: Vec<(usize, &str)> = labels.iter().enumerate().map(|(i, l)| (i, l.as_str())).collect();
            let mut v = pick;
            if theme::segmented(ui, &mut v, &options) {
                self.tactic_pick = Some(v);
                theme::restart_enter(ctx);
            }
            if !tactic.style.is_empty() {
                theme::pill(ui, &format!("{} {}", t!("sjabloon", "template"), tactic.style), theme::TEXT_DIM);
            }
        });
        ui.add_space(12.0);
        if !in_use {
            ui.add(
                egui::Label::new(theme::faint(t!(
                    "Dit is niet de tactiek die nu in gebruik is: FM bewaart je basiself alleen voor die tactiek. Hieronder staat daarom alleen de beste elf voor deze tactiek.",
                    "This is not the tactic in use: FM keeps your starting eleven for that tactic only. So below you only see the best eleven for this tactic."
                )))
                .wrap(),
            );
            ui.add_space(8.0);
        }

        ui.columns(3, |c| {
            theme::stat_tile(&mut c[0], icon::STRATEGY, t!("Elftal nu", "Current eleven"), &format!("{avg:.0}"), t!("gemiddelde geschiktheid", "average fit"), theme::rgb(level_color(avg)), Some(avg));
            let gain = best_avg - avg;
            theme::stat_tile(
                &mut c[1],
                icon::USERS_THREE,
                t!("Beste elf", "Best eleven"),
                &format!("{best_avg:.0}"),
                &if gain >= 0.5 { t!(format!("+{gain:.1} te winnen"), format!("+{gain:.1} to gain")) } else { t!("je elftal is al het beste".to_string(), "your eleven is already the best".to_string()) },
                theme::rgb(level_color(best_avg)),
                Some(best_avg),
            );
            let (wname, wval) = weakest
                .map(|(i, f)| (format!("{} · {}", places[i].line.label(places[i].side), places[i].uid.and_then(|u| by_uid.get(&u)).map(|p| p.name.as_str()).unwrap_or("?")), f))
                .unwrap_or((t!("geen elftal gekozen", "no eleven picked").to_string(), 0.0));
            theme::stat_tile(&mut c[2], icon::WARNING, t!("Zwakste plek", "Weakest place"), &format!("{wval:.0}"), &wname, theme::rgb(level_color(wval)), Some(wval));
        });
        ui.add_space(14.0);

        // de elf plekken
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Je elftal in deze tactiek", "Your eleven in this tactic")));
            ui.add_space(6.0);
            egui::Grid::new("tactic_slots").num_columns(6).spacing([18.0, 10.0]).striped(true).show(ui, |ui| {
                for h in [t!("Plek", "Place"), t!("Rol met / zonder bal", "Role in / out of possession"), t!("Speler", "Player"), t!("Geschikt", "Fit"), t!("Positie", "Position"), t!("Beter in de selectie", "Better in the squad")] {
                    ui.label(theme::caps(h));
                }
                ui.end_row();
                for &i in &order {
                    let pl = &places[i];
                    ui.label(theme::label_strong(pl.line.label(pl.side)));
                    let (ic, iname) = tx::ip_label(pl.ip);
                    let (oc, oname) = tx::oop_label(pl.oop);
                    ui.horizontal(|ui| {
                        theme::pill(ui, &ic, theme::ACCENT_HI).on_hover_text(iname);
                        theme::pill(ui, &oc, theme::INFO).on_hover_text(oname);
                    });
                    let cur = pl.uid.and_then(|u| by_uid.get(&u));
                    match cur {
                        Some(p) => {
                            let f = tx::fit(p, pl);
                            let r = tx::position_rating(p, pl);
                            ui.horizontal(|ui| {
                                ui.label(theme::label_strong(&p.name));
                                if p.injured {
                                    theme::pill(ui, t!("geblesseerd", "injured"), theme::DANGER);
                                }
                                if p.banned {
                                    theme::pill(ui, t!("geschorst", "suspended"), theme::WARNING);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.scope(|ui| {
                                    ui.set_width(80.0);
                                    theme::progress(ui, f, 6.0, theme::rgb(level_color(f)));
                                });
                                ui.label(RichText::new(format!("{f:.0}")).color(theme::TEXT));
                            });
                            theme::pill(ui, tx::position_word(r), theme::rgb(level_color(r as f32 * 5.0)));
                            // de beste speler buiten de elf voor deze plek
                            let alt = data
                                .players
                                .iter()
                                .filter(|q| !in_eleven.contains(&q.uid) && !q.injured && !q.banned)
                                .map(|q| (q, tx::fit(q, pl)))
                                .max_by(|a, b| a.1.total_cmp(&b.1));
                            match alt {
                                Some((q, qf)) if qf >= f + 2.0 => {
                                    ui.label(RichText::new(format!("{} (+{:.0})", q.name, qf - f)).color(theme::SUCCESS));
                                }
                                _ => {
                                    ui.label(theme::faint("—"));
                                }
                            }
                        }
                        None => {
                            ui.label(theme::faint(t!("leeg", "empty")));
                            ui.label(theme::faint("—"));
                            ui.label(theme::faint("—"));
                            ui.label(theme::faint("—"));
                        }
                    }
                    ui.end_row();
                }
            });
        });

        // de beste elf: alleen wat anders is dan nu
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Beste elf voor deze tactiek", "Best eleven for this tactic")));
            ui.add(
                egui::Label::new(theme::faint(t!(
                    "De verdeling van je fitte spelers over de elf plekken met de hoogste totale geschiktheid. Geblesseerde en geschorste spelers doen niet mee.",
                    "The way to spread your available players over the eleven places with the highest total fit. Injured and suspended players are left out."
                )))
                .wrap(),
            );
            ui.add_space(6.0);
            let mut changes = 0;
            if best.len() == places.len() {
                for &i in &order {
                    let pl = &places[i];
                    let b = &data.players[best[i]];
                    if pl.uid == Some(b.uid) {
                        continue;
                    }
                    changes += 1;
                    let now = pl.uid.and_then(|u| by_uid.get(&u));
                    ui.horizontal(|ui| {
                        ui.scope(|ui| {
                            ui.set_width(90.0);
                            ui.label(theme::label_strong(pl.line.label(pl.side)));
                        });
                        ui.label(theme::dim(now.map(|p| p.name.as_str()).unwrap_or(t!("leeg", "empty"))));
                        ui.label(theme::ic(icon::ARROW_RIGHT).color(theme::TEXT_FAINT));
                        ui.label(theme::label_strong(&b.name));
                        let (bf, nf) = (tx::fit(b, pl), now.map(|p| tx::fit(p, pl)));
                        theme::pill(ui, &match nf { Some(nf) => format!("{nf:.0} → {bf:.0}"), None => format!("{bf:.0}") }, theme::rgb(level_color(bf)));
                        if in_eleven.contains(&b.uid) {
                            ui.label(theme::faint(t!("(schuift door)", "(moves across)")));
                        }
                    });
                }
            }
            if changes == 0 {
                ui.label(theme::dim(t!("Je huidige elf is volgens deze berekening al de beste.", "By this calculation your current eleven is already the best.")));
            }
        });

        // een rol die beter bij de speler past
        let role_tips: Vec<(String, &'static str, &'static str, &'static str, f32, f32)> = order
            .iter()
            .filter_map(|&i| {
                let pl = &places[i];
                let p = pl.uid.and_then(|u| by_uid.get(&u))?;
                let (r, now, then) = tx::better_role(p, pl)?;
                Some((p.name.clone(), tx::ip_role(pl.ip?)?.code, r.code, r.name, now, then))
            })
            .collect();
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Rollen die beter passen", "Roles that fit better")));
            ui.add(
                egui::Label::new(theme::faint(t!(
                    "Een andere rol met bal in dezelfde linie waar de attributen van de speler duidelijk beter bij passen.",
                    "Another in-possession role in the same line that the player's attributes suit clearly better."
                )))
                .wrap(),
            );
            ui.add_space(6.0);
            if role_tips.is_empty() {
                ui.label(theme::dim(t!("Geen: elke speler staat in een rol die bij hem past.", "None: every player is in a role that suits him.")));
            }
            for (name, cur, code, full, now, then) in &role_tips {
                ui.horizontal(|ui| {
                    ui.label(theme::label_strong(name));
                    theme::pill(ui, &format!("{cur} {now:.0}"), theme::TEXT_DIM);
                    ui.label(theme::ic(icon::ARROW_RIGHT).color(theme::TEXT_FAINT));
                    theme::pill(ui, &format!("{code} {then:.0}"), theme::SUCCESS).on_hover_text(*full);
                    ui.label(theme::faint(*full));
                });
            }
        });

        ui.add(
            egui::Label::new(theme::faint(t!(
                "De rollen, posities en attributen komen uit FM. Welke attributen bij een rol horen is onze eigen inschatting, en links of rechts is afgeleid uit de volgorde van de plekken: zie de score als hulpmiddel, niet als cijfer van FM. Er wordt niets in FM veranderd.",
                "Roles, positions and attributes come from FM. Which attributes belong to a role is our own estimate, and left or right is derived from the order of the places: treat the score as a guide, not as a number from FM. Nothing is changed in FM."
            )))
            .wrap(),
        );
    }

    fn page_sell(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let busy = self.live.busy() || self.live.sell_busy();
        let connected = self.live.connected;
        let rate = self.settings.rate as f64;
        let mut undo = false;
        let buying = self.live.sell.buy;
        theme::page_header(
            ui,
            "Transfers",
            if buying {
                t!(
                    "Haal een speler van een andere club direct naar je eerste elftal. Jij betaalt de transfersom.",
                    "Bring a player from another club straight into your first team. You pay the fee."
                )
            } else {
                t!(
                    "Verkoop een speler direct aan een club naar keuze. De transfersom wordt meteen geboekt.",
                    "Sell a player straight to a club of your choice. The fee is booked at once."
                )
            },
            |ui| {
                if theme::button(ui, icon::ARROW_COUNTER_CLOCKWISE, t!("Ongedaan maken", "Undo"), connected && !busy).clicked() {
                    undo = true;
                }
            },
        );
        if undo {
            self.live.sell.plan = None;
            self.live.refresh(ctx, t!("ongedaan maken", "undo"), Some(json!({"cmd": "undo"})));
        }
        let (m, e) = (self.live.status.clone(), self.live.status_error);
        if !m.is_empty() && (e || undo) {
            self.status_line(ui, &m, e);
        }
        let (m, e) = (self.live.sell.status.clone(), self.live.sell.error);
        self.status_line(ui, &m, e);
        if !connected {
            self.connect_card(ui, ctx);
            return;
        }
        let mut buy = self.live.sell.buy;
        if theme::segmented(ui, &mut buy, &[(false, t!("Verkopen", "Sell")), (true, t!("Kopen", "Buy"))]) {
            self.live.sell.buy = buy;
            self.live.sell.plan = None;
            self.live.sell.status.clear();
        }
        ui.add_space(10.0);
        if self.live.sell.buy {
            self.page_buy(ui, ctx);
            return;
        }

        let eur = |pounds: i64| format!("€ {}", money((pounds as f64 * rate) as i64));
        let squad = self.live.squad.clone();
        let chosen = self.live.sell.player.and_then(|uid| squad.iter().find(|p| p.get("uid").and_then(|v| v.as_u64()) == Some(uid as u64)).cloned());
        let mut pick: Option<u32> = None;
        let mut search = false;
        let mut check: Option<(u32, u32, i64, Option<i64>)> = None;
        let mut confirm: Option<(u32, u32, i64, Option<i64>)> = None;

        ui.columns(2, |cols| {
            // 1. speler
            theme::card(&mut cols[0], |ui| {
                ui.label(theme::h2(t!("1 · Speler", "1 · Player")));
                ui.label(theme::faint(t!("Kies wie je wilt verkopen.", "Choose who to sell.")));
                ui.add_space(6.0);
                ScrollArea::vertical().id_salt("sell_players").max_height(430.0).show(ui, |ui| {
                    let mut sorted = squad.clone();
                    sorted.sort_by_key(|p| std::cmp::Reverse(p.get("value").and_then(|v| v.as_i64()).unwrap_or(0)));
                    for p in &sorted {
                        let uid = p.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let coach = p.get("player_coach").and_then(|v| v.as_bool()).unwrap_or(false);
                        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let value = p.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
                        let until = p.get("contract_ends").and_then(|d| d.get("year")).and_then(|y| y.as_u64());
                        let selected = self.live.sell.player == Some(uid);
                        ui.horizontal(|ui| {
                            let label = if coach { format!("{name} · {}", t!("speler-trainer", "player-coach")) } else { name.to_string() };
                            let r = ui.add_enabled(!coach, egui::SelectableLabel::new(selected, RichText::new(label).size(14.0)));
                            if r.clicked() {
                                pick = Some(uid);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let Some(y) = until {
                                    ui.label(theme::faint(format!("{} {y}", t!("tot", "until"))));
                                }
                                ui.label(RichText::new(eur(value)).color(theme::TEXT));
                                if p.get("listed").and_then(|v| v.as_u64()).unwrap_or(0) & 1 == 1 {
                                    theme::pill(ui, t!("transferlijst", "listed"), theme::WARNING);
                                }
                            });
                        });
                    }
                });
            });

            // 2. koper en 3. bedrag
            let ui = &mut cols[1];
            theme::card(ui, |ui| {
                ui.label(theme::h2(t!("2 · Koper", "2 · Buyer")));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.live.sell.query).hint_text(t!("Zoek een club, bijv. AC Milan", "Search a club, e.g. AC Milan")).desired_width(260.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        search = true;
                    }
                    if theme::small_button(ui, t!("Zoek", "Search"), !busy && self.live.sell.query.trim().len() >= 2).clicked() {
                        search = true;
                    }
                });
                // zoeken terwijl je typt
                let q = self.live.sell.query.trim().to_string();
                if q.len() >= 3 && q != self.live.sell.searched && !self.live.sell_busy() {
                    search = true;
                }
                ui.add_space(4.0);
                let clubs = self.live.sell.clubs.clone();
                for c in clubs.iter().take(8) {
                    let uid = c.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                    let selected = self.live.sell.club.as_ref().is_some_and(|(u, _)| *u == uid);
                    ui.horizontal(|ui| {
                        if ui.add(egui::SelectableLabel::new(selected, RichText::new(&name).size(14.0))).clicked() {
                            self.live.sell.club = Some((uid, name.clone()));
                            self.live.sell.plan = None;
                        }
                        if c.get("women").and_then(|v| v.as_bool()).unwrap_or(false) {
                            theme::pill(ui, t!("vrouwen", "women"), theme::INFO);
                        }
                    });
                }
                if clubs.is_empty() && !self.live.sell.searched.is_empty() && !self.live.sell_busy() {
                    ui.label(theme::faint(t!("Geen clubs gevonden.", "No clubs found.")));
                }
            });
            ui.add_space(12.0);
            theme::card(ui, |ui| {
                ui.label(theme::h2(t!("3 · Bedrag", "3 · Fee")));
                ui.add_space(4.0);
                let value = chosen.as_ref().and_then(|p| p.get("value")).and_then(|v| v.as_i64()).unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.live.sell.fee_input).hint_text("10").desired_width(90.0));
                    ui.label(t!("mln €", "M €"));
                    for (label, f) in [(t!("waarde", "value"), 1.0), ("×1.5", 1.5), ("×2", 2.0), ("×3", 3.0)] {
                        if theme::small_button(ui, label, value > 0).clicked() {
                            self.live.sell.fee_input = format!("{:.1}", value as f64 * f * rate / 1e6);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(t!("Nieuw salaris", "New wage"));
                    ui.add(egui::TextEdit::singleline(&mut self.live.sell.wage_input).hint_text(t!("blijft", "unchanged")).desired_width(90.0));
                    ui.label("k € p/w");
                });
                let fee = self.live.sell.fee_input.replace(',', ".").trim().parse::<f64>().ok().map(|m| (m * 1e6 / rate).round() as i64);
                let wage = self.live.sell.wage_input.replace(',', ".").trim().parse::<f64>().ok().map(|k| (k * 1e3 / rate).round() as i64);
                let key = match (self.live.sell.player, &self.live.sell.club, fee) {
                    (Some(p), Some((c, _)), Some(f)) if f >= 0 => Some((p, *c, f, wage)),
                    _ => None,
                };
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, icon::MAGNIFYING_GLASS, t!("Controleer", "Check"), key.is_some() && !busy).clicked() {
                        check = key;
                    }
                    let ready = self.live.sell.plan.as_ref().is_some_and(|(v, k)| Some(*k) == key && v.get("allowed").and_then(|a| a.as_bool()) == Some(true));
                    let label = match (&chosen, &self.live.sell.club) {
                        (Some(p), Some((_, c))) => t!(
                            format!("Verkoop {} aan {}", p.get("name").and_then(|v| v.as_str()).unwrap_or("?"), c),
                            format!("Sell {} to {}", p.get("name").and_then(|v| v.as_str()).unwrap_or("?"), c)
                        ),
                        _ => t!("Verkoop", "Sell").to_string(),
                    };
                    if theme::primary_button(ui, icon::HANDSHAKE, &label, ready && !busy).clicked() {
                        confirm = key;
                    }
                    if self.live.sell_busy() {
                        ui.spinner();
                    }
                });
                if let Some((v, k)) = &self.live.sell.plan {
                    if Some(*k) == key {
                        ui.add_space(8.0);
                        let s = |n: &str| v.get(n).and_then(|x| x.as_str()).unwrap_or("?").to_string();
                        let i = |n: &str| v.get(n).and_then(|x| x.as_i64()).unwrap_or(0);
                        let b = |n: &str| v.get(n).and_then(|x| x.as_bool()).unwrap_or(false);
                        let mut lines = vec![t!(
                            format!("{} gaat naar {} voor {}.", s("player"), s("to"), eur(i("fee"))),
                            format!("{} moves to {} for {}.", s("player"), s("to"), eur(i("fee")))
                        )];
                        lines.push(if b("buyer_pays") {
                            t!(format!("{} betaalt uit saldo en transferbudget.", s("to")), format!("{} pays from its balance and transfer budget.", s("to")))
                        } else {
                            t!(format!("{} heeft geen volledige financiën in je save: alleen jij krijgt het geld.", s("to")), format!("{} has no full finances in your save: only you receive the money.", s("to")))
                        });
                        lines.push(t!(
                            format!("Jouw saldo wordt {} en je transferbudget {}.", eur(i("balance_after")), eur(i("transfer_budget_after"))),
                            format!("Your balance becomes {} and your transfer budget {}.", eur(i("balance_after")), eur(i("transfer_budget_after")))
                        ));
                        if i("selections") > 0 {
                            lines.push(t!(format!("Zijn plek in {} opstelling(en) wordt leeg.", i("selections")), format!("His place in {} selection(s) is emptied.", i("selections"))));
                        }
                        if b("was_captain") {
                            lines.push(t!("Hij was aanvoerder: de band is vrij voor iemand anders.", "He was captain: the armband is free for someone else.").into());
                        }
                        if !b("allowed") {
                            lines.push(t!("Deze FM-versie is niet getest: sta aanpassen eerst toe in Instellingen.", "This FM build is untested: allow changes in Settings first.").into());
                        }
                        for l in lines {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(theme::ic(icon::CHECK_CIRCLE).color(theme::SUCCESS));
                                ui.label(l);
                            });
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Goed om te weten", "Good to know")));
            ui.add_space(4.0);
            for l in [
                t!("Dit is een directe verkoop, geen bod: FM zet de transfer niet in het nieuws of de transferhistorie.", "This is a direct sale, not an offer: FM does not put the transfer in the news or the transfer history."),
                t!("De speler krijgt bij de koper hetzelfde contract (of het nieuwe salaris), vanaf vandaag, zonder rugnummer.", "The player keeps his contract at the buyer (or the new wage), from today, without a squad number."),
                t!("Ongedaan maken zet alles terug zolang je nog niet verder hebt gespeeld. Maak eerst een backup en sla pas op als alles klopt.", "Undo puts everything back as long as you have not played on. Back up first and only save when everything is right."),
                t!("Speler-trainers en gehuurde spelers kunnen (nog) niet worden verkocht.", "Player-coaches and loaned players cannot be sold (yet)."),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::ic(icon::INFO).color(theme::INFO));
                    ui.label(theme::dim(l));
                });
            }
        });

        if let Some(uid) = pick {
            self.start_sale(uid);
        }
        if search {
            self.live.search_clubs(ctx);
        }
        if let Some((p, c, f, w)) = check {
            self.live.plan_sale(ctx, p, c, f, w);
        }
        if let Some((p, c, f, w)) = confirm {
            self.live.sell_now(ctx, p, c, f, w);
        }
    }

    /// Kopen: een speler van een andere club naar je eerste elftal halen.
    fn page_buy(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let busy = self.live.busy() || self.live.sell_busy();
        let rate = self.settings.rate as f64;
        let eur = |pounds: i64| format!("€ {}", money((pounds as f64 * rate) as i64));
        let club = self.live.sell.club.clone();
        let squad = self.live.sell.buy_squad.clone();
        let chosen = self.live.sell.buy_player.and_then(|uid| squad.iter().find(|p| p.get("uid").and_then(|v| v.as_u64()) == Some(uid as u64)).cloned());
        let mut search = false;
        let mut load: Option<u32> = None;
        let mut pick: Option<(u32, i64, Option<i64>)> = None;
        let mut check: Option<(u32, u32, i64, Option<i64>)> = None;
        let mut confirm: Option<(u32, u32, i64, Option<i64>)> = None;

        ui.columns(2, |cols| {
            // 1. club
            theme::card(&mut cols[0], |ui| {
                ui.label(theme::h2(t!("1 · Club", "1 · Club")));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.live.sell.query).hint_text(t!("Zoek een club, bijv. Real Madrid", "Search a club, e.g. Real Madrid")).desired_width(260.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        search = true;
                    }
                    if theme::small_button(ui, t!("Zoek", "Search"), !busy && self.live.sell.query.trim().len() >= 2).clicked() {
                        search = true;
                    }
                });
                let q = self.live.sell.query.trim().to_string();
                if q.len() >= 3 && q != self.live.sell.searched && !self.live.sell_busy() {
                    search = true;
                }
                ui.add_space(4.0);
                let clubs = self.live.sell.clubs.clone();
                for c in clubs.iter().take(8) {
                    let uid = c.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                    let selected = self.live.sell.club.as_ref().is_some_and(|(u, _)| *u == uid);
                    ui.horizontal(|ui| {
                        if ui.add(egui::SelectableLabel::new(selected, RichText::new(&name).size(14.0))).clicked() {
                            self.live.sell.club = Some((uid, name.clone()));
                            self.live.sell.plan = None;
                            load = Some(uid);
                        }
                        if c.get("women").and_then(|v| v.as_bool()).unwrap_or(false) {
                            theme::pill(ui, t!("vrouwen", "women"), theme::INFO);
                        }
                    });
                }
                if clubs.is_empty() && !self.live.sell.searched.is_empty() && !self.live.sell_busy() {
                    ui.label(theme::faint(t!("Geen clubs gevonden.", "No clubs found.")));
                }
            });
            cols[0].add_space(12.0);
            // 2. speler
            theme::card(&mut cols[0], |ui| {
                ui.label(theme::h2(t!("2 · Speler", "2 · Player")));
                match &club {
                    None => {
                        ui.label(theme::faint(t!("Kies eerst een club.", "Choose a club first.")));
                    }
                    Some((_, name)) => {
                        ui.label(theme::faint(t!(format!("Eerste elftal van {name}, duurste eerst."), format!("First team of {name}, most valuable first."))));
                        if squad.is_empty() && self.live.sell_busy() {
                            ui.spinner();
                        }
                        ui.add_space(6.0);
                        ScrollArea::vertical().id_salt("buy_players").max_height(360.0).show(ui, |ui| {
                            for p in &squad {
                                let uid = p.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let coach = p.get("player_coach").and_then(|v| v.as_bool()).unwrap_or(false);
                                let pname = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                                let value = p.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
                                let wage = p.get("wage").and_then(|v| v.as_i64());
                                let until = p.get("contract_ends").and_then(|d| d.get("year")).and_then(|y| y.as_u64());
                                let selected = self.live.sell.buy_player == Some(uid);
                                ui.horizontal(|ui| {
                                    let label = if coach { format!("{pname} · {}", t!("speler-trainer", "player-coach")) } else { pname.to_string() };
                                    if ui.add_enabled(!coach, egui::SelectableLabel::new(selected, RichText::new(label).size(14.0))).clicked() {
                                        pick = Some((uid, value, wage));
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(y) = until {
                                            ui.label(theme::faint(format!("{} {y}", t!("tot", "until"))));
                                        }
                                        if let Some(w) = wage {
                                            ui.label(theme::faint(format!("{} p/w", eur(w))));
                                        }
                                        ui.label(RichText::new(eur(value)).color(theme::TEXT));
                                    });
                                });
                            }
                        });
                    }
                }
            });

            // 3. bedrag
            let ui = &mut cols[1];
            theme::card(ui, |ui| {
                ui.label(theme::h2(t!("3 · Bedrag", "3 · Fee")));
                ui.add_space(4.0);
                let value = chosen.as_ref().and_then(|p| p.get("value")).and_then(|v| v.as_i64()).unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.live.sell.fee_input).hint_text("10").desired_width(90.0));
                    ui.label(t!("mln €", "M €"));
                    for (label, f) in [(t!("waarde", "value"), 1.0), ("×0.5", 0.5), ("×1.5", 1.5), ("0", 0.0)] {
                        if theme::small_button(ui, label, value > 0).clicked() {
                            self.live.sell.fee_input = format!("{:.1}", value as f64 * f * rate / 1e6);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(t!("Salaris bij jou", "Wage with you"));
                    ui.add(egui::TextEdit::singleline(&mut self.live.sell.wage_input).hint_text(t!("blijft", "unchanged")).desired_width(90.0));
                    ui.label("k € p/w");
                });
                let fee = self.live.sell.fee_input.replace(',', ".").trim().parse::<f64>().ok().map(|m| (m * 1e6 / rate).round() as i64);
                let wage = self.live.sell.wage_input.replace(',', ".").trim().parse::<f64>().ok().map(|k| (k * 1e3 / rate).round() as i64);
                let key = match (self.live.sell.buy_player, &self.live.sell.club, fee) {
                    (Some(p), Some((c, _)), Some(f)) if f >= 0 => Some((p, *c, f, wage)),
                    _ => None,
                };
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, icon::MAGNIFYING_GLASS, t!("Controleer", "Check"), key.is_some() && !busy).clicked() {
                        check = key;
                    }
                    let ready = self.live.sell.plan.as_ref().is_some_and(|(v, k)| Some(*k) == key && v.get("allowed").and_then(|a| a.as_bool()) == Some(true));
                    let label = match (&chosen, &club) {
                        (Some(p), Some((_, c))) => t!(
                            format!("Koop {} van {}", p.get("name").and_then(|v| v.as_str()).unwrap_or("?"), c),
                            format!("Buy {} from {}", p.get("name").and_then(|v| v.as_str()).unwrap_or("?"), c)
                        ),
                        _ => t!("Koop", "Buy").to_string(),
                    };
                    if theme::primary_button(ui, icon::USER_PLUS, &label, ready && !busy).clicked() {
                        confirm = key;
                    }
                    if self.live.sell_busy() {
                        ui.spinner();
                    }
                });
                if let Some((v, k)) = &self.live.sell.plan {
                    if Some(*k) == key {
                        ui.add_space(8.0);
                        let s = |n: &str| v.get(n).and_then(|x| x.as_str()).unwrap_or("?").to_string();
                        let i = |n: &str| v.get(n).and_then(|x| x.as_i64()).unwrap_or(0);
                        let b = |n: &str| v.get(n).and_then(|x| x.as_bool()).unwrap_or(false);
                        let mut lines = vec![t!(
                            format!("{} komt van {} naar jouw eerste elftal voor {}.", s("player"), s("from"), eur(i("fee"))),
                            format!("{} moves from {} to your first team for {}.", s("player"), s("from"), eur(i("fee")))
                        )];
                        lines.push(if b("buyer_pays") {
                            t!(format!("{} ontvangt het geld op saldo en transferbudget.", s("from")), format!("{} receives the money on its balance and transfer budget.", s("from")))
                        } else {
                            t!(format!("{} heeft geen volledige financiën in je save: alleen jij betaalt.", s("from")), format!("{} has no full finances in your save: only you pay.", s("from")))
                        });
                        lines.push(t!(
                            format!("Jouw saldo wordt {} en je transferbudget {}.", eur(i("balance_after")), eur(i("transfer_budget_after"))),
                            format!("Your balance becomes {} and your transfer budget {}.", eur(i("balance_after")), eur(i("transfer_budget_after")))
                        ));
                        if b("was_captain") {
                            lines.push(t!(format!("Hij was aanvoerder bij {}; die band wordt daar vrij.", s("from")), format!("He was captain at {}; that armband becomes free there.", s("from"))));
                        }
                        if !b("allowed") {
                            lines.push(t!("Deze FM-versie is niet getest: sta aanpassen eerst toe in Instellingen.", "This FM build is untested: allow changes in Settings first.").into());
                        }
                        for l in lines {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(theme::ic(icon::CHECK_CIRCLE).color(theme::SUCCESS));
                                ui.label(l);
                            });
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Goed om te weten", "Good to know")));
            ui.add_space(4.0);
            for l in [
                t!("Dit is een directe overgang, geen bod: FM zet de transfer niet in het nieuws of de transferhistorie.", "This is a direct move, not an offer: FM does not put the transfer in the news or the transfer history."),
                t!("De speler houdt zijn huidige contract (of krijgt het nieuwe salaris), vanaf vandaag bij jou, zonder rugnummer. Geef hem in FM een rugnummer en registreer hem waar nodig.", "The player keeps his current contract (or gets the new wage), from today at your club, without a squad number. Give him a number in FM and register him where needed."),
                t!("Ongedaan maken zet alles terug zolang je nog niet verder hebt gespeeld. Maak eerst een backup en sla pas op als alles klopt.", "Undo puts everything back as long as you have not played on. Back up first and only save when everything is right."),
                t!("Speler-trainers en gehuurde spelers kunnen (nog) niet worden overgenomen.", "Player-coaches and loaned players cannot be taken over (yet)."),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::ic(icon::INFO).color(theme::INFO));
                    ui.label(theme::dim(l));
                });
            }
        });
        if search {
            self.live.search_clubs(ctx);
        }
        if let Some(uid) = load {
            self.live.load_club_squad(ctx, uid);
        }
        if let Some((uid, value, wage)) = pick {
            self.live.sell.buy_player = Some(uid);
            self.live.sell.plan = None;
            self.live.sell.fee_input = format!("{:.1}", value as f64 * rate / 1e6);
            self.live.sell.wage_input = wage.map(|w| format!("{:.1}", w as f64 * rate / 1e3)).unwrap_or_default();
        }
        if let Some((p, c, f, w)) = check {
            self.live.plan_buy(ctx, p, c, f, w);
        }
        if let Some((p, c, f, w)) = confirm {
            self.live.buy_now(ctx, p, c, f, w);
        }
    }

    // -------------------------------------------------------------------- instellingen

    fn page_settings(&mut self, ui: &mut Ui) {
        theme::page_header(ui, t!("Instellingen", "Settings"), "", |_| {});
        let mut dirty = false;
        ui.columns(2, |cols| {
            theme::card(&mut cols[0], |ui| {
                ui.label(theme::h2(t!("Algemeen", "General")));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::TRANSLATE).color(theme::ACCENT));
                    ui.label(t!("Taal", "Language"));
                    let mut lang = self.settings.lang;
                    if theme::segmented(ui, &mut lang, &[(Lang::Nl, "Nederlands"), (Lang::En, "English")]) {
                        self.settings.lang = lang;
                        i18n::set(lang);
                        dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::COINS).color(theme::ACCENT));
                    ui.label(t!("Koers € per £", "Rate € per £"));
                    dirty |= ui.add(egui::DragValue::new(&mut self.settings.rate).range(0.5..=2.0).speed(0.01).fixed_decimals(2)).changed();
                });
                dirty |= ui.checkbox(&mut self.settings.capture_delay, t!("FM eerst naar voren halen bij uitlezen", "Bring FM to front before reading")).changed();
            });
            theme::card(&mut cols[1], |ui| {
                ui.label(theme::h2("Planner"));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t!("Team Bonding onder Buy-In", "Team Bonding below Buy-In"));
                    dirty |= ui.add(egui::Slider::new(&mut self.settings.planner.bonding_below, 0.0..=100.0).fixed_decimals(0)).changed();
                });
                ui.horizontal(|ui| {
                    ui.label(t!("Standaardsituaties elke (weken, 0 = uit)", "Set pieces every (weeks, 0 = off)"));
                    dirty |= ui.add(egui::DragValue::new(&mut self.settings.planner.set_pieces_every).range(0..=8)).changed();
                });
                ui.horizontal(|ui| {
                    ui.label(t!("Doelen bijstellen elke (weken)", "Re-target every (weeks)"));
                    dirty |= ui.add(egui::DragValue::new(&mut self.settings.planner.replan_every_weeks).range(1..=8)).changed();
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t!("Rust", "Rest"));
                    use planner::Load;
                    dirty |= theme::segmented(
                        ui,
                        &mut self.settings.planner.load,
                        &[(Load::Light, t!("Meer rust", "More rest")), (Load::Balanced, t!("Gebalanceerd", "Balanced")), (Load::Intense, t!("Minder rust", "Less rest"))],
                    );
                });
                ui.add(
                    egui::Label::new(theme::dim(match self.settings.planner.load {
                        planner::Load::Light => t!(
                            "Elke trainingsdag en de dag voor een wedstrijd eindigt met Rest.",
                            "Every training day and the day before a match ends with Rest."
                        ),
                        planner::Load::Balanced => t!("Het gewone plan.", "The regular plan."),
                        planner::Load::Intense => t!(
                            "De dag na een wedstrijd krijgt na Recovery ook een tactische sessie.",
                            "The day after a match gets a tactical session after Recovery."
                        ),
                    }))
                    .wrap(),
                );
                ui.add_space(6.0);
                dirty |= ui
                    .checkbox(&mut self.settings.planner.no_tactical, t!("Geen tactische sessies", "No tactical sessions"))
                    .changed();
                ui.add(
                    egui::Label::new(theme::dim(t!(
                        "Die plekken gaan naar spelersontwikkeling; wedstrijdvoorbereiding blijft. Bedoeld voor wie het tactisch begrip op 100 % laat houden (Selectie).",
                        "Those slots go to player development; match preparation stays. Meant for use with \"Keep at 100 %\" for tactical understanding (Squad)."
                    )))
                    .wrap(),
                );
            });
        });
        ui.add_space(14.0);
        if agent::LIVE_SUPPORTED {
            theme::card(ui, |ui| {
                ui.label(theme::h2(t!("Live in FM", "Live in FM")));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let mut on = self.settings.allow_untested;
                    if theme::toggle(ui, &mut on).changed() {
                        self.settings.allow_untested = on;
                        self.live.allow_untested = on;
                        dirty = true;
                        if self.live.connected {
                            self.live.refresh(ui.ctx(), "", None);
                        }
                    }
                    ui.label(RichText::new(t!("Aanpassen op een niet-geteste FM-versie", "Allow changes on an untested FM build")).strong());
                });
                ui.add(egui::Label::new(theme::dim(t!(
                    "FM27 Trainer past alleen iets aan op FM-versies die het kent (Steam op Windows en Mac, Game Pass / Xbox-app op Windows). Op andere versies (na een FM-update, of bijvoorbeeld Epic) staat de app op alleen lezen. Kloppen club, budget en selectie in de app met wat FM laat zien? Dan kun je aanpassen hier toestaan. Elke wijziging wordt eerst gecontroleerd, kan ongedaan worden gemaakt en blijft pas bewaard als je in FM opslaat. Maak eerst een backup van je save.",
                    "FM27 Trainer only changes things on FM builds it knows (Steam on Windows and Mac, Game Pass / Xbox app on Windows). On other builds (after an FM update, or Epic for example) the app is read-only. Do club, budget and squad in the app match what FM shows? Then you can allow changes here. Every change is checked first, can be undone and only sticks once you save in FM. Back up your save first."
                )))
                .wrap());
                if self.live.connected {
                    ui.add_space(4.0);
                    let (c, txt) = if !self.live.untested_build {
                        (theme::SUCCESS, t!("Deze FM-versie is getest.", "This FM build is tested."))
                    } else if self.settings.allow_untested {
                        (theme::WARNING, t!("Niet-geteste FM-versie: aanpassen toegestaan.", "Untested FM build: changes allowed."))
                    } else {
                        (theme::WARNING, t!("Niet-geteste FM-versie: alleen lezen.", "Untested FM build: read-only."))
                    };
                    ui.horizontal(|ui| {
                        theme::status_dot(ui, c);
                        ui.label(RichText::new(txt).color(theme::TEXT));
                    });
                }
            });
            ui.add_space(14.0);
        }
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Mappen en toestemming", "Folders and permissions")));
            ui.add_space(6.0);
            ui.label(theme::dim(format!("Export: {}", export::output_dir().display())));
            ui.label(theme::dim(format!("{}: {}", t!("Instellingen", "Settings"), crate::data_dir().display())));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::button(ui, icon::EXPORT, t!("Open exportmap", "Open export folder"), true).clicked() {
                    let _ = std::fs::create_dir_all(export::output_dir());
                    open_path(&export::output_dir());
                }
                // daar staat ook de laatste schermopname (last-breakdown.png): handig om mee te sturen
                // als een meter verkeerd gelezen wordt
                if theme::button(ui, icon::EXPORT, t!("Open app-map (laatste schermopname en uitlezing)", "Open app folder (last screenshot and reading)"), true).clicked() {
                    let _ = std::fs::create_dir_all(crate::data_dir());
                    open_path(&crate::data_dir());
                }
                if cfg!(target_os = "macos") && theme::button(ui, icon::CAMERA, t!("Schermopname-toestemming", "Screen Recording permission"), true).clicked() {
                    open_path(std::path::Path::new("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"));
                }
            });
        });
        ui.add_space(14.0);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Steun de ontwikkeling", "Support development")));
            ui.add_space(6.0);
            ui.add(egui::Label::new(theme::dim(t!(
                "FM27 Trainer is gratis en blijft gratis. Wil je iets terugdoen voor het werk, dan kan dat met een donatie. Alle functies werken ook zonder.",
                "FM27 Trainer is free and stays free. If you want to give something back for the work, you can donate. Every feature works without it."
            )))
            .wrap());
            ui.add_space(6.0);
            if theme::button(ui, icon::HEART, t!("Doneer via Ko-fi", "Donate on Ko-fi"), true).clicked() {
                open_path(std::path::Path::new(DONATE_URL));
            }
        });
        ui.add_space(14.0);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Over", "About")));
            ui.add(egui::Label::new(theme::dim(t!(
                "Sessienamen, limieten en presets komen uit FM27's eigen game-data. De module leest en schrijft alleen na controle, met undo. Alleen voor single-player. Lettertype: Inter (SIL OFL). Iconen: Phosphor (MIT).",
                "Session names, limits and presets come from FM27's own game data. The module only reads and writes after checks, with undo. Single-player only. Font: Inter (SIL OFL). Icons: Phosphor (MIT)."
            )))
            .wrap());
        });
        if dirty {
            self.save_settings();
            self.week_plan = None;
            self.year.clear();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.snapshot(ctx);
        self.poll_analysis(ctx);
        // automatisch uitlezen: niet tijdens handmatig aanpassen en nooit tijdens een testopname
        if self.settings.auto_read
            && !self.edit_mode
            && self.analysis_rx.is_none()
            && std::env::var_os("FM27_SNAPSHOT").is_none()
            && self.last_auto.elapsed().as_secs() >= self.settings.auto_read_secs.max(5) as u64
        {
            self.last_auto = std::time::Instant::now();
            self.auto_pending = true;
            self.start_analysis(ctx, None);
        }
        self.live.allow_untested = self.settings.allow_untested;
        self.live.choice = (self.settings.manager_club, self.settings.training_team);
        self.live.keep_understanding = self.settings.keep_understanding;
        self.live.tick(ctx, &self.eff, &self.settings.planner, self.settings.auto_training);
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin { left: 28, right: 28, top: 24, bottom: 16 }))
            .show(ctx, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(1400.0));
                    // pagina komt zacht binnen: kort vervagen en een paar pixels omhoog schuiven
                    if self.shown_page != Some(self.page) {
                        self.shown_page = Some(self.page);
                        theme::restart_enter(ctx);
                    }
                    let t = theme::enter(ctx, 0.22);
                    ui.set_opacity(0.25 + 0.75 * t);
                    ui.add_space(8.0 * (1.0 - t));
                    match self.page {
                        Page::Overview => self.page_overview(ui, ctx),
                        Page::Training => self.page_training(ui, ctx),
                        Page::Planning => self.page_planning(ui),
                        Page::Club => self.page_club(ui, ctx),
                        Page::Squad => self.page_squad(ui, ctx),
                        Page::Tactics => self.page_tactics(ui, ctx),
                        Page::Sell => self.page_sell(ui, ctx),
                        Page::Settings => self.page_settings(ui),
                    }
                    ui.add_space(20.0);
                });
            });
        if self.analysis_rx.is_some() || self.live.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.settings.auto_read {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(5));
        }
    }
}

// ------------------------------------------------------------------------ helpers

/// Donatiepagina (knop op Instellingen).
const DONATE_URL: &str = "https://ko-fi.com/semhiel";

fn open_path(p: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).status();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).status();
}

fn money(v: i64) -> String {
    let a = v.unsigned_abs() as f64;
    let s = if a >= 1e6 { format!("{:.1} M", a / 1e6) } else if a >= 1e3 { format!("{:.0}k", a / 1e3) } else { format!("{a:.0}") };
    if v < 0 { format!("-{s}") } else { s }
}

/// De acht onderdelen van FM's Tactical Understanding, in de volgorde van de module.
const UNDERSTANDING_AREAS: [&str; 8] = ["Passing Style", "Build Up", "Chance Creation", "In Possession Shape", "Line of Engagement", "Pressing Intensity", "Pressing Trap", "Out Of Possession Shape"];

fn morale_label(m: u64) -> &'static str {
    match m {
        0..=4 => "Abysmal",
        5..=7 => "Very Poor",
        8..=10 => "Poor",
        11..=13 => "Okay",
        14..=16 => "Good",
        17..=18 => "Very Good",
        _ => "Superb",
    }
}

/// Korte, leesbare sessienaam voor in chips.
fn chip_name(name: &str) -> String {
    name.replace("Out Of Possession", "OOP")
        .replace("In Possession", "IP")
        .replace("Defensive", "Def.")
        .replace("Attacking", "Att.")
        .replace("Possession Ball Work", "Poss. Ball Work")
        .replace("Conditioning", "Cond.")
        .replace("Individual Development", "Individual")
        .replace("Line of Engagement", "Line of Eng.")
        .replace("Pressing Intensity", "Press. Intensity")
}

fn chip(ui: &mut Ui, name: &str, highlight: bool, faded: bool) {
    let col = if name == "Match" { theme::DANGER } else { theme::rgb(category_of(name).color()) };
    let col = if faded { theme::tint(col, 0.45) } else { col };
    theme::session_chip(ui, &chip_name(name), col, highlight).on_hover_text(name);
}

fn step_card(ui: &mut Ui, n: u8, icon_s: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            Frame::new().fill(theme::ACCENT_SOFT).corner_radius(CornerRadius::same(20)).inner_margin(Margin::symmetric(8, 2)).show(ui, |ui| {
                ui.label(RichText::new(n.to_string()).color(theme::ACCENT).strong());
            });
            ui.label(theme::ic(icon_s).color(theme::ACCENT).size(16.0));
            ui.label(theme::h2(title));
        });
        ui.add_space(4.0);
        body(ui);
    });
}

fn codes_text(c: &[u16; 3]) -> String {
    c.iter().map(|x| chip_name(session_by_code(*x).unwrap_or("?"))).collect::<Vec<_>>().join(" · ")
}

fn week_card(ui: &mut Ui, w: &live_training::WeekPreview, today: Option<NaiveDate>, fm_had: &std::collections::HashMap<NaiveDate, [u16; 3]>) {
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::h2(t!(format!("Week van {}", i18n::date_short(w.start)), format!("Week of {}", i18n::date_short(w.start)))));
            if w.rest {
                theme::pill(ui, t!("Rustweek: blijft van FM", "Rest week: kept from FM"), theme::WARNING);
                return;
            }
            theme::pill(ui, &t!(format!("{} wedstrijd(en)", w.matches), format!("{} match(es)", w.matches)), theme::DANGER);
            theme::pill(ui, &w.focus, theme::ACCENT);
            let pending = (0..7).filter(|d| w.current[*d] != w.planned[*d]).count();
            if w.written > 0 {
                theme::pill(ui, &t!(format!("{} dagen teruggezet", w.written), format!("{} days restored", w.written)), theme::SUCCESS);
            } else if pending > 0 {
                theme::pill(ui, &t!(format!("{pending} dagen wijken af van je plan"), format!("{pending} days differ from your plan")), theme::WARNING);
            } else {
                theme::pill(ui, t!("volgens plan", "on plan"), theme::SUCCESS);
            }
        });
        if w.rest {
            return;
        }
        ui.add_space(8.0);
        ui.columns(7, |cols| {
            for d in 0..7 {
                let ui = &mut cols[d];
                let date = w.start + Duration::days(d as i64);
                let past = today.is_some_and(|t| date <= t);
                ui.label(theme::caps(i18n::weekday_short(d)));
                ui.label(theme::faint(i18n::date_short(date)));
                // wat FM wilde: nu afwijkend (wordt aangepast) of eerder teruggezet
                let differs = w.current[d] != w.planned[d];
                let was = if differs { Some(w.current[d]) } else { fm_had.get(&date).copied().filter(|h| *h != w.planned[d]) };
                for code in w.planned[d] {
                    let name = session_by_code(code).unwrap_or("?");
                    chip(ui, name, was.is_some() && !past, past || !w.editable[d]);
                }
                if let Some(h) = was.filter(|_| !past && w.editable[d]) {
                    ui.add(egui::Label::new(RichText::new(format!("FM: {}", codes_text(&h))).size(10.5).color(theme::TEXT_FAINT).strikethrough()).truncate())
                        .on_hover_text(t!(
                            format!("FM had ingepland: {}", codes_text(&h)),
                            format!("FM had planned: {}", codes_text(&h))
                        ));
                }
            }
        });
    });
}

fn plan_grid(ui: &mut Ui, p: &WeekPlan) {
    ui.columns(7, |cols| {
        for (i, d) in p.days.iter().enumerate() {
            let ui = &mut cols[i];
            ui.label(theme::caps(i18n::weekday_short(i)));
            ui.label(theme::faint(format!("{} · {}", i18n::date_short(d.date), d.role.label())));
            for s in &d.slots {
                chip(ui, s, false, false);
            }
        }
    });
}

fn legend(ui: &mut Ui) {
    use crate::model::Category::*;
    ui.horizontal_wrapped(|ui| {
        ui.label(theme::faint(t!("Omlijnd = door FM27 Trainer gezet · doorgestreept = wat FM wilde ·", "Outlined = set by FM27 Trainer · struck through = what FM wanted ·")));
        for c in [TacticalUnderstanding, MatchPreparation, FootballingDevelopment, PhysicalRecovery, ExtraCurricular, Fixed] {
            theme::status_dot(ui, theme::rgb(c.color()));
            ui.label(theme::faint(c.label()));
            ui.add_space(8.0);
        }
    });
}

fn kind_combo(ui: &mut Ui, id: &str, kind: &mut WeekKind) {
    egui::ComboBox::from_id_salt(id).selected_text(kind.label()).width(170.0).show_ui(ui, |ui| {
        for k in WeekKind::ALL {
            ui.selectable_value(kind, k, k.label());
        }
    });
}

fn weekday_combo(ui: &mut Ui, id: &str, day: &mut Weekday) {
    let names = [(Weekday::Fri, t!("vrijdag", "Friday")), (Weekday::Sat, t!("zaterdag", "Saturday")), (Weekday::Sun, t!("zondag", "Sunday"))];
    let cur = names.iter().find(|(d, _)| d == day).map(|(_, n)| *n).unwrap_or("—");
    egui::ComboBox::from_id_salt(id).selected_text(cur).show_ui(ui, |ui| {
        for (d, n) in names {
            ui.selectable_value(day, d, n);
        }
    });
}
