//! Training automatisch in FM zetten: de kalender van FM lezen (via de module), elke komende
//! week plannen met de echte wedstrijddagen en de huidige Tactical Effectiveness, en de
//! sessies terugschrijven. Alleen dagen ná de spel-datum; wedstrijd- en reisdagen blijven van FM.

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate};
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::agent;
use crate::model::{self, Effectiveness};
use crate::planner::{self, CalendarWeek, PlannerSettings, WeekKind};

#[derive(Clone, Debug)]
pub struct WeekPreview {
    pub start: NaiveDate,
    /// FM's naam voor het schema van deze week ("Tactical Focus - Balanced", "Full Week of Rest", …).
    pub name: String,
    /// Rustweek (vakantie): die laat de app altijd aan FM.
    pub rest: bool,
    pub matches: usize,
    pub focus: String,
    pub current: Vec<[u16; 3]>,
    pub planned: Vec<[u16; 3]>,
    pub editable: [bool; 7],
    pub written: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub today: Option<NaiveDate>,
    pub weeks: Vec<WeekPreview>,
    pub written_days: usize,
}

fn fm_date(v: &Value) -> Option<NaiveDate> {
    NaiveDate::from_yo_opt(v.get("year")?.as_i64()? as i32, v.get("doy")?.as_u64()? as u32)
}

fn codes(v: &Value) -> Vec<[u16; 3]> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|d| {
                    let x = |i: usize| d.get(i).and_then(|c| c.as_u64()).unwrap_or(0) as u16;
                    [x(0), x(1), x(2)]
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Naam van het schema van een week: het record begint met een pointer naar een FM-string.
/// Gelezen met de algemene leesopdracht, zodat dit ook met oudere modules werkt.
fn week_name(addr: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();
    let r = agent::request(json!({"cmd": "read", "addr": addr, "len": 8})).ok()?;
    let b = hex_bytes(r.get("hex")?.as_str()?)?;
    let ptr = u64::from_le_bytes(b.get(..8)?.try_into().ok()?);
    if ptr == 0 {
        return None;
    }
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(n) = cache.lock().ok()?.get(&ptr) {
        return Some(n.clone());
    }
    let r = agent::request(json!({"cmd": "read", "addr": format!("{ptr:#x}"), "len": 72})).ok()?;
    let b = hex_bytes(r.get("hex")?.as_str()?)?;
    let len = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as usize;
    let name = String::from_utf8(b.get(4..4 + len)?.to_vec()).ok()?;
    cache.lock().ok()?.insert(ptr, name.clone());
    Some(name)
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn is_match_or_travel(d: &[u16; 3]) -> bool {
    d.iter().any(|c| *c == 500 || *c == 402)
}

fn kind_for(start: NaiveDate) -> WeekKind {
    let mid = start + Duration::days(3);
    match (mid.month(), mid.day()) {
        (6, 20..) => WeekKind::PreSeasonEarly,
        (7, 1..=10) => WeekKind::PreSeasonEarly,
        (7, 11..=21) => WeekKind::PreSeasonMid,
        (7, _) => WeekKind::PreSeasonLate,
        _ => WeekKind::Season,
    }
}

/// Plant de komende weken; schrijft ze naar FM als `write`. Met `skip` worden weken die
/// al eerder ingepland zijn overgeslagen (automatische modus).
///
/// Elke week wordt gepland met een geschatte stand van de Tactical Areas: na elke week telt de app de
/// geplande sessies op (en een beetje verval voor wat niet getraind is), zodat de focus over de weken
/// meeschuift in plaats van elke week dezelfde drie gebieden te trainen. Rustweken ("Full Week of
/// Rest") blijven van FM.
pub fn run(eff: &Effectiveness, settings: &PlannerSettings, write: bool, skip: &HashSet<NaiveDate>) -> Result<Outcome> {
    let data = agent::request(json!({"cmd": "training_weeks"})).context("training calendar")?;
    let today = fm_date(&data["today"]).context("game date")?;
    let weeks: Vec<(NaiveDate, Vec<[u16; 3]>, String)> = data["weeks"]
        .as_array()
        .context("weeks")?
        .iter()
        .filter_map(|w| Some((fm_date(&w["start"])?, codes(&w["days"]), w["addr"].as_str().unwrap_or("").to_string())))
        .filter(|(_, d, _)| d.len() == 7)
        .collect();

    let mut out = Outcome { today: Some(today), ..Default::default() };
    let mut proj = eff.clone();
    for (i, (start, cur, addr)) in weeks.iter().enumerate() {
        if *start + Duration::days(6) <= today {
            continue; // week is voorbij
        }
        let name = week_name(addr).unwrap_or_default();
        let rest = name.contains("Rest");
        if rest {
            out.weeks.push(WeekPreview {
                start: *start,
                name: name.clone(),
                rest: true,
                matches: 0,
                focus: "Full Week of Rest".into(),
                current: cur.clone(),
                planned: cur.clone(),
                editable: [false; 7],
                written: 0,
            });
            continue;
        }
        let mut matches = [false; 7];
        for d in 0..7 {
            matches[d] = cur[d].contains(&500);
        }
        let prev_sun = i > 0 && weeks[i - 1].1[6].contains(&500);
        let next_mon = weeks.get(i + 1).is_some_and(|w| w.1[0].contains(&500));
        let cw = CalendarWeek { start: *start, kind: kind_for(*start), matches, note: String::new() };
        let plan = planner::plan_week(&cw, prev_sun, next_mon, &proj, settings, i as u32 + 1);
        // geschatte vooruitgang: de volgende week mikt op wat dan het zwakst is
        for f in crate::model::Factor::TACTICAL_AREAS {
            let n = plan.targets.iter().find(|(x, _)| *x == f).map(|(_, n)| *n).unwrap_or(0);
            let mut v = proj.get(f);
            for _ in 0..n {
                v += settings.gain_per_session * (1.0 - v / 105.0).max(0.0);
            }
            if n == 0 {
                v -= settings.decay_per_week;
            }
            proj.set(f, v);
        }

        let mut planned = cur.clone();
        let mut editable = [false; 7];
        for d in 0..7 {
            let date = *start + Duration::days(d as i64);
            // een code die de app niet kent (leeg vak, nieuw soort dag) laat de module ook ongemoeid
            editable[d] = date > today && !is_match_or_travel(&cur[d]) && cur[d].iter().all(|c| *c <= 600);
            if !editable[d] {
                continue;
            }
            for s in 0..3 {
                if let Some(code) = model::session_code(&plan.days[d].slots[s]) {
                    if code != 500 && code != 402 {
                        planned[d][s] = code;
                    }
                }
            }
        }
        let mut written = 0;
        if write && !skip.contains(start) && planned != *cur {
            let req = json!({
                "cmd": "set_week",
                "start": {"year": start.year(), "doy": start.ordinal()},
                "days": planned,
            });
            let r = agent::request(req).with_context(|| format!("week van {start} schrijven"))?;
            written = r.get("days").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            out.written_days += written;
        }
        out.weeks.push(WeekPreview {
            start: *start,
            name,
            rest: false,
            matches: matches.iter().filter(|m| **m).count(),
            focus: plan.focus.to_string(),
            current: cur.clone(),
            planned,
            editable,
            written,
        });
    }
    Ok(out)
}

/// Korte naam voor in het overzicht.
pub fn short(code: u16) -> &'static str {
    match code {
        0 => "PS",
        1 => "BU",
        2 => "CC",
        3 => "IPS",
        4 => "LoE",
        5 => "PI",
        6 => "PT",
        7 => "OPS",
        100 => "PrAv",
        101 => "BallP",
        102 => "F3rd",
        103 => "DBlk",
        104 => "DPos",
        105 => "DEng",
        106 => "Cor",
        107 => "FK",
        108 => "Pen",
        200 => "ID",
        201 => "ABW",
        202 => "DBW",
        203 => "PBW",
        300 => "Rest",
        301 => "Rec",
        302 => "LCon",
        303 => "Con",
        304 => "HCon",
        400 => "Bond",
        401 => "Comm",
        402 => "Reis",
        500 => "MATCH",
        _ => "·",
    }
}

pub fn long(code: u16) -> String {
    model::session_by_code(code).map(str::to_string).unwrap_or_else(|| format!("code {code}"))
}
