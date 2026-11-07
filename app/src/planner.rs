//! Weekplanner en jaarplanner.
//!
//! Het ritme komt uit de presets van Sports Interactive zelf (`training_schedule_templates` in
//! de game-data van FM27): wedstrijddag = Rest/Match/Rest, de dag erna Recovery/Rest/Rest, de
//! dag ervoor drie Match Preparation-sessies, één Light Conditioning in een week met één
//! wedstrijd, (Heavy) Conditioning in de voorbereiding. Waar SI vaste tactische sessies zet,
//! kiest deze planner de sessies van de zwakste Tactical Areas, met de limieten uit de
//! game-data: elke Tactical Understanding-sessie max 8× per week (categorie max 16), elke
//! Match Preparation-sessie max 1× per week, Team Bonding max 2× per week.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::model::{Effectiveness, Factor, Phase};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeekKind {
    PreSeasonEarly,
    PreSeasonMid,
    PreSeasonLate,
    Season,
    InternationalBreak,
    WinterBreak,
    OffSeason,
}

impl WeekKind {
    pub const ALL: [WeekKind; 7] = [
        WeekKind::PreSeasonEarly,
        WeekKind::PreSeasonMid,
        WeekKind::PreSeasonLate,
        WeekKind::Season,
        WeekKind::InternationalBreak,
        WeekKind::WinterBreak,
        WeekKind::OffSeason,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WeekKind::PreSeasonEarly => crate::t!("Voorbereiding (begin)", "Pre-season (early)"),
            WeekKind::PreSeasonMid => crate::t!("Voorbereiding (midden)", "Pre-season (mid)"),
            WeekKind::PreSeasonLate => crate::t!("Voorbereiding (eind)", "Pre-season (late)"),
            WeekKind::Season => crate::t!("Seizoen", "Season"),
            WeekKind::InternationalBreak => crate::t!("Interlandperiode", "International break"),
            WeekKind::WinterBreak => crate::t!("Winterstop", "Winter break"),
            WeekKind::OffSeason => crate::t!("Vakantie", "Off-season"),
        }
    }

    fn is_preseason(self) -> bool {
        matches!(self, WeekKind::PreSeasonEarly | WeekKind::PreSeasonMid | WeekKind::PreSeasonLate)
    }
}

/// Eén week van de kalender: maandag t/m zondag, met de wedstrijddagen.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarWeek {
    pub start: NaiveDate,
    pub kind: WeekKind,
    pub matches: [bool; 7],
    #[serde(default)]
    pub note: String,
}

impl CalendarWeek {
    pub fn match_count(&self) -> usize {
        self.matches.iter().filter(|m| **m).count()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayRole {
    Match,
    PostMatch,
    PreMatch,
    PostAndPreMatch,
    Training,
    Recovery,
    Holiday,
}

impl DayRole {
    pub fn label(self) -> &'static str {
        match self {
            DayRole::Match => crate::t!("Wedstrijd", "Match day"),
            DayRole::PostMatch => crate::t!("Dag na wedstrijd", "Day after match"),
            DayRole::PreMatch => crate::t!("Dag voor wedstrijd", "Day before match"),
            DayRole::PostAndPreMatch => crate::t!("Tussen 2 wedstrijden", "Between matches"),
            DayRole::Training => crate::t!("Trainingsdag", "Training day"),
            DayRole::Recovery => crate::t!("Herstel", "Recovery"),
            DayRole::Holiday => crate::t!("Vrij", "Day off"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DayPlan {
    pub date: NaiveDate,
    pub role: DayRole,
    /// session1, session2, extra — exact de FM27-sessienamen.
    pub slots: [String; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct WeekPlan {
    pub start: NaiveDate,
    pub kind: WeekKind,
    pub matches: usize,
    pub days: Vec<DayPlan>,
    /// Schedule Focus zoals FM27 hem noemt.
    pub focus: &'static str,
    /// Hoe vaak elke Tactical Area deze week getraind wordt.
    pub targets: Vec<(Factor, u32)>,
    pub notes: Vec<String>,
    /// Geschatte stand aan het eind van de week (alleen in het jaarplan).
    pub projected: Vec<(Factor, f32)>,
    /// Voorgestelde naam om het schema in FM op te slaan.
    pub schedule_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannerSettings {
    /// Onder deze Buy-In-score komt er Team Bonding na wedstrijden.
    pub bonding_below: f32,
    /// Elke n-de seizoensweek Corners/Free Kicks/Penalties op de dag voor de wedstrijd.
    pub set_pieces_every: u32,
    /// Geschatte winst per tactische sessie (punten op de 0–100-schaal, bij 0 begonnen).
    pub gain_per_session: f32,
    /// Geschat verval per week voor een gebied dat niet getraind is.
    pub decay_per_week: f32,
    /// Plan per blok van zoveel weken met dezelfde doelen, zodat weekschema's terugkomen.
    #[serde(default = "default_block")]
    pub replan_every_weeks: u32,
}

fn default_block() -> u32 {
    4
}

impl Default for PlannerSettings {
    fn default() -> Self {
        PlannerSettings { bonding_below: 60.0, set_pieces_every: 0, gain_per_session: 2.2, decay_per_week: 0.8, replan_every_weeks: 4 }
    }
}

// ------------------------------------------------------------------- kalender

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarOptions {
    /// Eerste dag van het plan (wordt naar maandag afgerond).
    pub start: NaiveDate,
    pub weeks: u32,
    /// Dag waarop de competitiewedstrijd valt.
    pub league_day: Weekday,
    /// Europees voetbal op donderdag (Europa/Conference) of dinsdag/woensdag (Champions League).
    pub europe: Option<Weekday>,
    pub cup: bool,
}

impl Default for CalendarOptions {
    fn default() -> Self {
        let today = chrono::Local::now().date_naive();
        CalendarOptions { start: today, weeks: 52, league_day: Weekday::Sun, europe: Some(Weekday::Thu), cup: true }
    }
}

pub fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// Standaard seizoenskalender in Eredivisie-ritme: voorbereiding in juli, competitie van
/// augustus tot half mei, interlandperiodes in sep/okt/nov/mrt, winterstop eind december,
/// vakantie eind mei–juni. Alle weken zijn in de app per dag aan te passen.
pub fn default_calendar(opts: &CalendarOptions) -> Vec<CalendarWeek> {
    let mut out = Vec::new();
    let first = monday_of(opts.start);
    let mut euro_count = 0;
    let mut cup_count = 0;
    for w in 0..opts.weeks {
        let start = first + Duration::weeks(w as i64);
        let mid = start + Duration::days(3);
        let (m, d) = (mid.month(), mid.day());
        let kind = match (m, d) {
            (6, _) => WeekKind::OffSeason,
            (5, 20..) => WeekKind::OffSeason,
            (7, 1..=10) => WeekKind::PreSeasonEarly,
            (7, 11..=21) => WeekKind::PreSeasonMid,
            (7, _) => WeekKind::PreSeasonLate,
            (9, 1..=10) | (10, 8..=15) | (11, 10..=17) | (3, 22..=29) => WeekKind::InternationalBreak,
            (12, 22..) | (1, 1..=8) => WeekKind::WinterBreak,
            _ => WeekKind::Season,
        };
        let mut matches = [false; 7];
        let idx = |wd: Weekday| wd.num_days_from_monday() as usize;
        let mut note = String::new();
        match kind {
            WeekKind::PreSeasonMid | WeekKind::PreSeasonLate => matches[idx(Weekday::Sat)] = true,
            WeekKind::PreSeasonEarly if d > 4 => matches[idx(Weekday::Sat)] = true,
            WeekKind::Season => {
                matches[idx(opts.league_day)] = true;
                // Europees: competitiefase sep–jan (8 speelrondes), knock-out feb–mrt
                if let Some(eday) = opts.europe {
                    let euro_week = matches!((m, d), (9, 15..) | (10, _) | (11, _) | (12, 1..=14) | (1, 18..) | (2, _) | (3, 1..=20))
                        && w % 2 == 0;
                    if euro_week && euro_count < 14 {
                        matches[idx(eday)] = true;
                        euro_count += 1;
                        note = crate::t!("Europees", "Europe").into();
                    }
                }
                // Beker: midweek in okt, dec, jan, feb, apr
                if opts.cup && !matches.iter().filter(|x| **x).count().gt(&1) {
                    let cup_week = matches!((m, d), (10, 26..) | (12, 15..=21) | (1, 12..=18) | (2, 8..=14) | (4, 18..=24));
                    if cup_week && cup_count < 5 {
                        matches[idx(Weekday::Wed)] = true;
                        cup_count += 1;
                        note = crate::t!("Beker", "Cup").into();
                    }
                }
            }
            _ => {}
        }
        out.push(CalendarWeek { start, kind, matches, note });
    }
    out
}

// ------------------------------------------------------------------- planner

const MATCH_PREP_IP: [&str; 3] = ["Press Avoidance", "Ball Progression", "Final Third"];
const MATCH_PREP_OOP: [&str; 3] = ["Defensive Block", "Defensive Positioning", "Defensive Engagement"];
const SET_PIECES: [&str; 3] = ["Corners", "Free Kicks", "Penalties"];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    Tactical,
    MatchPrep,
    Fixed(&'static str),
    Extra,
}

struct WeekState<'a> {
    /// Team Bonding dat nog voor dagen na een wedstrijd vrijgehouden wordt.
    bonding_reserved: u32,
    deficit: HashMap<Factor, f32>,
    count: HashMap<String, u32>,
    eff: &'a Effectiveness,
    settings: &'a PlannerSettings,
}

impl WeekState<'_> {
    fn used(&self, name: &str) -> u32 {
        self.count.get(name).copied().unwrap_or(0)
    }

    fn take(&mut self, name: &str) -> String {
        *self.count.entry(name.to_string()).or_default() += 1;
        name.to_string()
    }

    fn tactical_total(&self) -> u32 {
        Factor::TACTICAL_AREAS.iter().map(|f| self.used(f.label())).sum()
    }

    /// Kies het tactische gebied met het grootste resterende tekort (niet al vandaag gekozen).
    fn pick_tactical(&mut self, today: &[String]) -> Option<String> {
        if self.tactical_total() >= 16 {
            return None; // categorielimiet Tactical Understanding
        }
        let best = Factor::TACTICAL_AREAS
            .iter()
            .filter(|f| !today.iter().any(|t| t == f.label()) && self.used(f.label()) < 8)
            .max_by(|a, b| self.deficit[a].total_cmp(&self.deficit[b]))
            .copied()?;
        if let Some(d) = self.deficit.get_mut(&best) {
            *d *= 0.55; // afnemend: daarna komt het volgende zwakke gebied aan de beurt
        }
        Some(self.take(best.label()))
    }

    fn pick_match_prep(&mut self, today: &[String], set_piece_week: bool) -> String {
        let ip = self.eff.phase_score(Phase::InPossession);
        let oop = self.eff.phase_score(Phase::OutOfPossession);
        let mut order: Vec<&str> = Vec::new();
        if set_piece_week {
            order.extend(SET_PIECES);
        }
        if oop <= ip {
            order.extend(MATCH_PREP_OOP);
            order.extend(MATCH_PREP_IP);
        } else {
            order.extend(MATCH_PREP_IP);
            order.extend(MATCH_PREP_OOP);
        }
        for name in order {
            if self.used(name) == 0 && !today.iter().any(|t| t == name) {
                return self.take(name);
            }
        }
        // alle specifieke sessies al gebruikt: laat FM zelf kiezen
        "Match Preparation".to_string()
    }

    fn pick_extra(&mut self, today: &[String]) -> String {
        let buy_in = self.eff.buy_in();
        if buy_in < self.settings.bonding_below && self.used("Team Bonding") + self.bonding_reserved < 2 {
            return self.take("Team Bonding");
        }
        // erg lage tactical understanding: extra tactische sessie, anders individuele ontwikkeling
        let tu = (self.eff.phase_score(Phase::InPossession) + self.eff.phase_score(Phase::OutOfPossession)) / 2.0;
        if tu < 45.0 {
            if let Some(t) = self.pick_tactical(today) {
                return t;
            }
        }
        self.take("Individual Development")
    }
}

fn deficit(score: f32) -> f32 {
    (100.0 - score).max(3.0).powf(1.3)
}

/// Rol van elke dag; `prev_sunday_match` = wedstrijd op de zondag ervoor,
/// `next_monday_match` = wedstrijd op de maandag erna.
fn day_roles(week: &CalendarWeek, prev_sunday_match: bool, next_monday_match: bool) -> [DayRole; 7] {
    let m = |i: i32| -> bool {
        match i {
            -1 => prev_sunday_match,
            7 => next_monday_match,
            0..=6 => week.matches[i as usize],
            _ => false,
        }
    };
    let mut roles = [DayRole::Training; 7];
    for i in 0..7i32 {
        roles[i as usize] = if m(i) {
            DayRole::Match
        } else if m(i - 1) && m(i + 1) {
            DayRole::PostAndPreMatch
        } else if m(i - 1) {
            DayRole::PostMatch
        } else if m(i + 1) {
            DayRole::PreMatch
        } else {
            DayRole::Training
        };
    }
    if matches!(week.kind, WeekKind::OffSeason) {
        for r in roles.iter_mut() {
            if *r == DayRole::Training {
                *r = DayRole::Holiday;
            }
        }
    }
    // Zonder wedstrijden: één hersteldag (zondag), net als de SI-presets.
    if week.match_count() == 0 && roles[6] == DayRole::Training {
        roles[6] = DayRole::Recovery;
    }
    roles
}

pub fn plan_week(
    week: &CalendarWeek,
    prev_sunday_match: bool,
    next_monday_match: bool,
    eff: &Effectiveness,
    settings: &PlannerSettings,
    season_week_no: u32,
) -> WeekPlan {
    let roles = day_roles(week, prev_sunday_match, next_monday_match);
    let post_match_days = roles.iter().filter(|r| **r == DayRole::PostMatch).count() as u32;
    let mut st = WeekState {
        bonding_reserved: post_match_days.min(2),
        deficit: Factor::TACTICAL_AREAS.iter().map(|f| (*f, deficit(eff.get(*f)))).collect(),
        count: HashMap::new(),
        eff,
        settings,
    };
    let matches = week.match_count();
    let set_piece_week = settings.set_pieces_every > 0 && season_week_no % settings.set_pieces_every == 0;
    // Light Conditioning: één keer in een week met ≤1 wedstrijd (SI doet dat op de 3e dag)
    let mut conditioning_left: u32 = match (week.kind, matches) {
        (k, _) if k.is_preseason() => 0, // voorbereiding regelt het per dag hieronder
        (WeekKind::OffSeason, _) => 0,
        (_, 0 | 1) => 1,
        _ => 0,
    };
    let training_days: Vec<usize> = (0..7).filter(|i| roles[*i] == DayRole::Training).collect();
    let lc_day = if training_days.len() >= 3 { training_days.get(2).copied() } else { training_days.last().copied() };

    let mut days = Vec::new();
    for (i, role) in roles.iter().enumerate() {
        let date = week.start + Duration::days(i as i64);
        let plan: [Slot; 3] = match role {
            DayRole::Match => [Slot::Fixed("Rest"), Slot::Fixed("Match"), Slot::Fixed("Rest")],
            DayRole::PostMatch => [Slot::Fixed("Recovery"), Slot::Fixed("Rest"), Slot::Extra],
            DayRole::PostAndPreMatch => [Slot::Fixed("Recovery"), Slot::MatchPrep, Slot::Fixed("Rest")],
            DayRole::PreMatch => [Slot::MatchPrep, Slot::MatchPrep, Slot::MatchPrep],
            DayRole::Recovery => [Slot::Fixed("Recovery"), Slot::Fixed("Rest"), Slot::Fixed("Rest")],
            DayRole::Holiday => [Slot::Fixed("Rest"), Slot::Fixed("Rest"), Slot::Fixed("Rest")],
            DayRole::Training => match week.kind {
                WeekKind::PreSeasonEarly => [Slot::Fixed(if i % 2 == 0 { "Heavy Conditioning" } else { "Conditioning" }), Slot::Tactical, Slot::Tactical],
                WeekKind::PreSeasonMid => [Slot::Fixed(if i % 3 == 2 { "Heavy Conditioning" } else { "Conditioning" }), Slot::Tactical, Slot::Tactical],
                WeekKind::PreSeasonLate => [Slot::Fixed(if i % 2 == 0 { "Light Conditioning" } else { "Conditioning" }), Slot::Tactical, Slot::Tactical],
                _ if conditioning_left > 0 && Some(i) == lc_day => {
                    conditioning_left -= 1;
                    [Slot::Fixed("Light Conditioning"), Slot::Tactical, Slot::Tactical]
                }
                _ => [Slot::Tactical, Slot::Tactical, Slot::Extra],
            },
        };
        let mut today: Vec<String> = Vec::new();
        for slot in plan {
            let name = match slot {
                Slot::Fixed(n) => st.take(n),
                Slot::Tactical => st.pick_tactical(&today).unwrap_or_else(|| st.take("Individual Development")),
                Slot::MatchPrep => st.pick_match_prep(&today, set_piece_week),
                Slot::Extra => {
                    if *role == DayRole::PostMatch {
                        st.bonding_reserved = st.bonding_reserved.saturating_sub(1);
                        if eff.buy_in() < settings.bonding_below && st.used("Team Bonding") < 2 {
                            st.take("Team Bonding")
                        } else {
                            st.take("Rest")
                        }
                    } else {
                        st.pick_extra(&today)
                    }
                }
            };
            today.push(name);
        }
        let slots: [String; 3] = [today[0].clone(), today[1].clone(), today[2].clone()];
        days.push(DayPlan { date, role: *role, slots });
    }

    let targets: Vec<(Factor, u32)> = Factor::TACTICAL_AREAS
        .iter()
        .map(|f| (*f, st.used(f.label())))
        .filter(|(_, n)| *n > 0)
        .collect();
    let ip: u32 = targets.iter().filter(|(f, _)| f.phase() == Some(Phase::InPossession)).map(|(_, n)| n).sum();
    let oop: u32 = targets.iter().filter(|(f, _)| f.phase() == Some(Phase::OutOfPossession)).map(|(_, n)| n).sum();
    let focus = if matches!(week.kind, WeekKind::OffSeason) {
        "Full Week of Rest"
    } else if ip as f32 > oop as f32 * 1.4 {
        "In Possession"
    } else if oop as f32 > ip as f32 * 1.4 {
        "Out Of Possession"
    } else {
        "Balanced"
    };

    let mut notes = Vec::new();
    for (f, v) in eff.weakest().into_iter().filter(|(f, _)| !f.is_tactical_area()).take(4) {
        if v < 50.0 {
            notes.push(format!("{} ({:.0}): {}", f.label(), v, f.advice()));
        }
    }

    let mut top: Vec<(Factor, u32)> = targets.clone();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    let abbrev: Vec<&str> = top.iter().take(2).map(|(f, _)| short(*f)).collect();
    let schedule_name = format!("TE {}W {} {}", matches, focus_short(focus), abbrev.join("+"));

    WeekPlan {
        start: week.start,
        kind: week.kind,
        matches,
        days,
        focus,
        targets,
        notes,
        projected: Vec::new(),
        schedule_name,
    }
}

fn focus_short(f: &str) -> &'static str {
    match f {
        "In Possession" => "IP",
        "Out Of Possession" => "OOP",
        "Full Week of Rest" => "Rest",
        _ => "Bal",
    }
}

pub fn short(f: Factor) -> &'static str {
    match f {
        Factor::InPossessionShape => "IPS",
        Factor::PassingStyle => "PS",
        Factor::BuildUp => "BU",
        Factor::ChanceCreation => "CC",
        Factor::OutOfPossessionShape => "OPS",
        Factor::LineOfEngagement => "LoE",
        Factor::PressingIntensity => "PI",
        Factor::PressingTrap => "PT",
        _ => "?",
    }
}

/// Jaarplan: plant week na week en schat hoe de Tactical Areas zich ontwikkelen, zodat de
/// training mee verschuift naar wat dan het zwakst is. De schatting is grof: lees de
/// Breakdown elke paar weken opnieuw uit en genereer het plan dan opnieuw.
pub fn plan_year(calendar: &[CalendarWeek], start: &Effectiveness, settings: &PlannerSettings) -> Vec<WeekPlan> {
    let mut eff = start.clone();
    let mut plan_basis = start.clone();
    let mut out = Vec::new();
    let mut season_week = 0u32;
    let block = settings.replan_every_weeks.max(1) as usize;
    for (i, week) in calendar.iter().enumerate() {
        if i % block == 0 {
            plan_basis = eff.clone(); // doelen per blok vastzetten → schema's herhalen zich
        }
        let prev_sun = i > 0 && calendar[i - 1].matches[6];
        let next_mon = calendar.get(i + 1).is_some_and(|w| w.matches[0]);
        if week.kind == WeekKind::Season {
            season_week += 1;
        }
        let mut plan = plan_week(week, prev_sun, next_mon, &plan_basis, settings, season_week);

        // projectie: winst per sessie (afnemend naar 100), verval als niet getraind,
        // en een beetje winst door wedstrijden met de tactiek.
        for f in Factor::TACTICAL_AREAS {
            let n = plan.targets.iter().find(|(x, _)| *x == f).map(|(_, n)| *n).unwrap_or(0);
            let mut v = eff.get(f);
            for _ in 0..n {
                v += settings.gain_per_session * (1.0 - v / 105.0).max(0.0);
            }
            if n == 0 {
                v -= settings.decay_per_week;
            }
            v += 0.3 * plan.matches as f32;
            if week.kind == WeekKind::OffSeason {
                v -= settings.decay_per_week; // vakantie: extra verval
            }
            eff.set(f, v);
        }
        plan.projected = Factor::TACTICAL_AREAS.iter().map(|f| (*f, eff.get(*f))).collect();
        out.push(plan);
    }
    out
}

/// Unieke weekschema's in het plan (om eenmalig in FM aan te maken en op te slaan).
pub fn unique_schedules(plans: &[WeekPlan]) -> Vec<(String, Vec<NaiveDate>, &WeekPlan)> {
    let mut seen: Vec<(Vec<[String; 3]>, String, Vec<NaiveDate>, usize)> = Vec::new();
    for (i, p) in plans.iter().enumerate() {
        let key: Vec<[String; 3]> = p.days.iter().map(|d| d.slots.clone()).collect();
        if let Some(e) = seen.iter_mut().find(|e| e.0 == key) {
            e.2.push(p.start);
        } else {
            seen.push((key, p.schedule_name.clone(), vec![p.start], i));
        }
    }
    seen.into_iter().map(|(_, name, dates, i)| (name, dates, &plans[i])).collect()
}
