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
    understanding: Option<Value>,
    /// uitkomst van het op 100 % houden, als er iets teruggezet is
    kept: Option<Value>,
    human_flags: Option<Value>,
    teams: Option<Value>,
    message: String,
    error: bool,
    untested: Option<bool>,
}

/// Wat de verkooppagina bijhoudt.
#[derive(Default)]
pub struct SellState {
    pub player: Option<u32>,
    pub query: String,
    /// de zoekopdracht waarvoor `clubs` geldt
    pub searched: String,
    pub clubs: Vec<Value>,
    pub club: Option<(u32, String)>,
    pub fee_input: String,
    pub wage_input: String,
    /// uitkomst van de controle, met de (speler, club, som, salaris) waarvoor hij geldt
    pub plan: Option<(Value, (u32, u32, i64, Option<i64>))>,
    pub status: String,
    pub error: bool,
    /// kopen in plaats van verkopen
    pub buy: bool,
    /// de selectie van de gekozen club (om iemand te kopen) en voor welke club die geldt
    pub buy_squad: Vec<Value>,
    pub squad_club: Option<u32>,
    pub buy_player: Option<u32>,
}

#[derive(Clone, Copy)]
enum Aux {
    Clubs,
    Plan((u32, u32, i64, Option<i64>)),
    Sale,
    Squad,
    BuyPlan((u32, u32, i64, Option<i64>)),
    Buy,
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
    /// tactisch begrip van de selectie (opdracht `understanding`)
    pub understanding: Option<Value>,
    /// editorvlaggen van de manager (opdracht `human_flags`)
    pub human_flags: Option<Value>,
    pub training: Option<Outcome>,
    pub status: String,
    pub status_error: bool,
    pub train_status: String,
    pub train_error: bool,
    pub transfer_input: String,
    pub wage_input: String,
    /// Aanpassen op een FM-build die nog niet live getest is (instelling van de gebruiker).
    pub allow_untested: bool,
    /// Tactisch begrip op 100 % houden (instelling van de gebruiker).
    pub keep_understanding: bool,
    /// De build van FM is niet getest: de module staat op alleen lezen (tenzij toegestaan).
    pub untested_build: bool,
    pub sell: SellState,
    /// tactieken en attributen van de selectie, voor de tactiekhulp (op verzoek opgehaald)
    pub tactics: Option<crate::tactics::Data>,
    pub tactics_error: String,
    /// managers in de save en teams van de club (opdracht `select`; None bij een oudere module)
    pub teams: Option<Value>,
    /// keuze van de gebruiker: (UID van de club van de manager, plek van het team voor de training)
    pub choice: (Option<u32>, Option<usize>),
    tactics_rx: Option<mpsc::Receiver<Result<Value, String>>>,
    aux_rx: Option<mpsc::Receiver<(Aux, Result<Value, String>)>>,
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
            understanding: None,
            human_flags: None,
            training: None,
            status: String::new(),
            status_error: false,
            train_status: String::new(),
            train_error: false,
            transfer_input: String::new(),
            wage_input: String::new(),
            allow_untested: false,
            keep_understanding: false,
            untested_build: false,
            sell: SellState::default(),
            tactics: None,
            tactics_error: String::new(),
            teams: None,
            choice: (None, None),
            tactics_rx: None,
            aux_rx: None,
            rx: None,
            train_rx: None,
            last_auto: None,
            last_poll: None,
        }
    }
}

/// Geeft de keuze van manager en team door aan de module; het antwoord is de lijst van managers en teams.
fn select(choice: (Option<u32>, Option<usize>)) -> anyhow::Result<Value> {
    agent::request(json!({"cmd": "select", "club": choice.0, "team": choice.1}))
}

fn short_result(v: &Value) -> String {
    match v {
        Value::Object(m) if m.contains_key("added") => {
            let mut s = t!(
                format!("({} spelers, {} waarden naar 100 %, {} toegevoegd", m["players"], m["items"], m["added"]),
                format!("({} players, {} values to 100%, {} added", m["players"], m["items"], m["added"]), format!("({} jogadores, {} valores para 100%, {} adicionados", m["players"], m["items"], m["added"])
            );
            if m.get("no_room").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                s += &t!(
                    format!("; bij {} spelers is de lijst vol: train een week en probeer opnieuw", m["no_room"]),
                    format!("; {} players have a full list: train a week and try again", m["no_room"]), format!("; {} jogadores têm a lista cheia: treine uma semana e tente de novo", m["no_room"])
                );
            }
            s + ")"
        }
        Value::Object(m) if m.contains_key("players") => format!("({} {})", m["players"], t!("spelers", "players", "jogadores")),
        Value::Object(m) if m.contains_key("bans") => format!("({} {})", m["bans"], t!("schorsingen", "bans", "suspensões")),
        Value::Object(m) if m.contains_key("undone") => format!("({})", m["undone"].as_str().unwrap_or("")),
        _ => String::new(),
    }
}

impl LiveState {
    /// Na een taalwissel: de standaardmelding "verbonden" opnieuw in de nieuwe taal.
    pub fn retranslate(&mut self) {
        if ["Verbonden met FM27.", "Connected to FM27.", "Conectado ao FM27."].contains(&self.status.as_str()) {
            self.status = crate::t!("Verbonden met FM27.", "Connected to FM27.", "Conectado ao FM27.").into();
        }
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some() || self.train_rx.is_some()
    }

    /// Loopt er een zoek-, controle- of verkoopopdracht?
    pub fn sell_busy(&self) -> bool {
        self.aux_rx.is_some()
    }

    fn aux(&mut self, ctx: &egui::Context, kind: Aux, req: Value) {
        if self.aux_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((kind, agent::request(req).map_err(|e| format!("{e:#}"))));
            ctx2.request_repaint();
        });
        self.aux_rx = Some(rx);
    }

    /// Clubs zoeken op naam.
    pub fn search_clubs(&mut self, ctx: &egui::Context) {
        let q = self.sell.query.trim().to_string();
        self.sell.searched = q.clone();
        self.aux(ctx, Aux::Clubs, json!({"cmd": "clubs", "query": q, "limit": 12}));
    }

    /// Controleren (er wordt niets geschreven).
    pub fn plan_sale(&mut self, ctx: &egui::Context, uid: u32, club: u32, fee: i64, wage: Option<i64>) {
        self.sell.plan = None;
        self.sell.status = t!("Controleren…", "Checking…", "Verificando…").into();
        self.sell.error = false;
        self.aux(ctx, Aux::Plan((uid, club, fee, wage)), json!({"cmd": "sell", "uid": uid, "club": club, "fee": fee, "wage": wage, "dry_run": true}));
    }

    pub fn sell_now(&mut self, ctx: &egui::Context, uid: u32, club: u32, fee: i64, wage: Option<i64>) {
        self.sell.status = t!("Verkopen…", "Selling…", "Vendendo…").into();
        self.sell.error = false;
        self.aux(ctx, Aux::Sale, json!({"cmd": "sell", "uid": uid, "club": club, "fee": fee, "wage": wage}));
    }

    /// De selectie van een andere club ophalen, om daar iemand uit te kopen.
    pub fn load_club_squad(&mut self, ctx: &egui::Context, club: u32) {
        self.sell.squad_club = Some(club);
        self.sell.buy_squad.clear();
        self.sell.buy_player = None;
        self.aux(ctx, Aux::Squad, json!({"cmd": "club_squad", "club": club}));
    }

    /// Een aankoop controleren (er wordt niets geschreven).
    pub fn plan_buy(&mut self, ctx: &egui::Context, uid: u32, club: u32, fee: i64, wage: Option<i64>) {
        self.sell.plan = None;
        self.sell.status = t!("Controleren…", "Checking…", "Verificando…").into();
        self.sell.error = false;
        self.aux(ctx, Aux::BuyPlan((uid, club, fee, wage)), json!({"cmd": "buy", "uid": uid, "club": club, "fee": fee, "wage": wage, "dry_run": true}));
    }

    pub fn buy_now(&mut self, ctx: &egui::Context, uid: u32, club: u32, fee: i64, wage: Option<i64>) {
        self.sell.status = t!("Kopen…", "Buying…", "Comprando…").into();
        self.sell.error = false;
        self.aux(ctx, Aux::Buy, json!({"cmd": "buy", "uid": uid, "club": club, "fee": fee, "wage": wage}));
    }

    /// De tactieken en de attributen van de selectie ophalen (alleen lezen).
    pub fn load_tactics(&mut self, ctx: &egui::Context) {
        if self.tactics_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(agent::request(json!({"cmd": "tactics"})).map_err(|e| format!("{e:#}")));
            ctx2.request_repaint();
        });
        self.tactics_rx = Some(rx);
    }

    pub fn tactics_busy(&self) -> bool {
        self.tactics_rx.is_some()
    }

    /// Een andere manager of een ander team: wat van het vorige team was, vervalt.
    pub fn choose(&mut self, ctx: &egui::Context, choice: (Option<u32>, Option<usize>)) {
        self.choice = choice;
        self.training = None;
        self.fm_had.clear();
        self.tactics = None;
        self.last_auto = Some(Instant::now());
        self.last_poll = None;
        // loopt er nog een opdracht voor het vorige team, dan volgt de verversing daarna vanzelf
        if !self.busy() {
            self.refresh(ctx, "", None);
        }
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
        let allow = self.allow_untested;
        let choice = self.choice;
        // een testopname (FM27_SNAPSHOT) mag nooit iets in FM veranderen
        let keep = self.keep_understanding && std::env::var_os("FM27_SNAPSHOT").is_none();
        std::thread::spawn(move || {
            let mut snap = Snapshot::default();
            // eerst de keuze van de gebruiker doorgeven (oudere modules kennen dit niet: geen probleem)
            let _ = agent::request(json!({"cmd": "allow_untested", "on": allow}));
            // welke manager en welk team: vóór al het andere, zodat club en selectie erbij horen
            snap.teams = select(choice).ok();
            if let Some(req) = action {
                match agent::request(req) {
                    Ok(v) => snap.message = format!("{label}: {} {}", t!("klaar", "done", "concluído"), short_result(&v)),
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
                // op 100 % houden: de module schrijft alleen wat gezakt is (op een niet-geteste build
                // weigert hij, dat is dan al gemeld)
                if keep {
                    if let Ok(r) = agent::request(json!({"cmd": "max_understanding"})) {
                        if r.get("players").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                            snap.kept = Some(r);
                        }
                    }
                }
                if let Ok(u) = agent::request(json!({"cmd": "understanding"})) {
                    snap.understanding = Some(u);
                }
                if let Ok(f) = agent::request(json!({"cmd": "human_flags"})) {
                    snap.human_flags = Some(f);
                }
                // onbekende FM-versie: de module staat dan op alleen-lezen
                if let Ok(p) = agent::request(json!({"cmd": "ping"})) {
                    let untested = p.get("known_build").and_then(|v| v.as_bool()) == Some(false);
                    snap.untested = Some(untested);
                    let allowed = p.get("untested_writes").and_then(|v| v.as_bool()) == Some(true);
                    if untested && !allowed && snap.message.is_empty() {
                        snap.message = t!(
                            "Deze FM-versie is nog niet getest met FM27 Trainer: alleen lezen. Controleer of club en selectie kloppen met FM; aanpassen kun je in Instellingen aanzetten.",
                            "This FM build has not been verified with FM27 Trainer yet: read-only. Check that club and squad match FM; you can allow changes in Settings.", "Esta versão do FM ainda não foi verificada com o FM27 Trainer: só leitura. Confira se clube e elenco batem com o FM; você pode permitir alterações em Configurações."
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
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (eff, settings) = (eff.clone(), settings.clone());
        let _ = only_new;
        let skip: HashSet<NaiveDate> = HashSet::new();
        let ctx2 = ctx.clone();
        let choice = self.choice;
        std::thread::spawn(move || {
            // de keuze kan net veranderd zijn: nooit in de kalender van het vorige team schrijven
            let _ = select(choice);
            let r = live_training::run(&eff, &settings, write, &skip).map_err(|e| format!("{e:#}"));
            let _ = tx.send((write, auto, r));
            ctx2.request_repaint();
        });
        self.train_rx = Some(rx);
        if !auto {
            self.train_status = if write { t!("Training wordt in FM gezet…", "Writing training into FM…", "Escrevendo o treino no FM…").into() } else { t!("Plan wordt gemaakt…", "Building plan…", "Montando o plano…").into() };
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
                if let Some(u) = s.understanding {
                    self.understanding = Some(u);
                }
                if let Some(f) = s.human_flags {
                    self.human_flags = Some(f);
                }
                if self.connected {
                    self.teams = s.teams;
                }
                if let Some(r) = s.kept {
                    let n = r.get("players").and_then(|v| v.as_u64()).unwrap_or(0);
                    let text = t!(
                        format!("Tactisch begrip teruggezet naar 100 % voor {n} spelers (FM had het laten zakken)."),
                        format!("Tactical understanding put back to 100% for {n} players (FM had let it drop)."), format!("Entrosamento tático devolvido a 100% para {n} jogadores (o FM tinha deixado cair).")
                    );
                    self.log.insert(0, LogEntry { at: chrono::Local::now(), text: text.clone(), auto: true });
                    self.log.truncate(40);
                    if s.message.is_empty() {
                        self.status = text;
                        self.status_error = false;
                    }
                }
                if let Some(u) = s.untested {
                    self.untested_build = u;
                }
                if !s.message.is_empty() || was != self.connected {
                    self.status = if s.message.is_empty() { t!("Verbonden met FM27.", "Connected to FM27.", "Conectado ao FM27.").into() } else { s.message };
                    self.status_error = s.error;
                }
            }
        }
        if let Some(rx) = &self.tactics_rx {
            if let Ok(r) = rx.try_recv() {
                self.tactics_rx = None;
                match r.and_then(|v| serde_json::from_value::<crate::tactics::Data>(v).map_err(|e| e.to_string())) {
                    Ok(d) => {
                        self.tactics = Some(d);
                        self.tactics_error.clear();
                    }
                    Err(e) => self.tactics_error = e,
                }
            }
        }
        if let Some(rx) = &self.aux_rx {
            if let Ok((kind, r)) = rx.try_recv() {
                self.aux_rx = None;
                match (kind, r) {
                    (Aux::Clubs, Ok(Value::Array(a))) => self.sell.clubs = a,
                    (Aux::Clubs, Ok(_)) => self.sell.clubs.clear(),
                    (Aux::Squad, Ok(Value::Array(a))) => self.sell.buy_squad = a,
                    (Aux::Squad, Ok(_)) => self.sell.buy_squad.clear(),
                    (Aux::Plan(key), Ok(v)) | (Aux::BuyPlan(key), Ok(v)) => {
                        self.sell.status.clear();
                        self.sell.plan = Some((v, key));
                    }
                    (Aux::Buy, Ok(v)) => {
                        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("?").to_string();
                        self.sell.status = t!(
                            format!("{} is gekocht van {} en staat in je selectie. Klopt er iets niet? Gebruik Ongedaan maken voordat je opslaat.", s("player"), s("from")),
                            format!("{} has been bought from {} and is in your squad. Something wrong? Use Undo before you save.", s("player"), s("from")), format!("{} foi comprado do {} e está no seu elenco. Algo errado? Use Desfazer antes de salvar.", s("player"), s("from"))
                        );
                        self.sell.error = false;
                        self.sell.plan = None;
                        self.sell.buy_player = None;
                        if let Some(c) = self.sell.squad_club {
                            self.load_club_squad(ctx, c);
                        }
                        self.refresh(ctx, "", None);
                    }
                    (Aux::Sale, Ok(v)) => {
                        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("?").to_string();
                        self.sell.status = t!(
                            format!("{} is verkocht aan {}. Klopt er iets niet? Gebruik Ongedaan maken voordat je opslaat.", s("player"), s("to")),
                            format!("{} has been sold to {}. Something wrong? Use Undo before you save.", s("player"), s("to")), format!("{} foi vendido para {}. Algo errado? Use Desfazer antes de salvar.", s("player"), s("to"))
                        );
                        self.sell.error = false;
                        self.sell.plan = None;
                        self.sell.player = None;
                        self.refresh(ctx, "", None);
                    }
                    (_, Err(e)) => {
                        self.sell.status = e;
                        self.sell.error = true;
                    }
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
                                        format!("Week of {date}: {} days put back on plan (FM had re-planned the week).", w.written), format!("Semana de {date}: {} dias devolvidos ao plano (o FM tinha replanejado a semana).", w.written)
                                    )
                                } else {
                                    t!(format!("Week van {date}: {} dagen in FM gezet.", w.written), format!("Week of {date}: {} days written into FM.", w.written), format!("Semana de {date}: {} dias escritos no FM.", w.written))
                                };
                                self.log.insert(0, LogEntry { at: chrono::Local::now(), text, auto });
                            }
                            self.log.truncate(40);
                            self.fixed_total += o.written_days;
                        }
                        if !auto {
                            self.train_status = if !wrote {
                                t!("Plan klaar. Groen = wordt aangepast.", "Plan ready. Green = will change.", "Plano pronto. Verde = vai mudar.").into()
                            } else if o.written_days == 0 {
                                t!("Alles staat al volgens plan.", "Everything already follows the plan.", "Tudo já segue o plano.").into()
                            } else {
                                t!(format!("{} dagen in FM gezet.", o.written_days), format!("{} days written into FM.", o.written_days), format!("{} dias escritos no FM.", o.written_days))
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
        if due && !self.busy() && agent::module_ready() {
            self.last_poll = Some(Instant::now());
            self.refresh(ctx, "", None);
        }
        // nieuwe effectiveness → komende weken opnieuw plannen
        // automatische modus: alle komende weken volgens plan houden. FM bouwt een week opnieuw op als
        // de wedstrijden erin veranderen (loting, verplaatsing); dat zetten we hier weer recht.
        let read_only = self.untested_build && !self.allow_untested;
        // een testopname (FM27_SNAPSHOT) mag nooit iets in FM veranderen
        let snapshot = std::env::var_os("FM27_SNAPSHOT").is_some();
        if auto && !snapshot && self.connected && !read_only && !self.busy() && self.last_auto.is_none_or(|t| t.elapsed().as_secs() >= 8) {
            self.last_auto = Some(Instant::now());
            self.training_job(ctx, eff, settings, true, false, true);
        }
    }
}
