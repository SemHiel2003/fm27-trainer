//! Toestand van de verbinding met FM27 (via de module): club, selectie en training.
//! Alle opdrachten lopen in de achtergrond; de UI leest alleen deze toestand.

use chrono::{Duration, NaiveDate};
use eframe::egui;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::Instant;

use crate::agent;
use crate::live_training::{self, Outcome};
use crate::model::Effectiveness;
use crate::planner::PlannerSettings;
use crate::t;

#[derive(Default)]
struct Snapshot {
    club: Option<Value>,
    squad: Option<Vec<Value>>,
    message: String,
    error: bool,
}

/// Regel in het logboek van de trainingspagina.
#[derive(Clone)]
pub struct LogEntry {
    pub at: chrono::DateTime<chrono::Local>,
    pub text: String,
    pub auto: bool,
}

pub struct LiveState {
    pub connected: bool,
    /// Wat er de laatste keer gebeurde met de training (nieuwste eerst).
    pub log: Vec<LogEntry>,
    pub last_check: Option<chrono::DateTime<chrono::Local>>,
    /// Dagen die deze sessie zijn (terug)gezet.
    pub fixed_total: usize,
    /// Per dag: wat FM had ingepland voordat de app het terugzette.
    pub fm_had: HashMap<NaiveDate, [u16; 3]>,
    pub club: Option<Value>,
    pub squad: Vec<Value>,
    pub training: Option<Outcome>,
    pub status: String,
    pub status_error: bool,
    pub train_status: String,
    pub train_error: bool,
    pub transfer_input: String,
    pub wage_input: String,
    rx: Option<mpsc::Receiver<Snapshot>>,
    train_rx: Option<mpsc::Receiver<(bool, bool, Result<Outcome, String>)>>,
    last_auto: Option<Instant>,
    last_poll: Option<Instant>,
}

impl Default for LiveState {
    fn default() -> Self {
        LiveState {
            connected: false,
            log: Vec::new(),
            last_check: None,
            fixed_total: 0,
            fm_had: HashMap::new(),
            club: None,
            squad: Vec::new(),
            training: None,
            status: String::new(),
            status_error: false,
            train_status: String::new(),
            train_error: false,
            transfer_input: String::new(),
            wage_input: String::new(),
            rx: None,
            train_rx: None,
            last_auto: None,
            last_poll: None,
        }
    }
}

fn short_result(v: &Value) -> String {
    match v {
        Value::Object(m) if m.contains_key("players") => format!("({} {})", m["players"], t!("spelers", "players")),
        Value::Object(m) if m.contains_key("bans") => format!("({} {})", m["bans"], t!("schorsingen", "bans")),
        Value::Object(m) if m.contains_key("undone") => format!("({})", m["undone"].as_str().unwrap_or("")),
        _ => String::new(),
    }
}

impl LiveState {
    pub fn busy(&self) -> bool {
        self.rx.is_some() || self.train_rx.is_some()
    }

    pub fn club_name(&self) -> Option<String> {
        self.club.as_ref()?.get("name")?.as_str().map(str::to_string)
    }

    /// Voert (optioneel) een opdracht uit en haalt daarna club + selectie op.
    pub fn refresh(&mut self, ctx: &egui::Context, label: &str, action: Option<Value>) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        let label = label.to_string();
        std::thread::spawn(move || {
            let mut snap = Snapshot::default();
            if let Some(req) = action {
                match agent::request(req) {
                    Ok(v) => snap.message = format!("{label}: {} {}", t!("klaar", "done"), short_result(&v)),
                    Err(e) => {
                        snap.message = format!("{label}: {e:#}");
                        snap.error = true;
                    }
                }
            }
            match agent::request(json!({"cmd": "club"})) {
                Ok(c) => snap.club = Some(c),
                Err(e) => {
                    if snap.message.is_empty() {
                        snap.message = format!("{e:#}");
                        snap.error = true;
                    }
                }
            }
            if snap.club.is_some() {
                if let Ok(Value::Array(a)) = agent::request(json!({"cmd": "squad"})) {
                    snap.squad = Some(a);
                }
                // onbekende FM-versie: de module staat dan op alleen-lezen
                if let Ok(p) = agent::request(json!({"cmd": "ping"})) {
                    if p.get("known_build").and_then(|v| v.as_bool()) == Some(false) {
                        snap.message = t!(
                            "Deze FM-versie is nog niet getest met FM27 Trainer: alleen lezen. Update de app voor aanpassingen.",
                            "This FM build has not been verified with FM27 Trainer yet: read-only. Update the app to make changes."
                        )
                        .into();
                        snap.error = true;
                    }
                }
            }
            let _ = tx.send(snap);
            ctx2.request_repaint();
        });
        self.rx = Some(rx);
    }

    pub fn start_fm(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let r = agent::start_fm();
            let _ = tx.send(Snapshot {
                message: match &r {
                    Ok(m) => m.clone(),
                    Err(e) => format!("{e:#}"),
                },
                error: r.is_err(),
                ..Default::default()
            });
            ctx2.request_repaint();
        });
        self.rx = Some(rx);
    }

    pub fn training(&mut self, ctx: &egui::Context, eff: &Effectiveness, settings: &PlannerSettings, write: bool, only_new: bool) {
        self.training_job(ctx, eff, settings, write, only_new, false);
    }

    fn training_job(&mut self, ctx: &egui::Context, eff: &Effectiveness, settings: &PlannerSettings, write: bool, only_new: bool, auto: bool) {
        if self.train_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (eff, settings) = (eff.clone(), settings.clone());
        let _ = only_new;
        let skip: HashSet<NaiveDate> = HashSet::new();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let r = live_training::run(&eff, &settings, write, &skip).map_err(|e| format!("{e:#}"));
            let _ = tx.send((write, auto, r));
            ctx2.request_repaint();
        });
        self.train_rx = Some(rx);
        if !auto {
            self.train_status = if write { t!("Training wordt in FM gezet…", "Writing training into FM…").into() } else { t!("Plan wordt gemaakt…", "Building plan…").into() };
            self.train_error = false;
        }
    }

    /// Elke frame: resultaten ophalen, automatisch verbinden en (optioneel) automatisch plannen.
    pub fn tick(&mut self, ctx: &egui::Context, eff: &Effectiveness, settings: &PlannerSettings, auto: bool) {
        if let Some(rx) = &self.rx {
            if let Ok(s) = rx.try_recv() {
                self.rx = None;
                let was = self.connected;
                self.connected = s.club.is_some();
                if s.club.is_some() {
                    self.club = s.club;
                }
                if let Some(sq) = s.squad {
                    self.squad = sq;
                }
                if !s.message.is_empty() || was != self.connected {
                    self.status = if s.message.is_empty() { t!("Verbonden met FM27.", "Connected to FM27.").into() } else { s.message };
                    self.status_error = s.error;
                }
            }
        }
        if let Some(rx) = &self.train_rx {
            if let Ok((wrote, auto, r)) = rx.try_recv() {
                self.train_rx = None;
                self.last_check = Some(chrono::Local::now());
                match r {
                    Ok(o) => {
                        if wrote {
                            // per week: wat FM had en wat er is teruggezet
                            for w in o.weeks.iter().filter(|w| w.written > 0) {
                                for d in 0..7 {
                                    if w.editable[d] && w.current[d] != w.planned[d] {
                                        self.fm_had.insert(w.start + Duration::days(d as i64), w.current[d]);
                                    }
                                }
                                let date = crate::i18n::date_short(w.start);
                                let text = if auto {
                                    t!(
                                        format!("Week van {date}: {} dagen teruggezet naar je plan (FM had de week opnieuw ingepland).", w.written),
                                        format!("Week of {date}: {} days put back on plan (FM had re-planned the week).", w.written)
                                    )
                                } else {
                                    t!(format!("Week van {date}: {} dagen in FM gezet.", w.written), format!("Week of {date}: {} days written into FM.", w.written))
                                };
                                self.log.insert(0, LogEntry { at: chrono::Local::now(), text, auto });
                            }
                            self.log.truncate(40);
                            self.fixed_total += o.written_days;
                        }
                        if !auto {
                            self.train_status = if !wrote {
                                t!("Plan klaar. Groen = wordt aangepast.", "Plan ready. Green = will change.").into()
                            } else if o.written_days == 0 {
                                t!("Alles staat al volgens plan.", "Everything already follows the plan.").into()
                            } else {
                                t!(format!("{} dagen in FM gezet.", o.written_days), format!("{} days written into FM.", o.written_days))
                            };
                            self.train_error = false;
                        }
                        self.training = Some(o);
                    }
                    Err(e) => {
                        self.train_status = e;
                        self.train_error = true;
                    }
                }
            }
        }
        if !agent::LIVE_SUPPORTED {
            return;
        }
        // verbinding bijhouden: elke 20 s (of meteen als er een socket is maar nog geen verbinding)
        let due = self.last_poll.is_none_or(|t| t.elapsed().as_secs() >= if self.connected { 20 } else { 5 });
        if due && !self.busy() && crate::data_dir().join("agent.sock").exists() {
            self.last_poll = Some(Instant::now());
            self.refresh(ctx, "", None);
        }
        // nieuwe effectiveness → komende weken opnieuw plannen
        // automatische modus: alle komende weken volgens plan houden. FM bouwt een week opnieuw op als
        // de wedstrijden erin veranderen (loting, verplaatsing); dat zetten we hier weer recht.
        if auto && self.connected && !self.busy() && self.last_auto.is_none_or(|t| t.elapsed().as_secs() >= 8) {
            self.last_auto = Some(Instant::now());
            self.training_job(ctx, eff, settings, true, false, true);
        }
    }
}
