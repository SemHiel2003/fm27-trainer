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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FM27 Trainer")
            .with_inner_size([1320.0, 880.0])
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
    auto_training: bool,
    planner: PlannerSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { lang: i18n::detect(), rate: 1.17, capture_delay: true, auto_training: false, planner: PlannerSettings::default() }
    }
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
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum PlanTab {
    Week,
    Season,
}

type AnalysisResult = Result<(vision::Analysis, String), String>;

struct App {
    page: Page,
    settings: Settings,
    eff: Effectiveness,
    status: String,
    status_error: bool,
    analysis_rx: Option<mpsc::Receiver<AnalysisResult>>,
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
        // FM27_PAGE=training|planning|club|squad|settings opent direct die pagina (handig bij testen)
        let page = match std::env::var("FM27_PAGE").unwrap_or_default().as_str() {
            "training" => Page::Training,
            "planning" => Page::Planning,
            "club" => Page::Club,
            "squad" => Page::Squad,
            "settings" => Page::Settings,
            _ => Page::Overview,
        };
        App {
            page,
            eff: crate::load_effectiveness().unwrap_or_default(),
            settings,
            status: String::new(),
            status_error: false,
            analysis_rx: None,
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
            live: LiveState::default(),
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
        let delay = self.settings.capture_delay && file.is_none();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            if delay && cfg!(target_os = "macos") {
                let _ = std::process::Command::new("open").args(["-b", "com.SportsInteractive.FootballManager27"]).status();
                std::thread::sleep(std::time::Duration::from_millis(1500));
            }
            let res: AnalysisResult = match file {
                Some(p) => vision::analyze_file(&p).map(|(a, _)| (a, p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())).map_err(|e| format!("{e:#}")),
                None => vision::analyze_screen().map(|(a, _)| (a, t!("FM27-scherm", "FM27 screen").to_string())).map_err(|e| format!("{e:#}")),
            };
            let _ = tx.send(res);
            ctx2.request_repaint();
        });
        self.analysis_rx = Some(rx);
        self.set_status(t!("Bezig met uitlezen…", "Reading…"), false);
    }

    fn poll_analysis(&mut self) {
        let Some(rx) = &self.analysis_rx else { return };
        if let Ok(res) = rx.try_recv() {
            self.analysis_rx = None;
            match res {
                Ok((a, src)) => {
                    let when = chrono::Local::now().format("%d-%m %H:%M");
                    self.eff = a.to_effectiveness(&format!("{src} · {when}"));
                    let _ = crate::save_effectiveness(&self.eff);
                    let msg = t!(format!("{} van 17 meters gelezen.", a.gauges.len()), format!("Read {} of 17 gauges.", a.gauges.len()));
                    self.set_status(msg, false);
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
            .frame(Frame::new().fill(theme::SIDEBAR).inner_margin(Margin { left: 14, right: 14, top: 18, bottom: 14 }).stroke(Stroke::new(1.0, theme::BORDER)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    Frame::new().fill(theme::ACCENT).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(7)).show(ui, |ui| {
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
                ui.label(theme::h2(t!("Live in FM is nog alleen beschikbaar op macOS", "Live in FM is currently macOS only")));
                ui.label(theme::dim(t!(
                    "Uitlezen, planning en export werken ook op Windows. De module die rechtstreeks in FM schrijft, volgt voor Windows.",
                    "Reading, planning and export also work on Windows. The module that writes straight into FM is coming to Windows."
                )));
                return;
            }
            ui.horizontal(|ui| {
                ui.label(theme::ic(icon::PLUGS_CONNECTED).size(28.0).color(theme::ACCENT));
                ui.vertical(|ui| {
                    ui.label(theme::h2(t!("Verbind met Football Manager", "Connect to Football Manager")));
                    ui.label(theme::dim(t!(
                        "De app praat rechtstreeks met FM27 via een kleine module die met FM meestart.",
                        "The app talks to FM27 directly through a small module that starts with FM."
                    )));
                });
            });
            ui.add_space(10.0);
            for (n, s) in [
                (1, t!("Sla je spel op en sluit FM27.", "Save your game and quit FM27.")),
                (2, t!("Klik op 'Start FM met module' (Steam moet open zijn).", "Click 'Start FM with module' (Steam must be open).")),
                (3, t!("Laad je save. De app verbindt vanzelf.", "Load your save. The app connects automatically.")),
            ] {
                ui.horizontal(|ui| {
                    Frame::new().fill(theme::ACCENT_SOFT).corner_radius(CornerRadius::same(20)).inner_margin(Margin::symmetric(8, 2)).show(ui, |ui| {
                        ui.label(RichText::new(n.to_string()).color(theme::ACCENT).strong());
                    });
                    ui.label(s);
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if theme::primary_button(ui, icon::PLAY, t!("Start FM met module", "Start FM with module"), !self.live.busy()).clicked() {
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
            self.start_analysis(ctx, None);
        }
        if file {
            self.show_file = !self.show_file;
        }
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
        let can = agent::LIVE_SUPPORTED && self.live.connected && !self.live.busy();
        let (mut view, mut write) = (false, false);
        theme::page_header(
            ui,
            "Training",
            t!("Automatisch afgestemd op je zwakste gebieden, rechtstreeks in FM's kalender.", "Tuned to your weakest areas automatically, written straight into FM's calendar."),
            |ui| {
                if theme::primary_button(ui, icon::LIGHTNING, t!("Zet training in FM", "Apply training in FM"), can).clicked() {
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
        if !self.has_data() {
            let msg = t!(
                "Nog geen effectiveness uitgelezen; het plan gebruikt standaardwaarden. Lees eerst de Breakdown uit (Overzicht).",
                "No effectiveness read yet; the plan uses defaults. Read the Breakdown first (Overview)."
            );
            self.status_line(ui, msg, true);
        }
        if !agent::LIVE_SUPPORTED || !self.live.connected {
            self.connect_card(ui, ctx);
            return;
        }

        ui.columns(3, |cols| {
            theme::card(&mut cols[0], |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::TARGET).color(theme::ACCENT).size(16.0));
                    ui.label(theme::caps("Focus"));
                });
                ui.add_space(4.0);
                for (f, v) in self.eff.weakest().into_iter().filter(|(f, _)| f.is_tactical_area()).take(3) {
                    theme::pill(ui, &format!("{} · {v:.0}", f.label()), theme::rgb(level_color(v)));
                }
            });
            theme::card(&mut cols[1], |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::CALENDAR).color(theme::INFO).size(16.0));
                    ui.label(theme::caps(t!("Spel-datum", "Game date")));
                });
                let d = self.live.training.as_ref().and_then(|o| o.today).map(i18n::date).unwrap_or_else(|| "—".into());
                ui.label(RichText::new(d).font(egui::FontId::new(20.0, theme::bold())));
                ui.label(theme::faint(t!("Alleen dagen hierna worden aangepast", "Only days after this are changed")));
            });
            theme::card(&mut cols[2], |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::SPARKLE).color(theme::SUCCESS).size(16.0));
                    ui.label(theme::caps(t!("Automatisch", "Automatic")));
                });
                if ui.checkbox(&mut self.settings.auto_training, t!("Nieuwe weken automatisch inplannen", "Plan new weeks automatically")).changed() {
                    self.save_settings();
                }
                ui.label(theme::faint(t!("Zolang de app open is (FM houdt ~4 weken vooruit bij)", "While the app is open (FM keeps ~4 weeks ahead)")));
            });
        });
        ui.add_space(14.0);

        let Some(o) = self.live.training.clone() else {
            theme::card(ui, |ui| {
                ui.label(theme::dim(t!("Klik op 'Bekijk plan' om de komende weken te zien.", "Click 'Preview' to see the coming weeks.")));
            });
            if !self.live.busy() {
                self.live.training(ctx, &self.eff, &self.settings.planner, false, false);
            }
            return;
        };
        for w in &o.weeks {
            week_card(ui, w, o.today);
            ui.add_space(10.0);
        }
        legend(ui);
    }

    // -------------------------------------------------------------------- planning

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
                if theme::primary_button(ui, icon::SPARKLE, t!("Maak weekplan", "Build week plan"), true).clicked() {
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
        theme::card(ui, |ui| {
            egui::Grid::new("squad").num_columns(7).spacing([18.0, 10.0]).striped(true).show(ui, |ui| {
                for h in [t!("Speler", "Player"), t!("Moraal", "Morale"), t!("Conditie", "Condition"), t!("Blessure", "Injury"), t!("Schorsing", "Ban"), t!("Ontevreden", "Unhappy"), ""] {
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
                    });
                    ui.end_row();
                }
            });
        });
        if let Some((l, r)) = action {
            self.live.refresh(ctx, &l, Some(r));
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
            });
        });
        ui.add_space(14.0);
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
                if cfg!(target_os = "macos") && theme::button(ui, icon::CAMERA, t!("Schermopname-toestemming", "Screen Recording permission"), true).clicked() {
                    open_path(std::path::Path::new("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"));
                }
            });
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
        self.poll_analysis();
        self.live.tick(ctx, &self.eff, &self.settings.planner, self.settings.auto_training);
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin { left: 28, right: 28, top: 24, bottom: 16 }))
            .show(ctx, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(1400.0));
                    match self.page {
                        Page::Overview => self.page_overview(ui, ctx),
                        Page::Training => self.page_training(ui, ctx),
                        Page::Planning => self.page_planning(ui),
                        Page::Club => self.page_club(ui, ctx),
                        Page::Squad => self.page_squad(ui, ctx),
                        Page::Settings => self.page_settings(ui),
                    }
                    ui.add_space(20.0);
                });
            });
        if self.analysis_rx.is_some() || self.live.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(5));
        }
    }
}

// ------------------------------------------------------------------------ helpers

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

fn week_card(ui: &mut Ui, w: &live_training::WeekPreview, today: Option<NaiveDate>) {
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::h2(t!(format!("Week van {}", i18n::date_short(w.start)), format!("Week of {}", i18n::date_short(w.start)))));
            theme::pill(ui, &t!(format!("{} wedstrijd(en)", w.matches), format!("{} match(es)", w.matches)), theme::DANGER);
            theme::pill(ui, &w.focus, theme::ACCENT);
            let changed = (0..7).filter(|d| w.current[*d] != w.planned[*d]).count();
            if w.written > 0 {
                theme::pill(ui, &t!(format!("{} dagen gezet", w.written), format!("{} days applied", w.written)), theme::SUCCESS);
            } else if changed > 0 {
                theme::pill(ui, &t!(format!("{changed} dagen wijzigen"), format!("{changed} days change")), theme::SUCCESS);
            }
        });
        ui.add_space(8.0);
        ui.columns(7, |cols| {
            for d in 0..7 {
                let ui = &mut cols[d];
                let date = w.start + Duration::days(d as i64);
                let past = today.is_some_and(|t| date <= t);
                ui.label(theme::caps(i18n::weekday_short(d)));
                ui.label(theme::faint(i18n::date_short(date)));
                let changed = w.current[d] != w.planned[d];
                for code in w.planned[d] {
                    let name = session_by_code(code).unwrap_or("?");
                    chip(ui, name, changed, past || !w.editable[d]);
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
