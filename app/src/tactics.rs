//! Tactiekhulp: past de tactiek uit FM op de selectie. Per plek hoe goed de speler bij de rol en
//! de positie past, wie beter zou passen, en de beste elf voor de tactiek.
//!
//! De rollen en de plekken komen uit FM's geheugen (opdracht `tactics` van de module). Welke
//! attributen bij een rol horen, staat niet in FM's data: de lijsten hieronder zijn onze eigen
//! inschatting, en de score is dus een hulpmiddel, geen cijfer van FM. De wedstrijdengine beslist.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug)]
pub struct Slot {
    pub uid: Option<u32>,
    pub ip: Option<u8>,
    pub oop: Option<u8>,
    pub oop_order: u8,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Tactic {
    pub index: usize,
    pub style: String,
    pub slots: Vec<Slot>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Player {
    pub uid: u32,
    pub name: String,
    pub positions: Vec<u8>,
    pub attributes: HashMap<String, u8>,
    #[serde(default)]
    pub left_foot: u8,
    #[serde(default)]
    pub right_foot: u8,
    #[serde(default)]
    pub injured: bool,
    #[serde(default)]
    pub banned: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Data {
    pub current: usize,
    pub tactics: Vec<Tactic>,
    pub players: Vec<Player>,
    #[serde(default)]
    pub bench: Vec<u32>,
}

// ---- posities ---------------------------------------------------------------------------------

/// Index in `Player.positions`.
const GK: usize = 0;
const DL: usize = 2;
const DC: usize = 3;
const DR: usize = 4;
const DM: usize = 5;
const ML: usize = 6;
const MC: usize = 7;
const MR: usize = 8;
const AML: usize = 9;
const AMC: usize = 10;
const AMR: usize = 11;
const ST: usize = 12;
const WBL: usize = 13;
const WBR: usize = 14;

/// De linie van een plek.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    Keeper,
    CentreBack,
    /// back of wingback
    WideBack,
    DefMid,
    /// verdedigende of centrale middenvelder (spelmaker die op beide kan staan)
    DefOrCentreMid,
    CentreMid,
    /// centrale of aanvallende middenvelder
    CentreOrAttMid,
    AttMid,
    /// buitenspeler of vleugelmiddenvelder
    Wide,
    Striker,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Right,
    Left,
    Centre,
}

impl Line {
    fn wide(self) -> bool {
        matches!(self, Line::WideBack | Line::Wide)
    }

    /// De positiecijfers die voor deze linie tellen.
    fn positions(self, side: Side) -> &'static [usize] {
        match (self, side) {
            (Line::Keeper, _) => &[GK],
            (Line::CentreBack, _) => &[DC],
            (Line::WideBack, Side::Left) => &[DL, WBL],
            (Line::WideBack, Side::Right) => &[DR, WBR],
            (Line::WideBack, Side::Centre) => &[DL, DR, WBL, WBR],
            (Line::DefMid, _) => &[DM],
            (Line::DefOrCentreMid, _) => &[DM, MC],
            (Line::CentreMid, _) => &[MC],
            (Line::CentreOrAttMid, _) => &[MC, AMC],
            (Line::AttMid, _) => &[AMC],
            (Line::Wide, Side::Left) => &[AML, ML],
            (Line::Wide, Side::Right) => &[AMR, MR],
            (Line::Wide, Side::Centre) => &[AML, AMR, ML, MR],
            (Line::Striker, _) => &[ST],
        }
    }

    pub fn label(self, side: Side) -> &'static str {
        match (self, side) {
            (Line::Keeper, _) => "GK",
            (Line::CentreBack, _) => "D (C)",
            (Line::WideBack, Side::Left) => "D/WB (L)",
            (Line::WideBack, Side::Right) => "D/WB (R)",
            (Line::WideBack, Side::Centre) => "D/WB",
            (Line::DefMid, _) => "DM",
            (Line::DefOrCentreMid, _) => "DM/M (C)",
            (Line::CentreMid, _) => "M (C)",
            (Line::CentreOrAttMid, _) => "M/AM (C)",
            (Line::AttMid, _) => "AM (C)",
            (Line::Wide, Side::Left) => "AM/M (L)",
            (Line::Wide, Side::Right) => "AM/M (R)",
            (Line::Wide, Side::Centre) => "AM/M (RL)",
            (Line::Striker, _) => "ST",
        }
    }
}

// ---- rollen -----------------------------------------------------------------------------------

pub struct Role {
    pub bit: u8,
    pub code: &'static str,
    pub name: &'static str,
    pub line: Line,
    /// de attributen die voor de rol tellen (onze inschatting)
    pub attrs: &'static [&'static str],
}

macro_rules! role {
    ($bit:expr, $code:expr, $name:expr, $line:ident, [$($a:ident),*]) => {
        Role { bit: $bit, code: $code, name: $name, line: Line::$line, attrs: &[$(stringify!($a)),*] }
    };
}

/// Rollen met bal. Het bitnummer is dat van FM's rolmasker; de namen bij de bits zijn in FM's
/// tactiekscherm gezien (FMLibreEditor, en 15/40/57 op de Ajax-save van 14 nov 2026).
pub const IP_ROLES: &[Role] = &[
    role!(1, "CB", "Centre-Back", CentreBack, [heading, marking, tackling, positioning, jumping_reach, strength, anticipation, concentration, bravery, pace]),
    role!(2, "FB", "Full-Back", WideBack, [tackling, marking, positioning, crossing, stamina, pace, work_rate, anticipation, teamwork, concentration]),
    role!(3, "WB", "Wing-Back", WideBack, [crossing, dribbling, stamina, pace, acceleration, work_rate, off_the_ball, tackling, teamwork, technique]),
    role!(4, "DM", "Defensive Midfielder", DefMid, [tackling, positioning, anticipation, concentration, marking, teamwork, work_rate, stamina, passing, decisions, strength]),
    role!(7, "W", "Winger", Wide, [crossing, dribbling, pace, acceleration, technique, agility, off_the_ball, flair, stamina]),
    role!(9, "AM", "Attacking Midfielder", AttMid, [first_touch, passing, technique, off_the_ball, vision, decisions, composure, long_shots, flair, anticipation, dribbling]),
    role!(10, "DLF", "Deep-Lying Forward", Striker, [first_touch, passing, technique, composure, decisions, off_the_ball, teamwork, vision, finishing, strength]),
    role!(11, "CFD", "Centre Forward", Striker, [finishing, off_the_ball, first_touch, composure, anticipation, heading, strength, pace, acceleration, technique, decisions]),
    role!(12, "BGK", "Ball-Playing Goalkeeper", Keeper, [handling, reflexes, one_on_ones, aerial_reach, command_of_area, positioning, concentration, passing, first_touch, composure, kicking, throwing, vision]),
    role!(14, "ACB", "Advanced Centre-Back", CentreBack, [passing, first_touch, composure, decisions, positioning, tackling, anticipation, dribbling, marking, heading]),
    role!(15, "DLP", "Deep-Lying Playmaker", DefOrCentreMid, [passing, vision, first_touch, composure, decisions, technique, teamwork, anticipation, positioning]),
    role!(16, "BBM", "Box-to-Box Midfielder", CentreMid, [stamina, work_rate, passing, tackling, off_the_ball, teamwork, long_shots, strength, decisions, pace]),
    role!(17, "AP", "Advanced Playmaker", CentreOrAttMid, [passing, vision, first_touch, technique, composure, decisions, flair, off_the_ball, teamwork]),
    role!(19, "P", "Poacher", Striker, [finishing, off_the_ball, anticipation, composure, acceleration, first_touch, pace]),
    role!(24, "BCB", "Ball-Playing Centre-Back", CentreBack, [heading, marking, tackling, positioning, jumping_reach, strength, anticipation, passing, first_touch, composure, vision, technique]),
    role!(27, "IF", "Inside Forward", Wide, [dribbling, finishing, first_touch, technique, off_the_ball, acceleration, pace, composure, anticipation, long_shots, agility]),
    role!(29, "NCB", "No-Nonsense Centre-Back", CentreBack, [heading, marking, tackling, positioning, strength, jumping_reach, bravery, aggression]),
    role!(35, "HB", "Half-Back", DefMid, [positioning, tackling, marking, anticipation, concentration, composure, passing, decisions, strength, heading]),
    role!(38, "AWB", "Advanced Wing-Back", WideBack, [crossing, dribbling, pace, acceleration, stamina, off_the_ball, flair, technique, work_rate]),
    role!(40, "F9", "False Nine", Striker, [first_touch, passing, technique, vision, dribbling, composure, decisions, off_the_ball, finishing, flair, agility]),
    role!(43, "PW", "Playmaking Winger", Wide, [passing, vision, first_touch, technique, dribbling, decisions, composure, flair, off_the_ball]),
    role!(44, "IWB", "Inside Wing-Back", WideBack, [passing, first_touch, decisions, composure, tackling, positioning, teamwork, anticipation, technique]),
    role!(45, "WFD", "Wide Forward", Wide, [finishing, off_the_ball, pace, acceleration, dribbling, first_touch, composure, anticipation, heading]),
    role!(46, "BBP", "Box-to-Box Playmaker", CentreMid, [stamina, work_rate, passing, vision, technique, first_touch, composure, off_the_ball, teamwork, decisions]),
    role!(48, "WCM", "Wide Central Midfielder", CentreMid, [passing, crossing, stamina, work_rate, off_the_ball, teamwork, first_touch, decisions]),
    role!(52, "IFB", "Inside Full-Back", WideBack, [tackling, marking, positioning, heading, strength, concentration, passing, anticipation]),
    role!(53, "NGK", "No-Nonsense Goalkeeper", Keeper, [handling, reflexes, aerial_reach, one_on_ones, concentration, positioning, command_of_area, communication]),
    role!(54, "OCB", "Overlapping Centre-Back", CentreBack, [pace, stamina, crossing, dribbling, tackling, marking, work_rate, positioning, heading]),
    role!(55, "PWB", "Playmaking Wing-Back", WideBack, [passing, vision, technique, crossing, first_touch, decisions, stamina, composure, teamwork]),
    role!(56, "CHM", "Channel Midfielder", CentreMid, [off_the_ball, stamina, work_rate, passing, crossing, acceleration, dribbling, teamwork]),
    role!(57, "MPM", "Midfield Playmaker", CentreMid, [passing, vision, first_touch, technique, composure, decisions, teamwork, anticipation, off_the_ball]),
];

/// Rollen zonder bal. FM nummert ze in veldvolgorde (keepers, centrale verdedigers, backs,
/// wingbacks, verdedigende middenvelders, middenvelders, aanvallende middenvelders,
/// vleugelmiddenvelders, buitenspelers, spitsen): een bit zonder naam krijgt zo toch zijn linie.
pub const OOP_ROLES: &[Role] = &[
    role!(1, "SK", "Sweeper Keeper", Keeper, [rushing_out, one_on_ones, anticipation, acceleration, command_of_area, reflexes, handling, decisions, concentration]),
    role!(2, "LHK", "Line-Holding Keeper", Keeper, [handling, reflexes, positioning, concentration, aerial_reach, command_of_area, one_on_ones]),
    role!(3, "CB", "Centre-Back", CentreBack, [marking, tackling, positioning, heading, jumping_reach, strength, concentration, anticipation]),
    role!(4, "SCB", "Stopping Centre-Back", CentreBack, [aggression, bravery, tackling, anticipation, strength, decisions, marking, heading]),
    role!(5, "CCB", "Covering Centre-Back", CentreBack, [pace, acceleration, positioning, anticipation, concentration, marking, decisions, tackling]),
    role!(6, "WCB", "Wide Centre-Back", CentreBack, [tackling, marking, pace, positioning, stamina, anticipation]),
    role!(9, "FB", "Full-Back", WideBack, [tackling, marking, positioning, concentration, anticipation, pace, teamwork]),
    role!(10, "PFB", "Pressing Full-Back", WideBack, [aggression, work_rate, stamina, tackling, acceleration, anticipation, pace, bravery]),
    role!(11, "HFB", "Holding Full-Back", WideBack, [positioning, concentration, marking, tackling, anticipation, decisions]),
    role!(12, "WB", "Wing-Back", WideBack, [tackling, marking, stamina, work_rate, pace, positioning]),
    role!(14, "HWB", "Holding Wing-Back", WideBack, [positioning, concentration, marking, tackling, anticipation, stamina]),
    role!(15, "DM", "Defensive Midfielder", DefMid, [positioning, tackling, anticipation, concentration, marking, teamwork, strength]),
    role!(16, "DDM", "Dropping Defensive Midfielder", DefMid, [positioning, marking, heading, concentration, anticipation, tackling, strength, decisions]),
    role!(17, "PDM", "Pressing Defensive Midfielder", DefMid, [aggression, work_rate, stamina, tackling, anticipation, acceleration, bravery, teamwork]),
    role!(20, "CM", "Central Midfielder", CentreMid, [tackling, positioning, work_rate, teamwork, stamina, anticipation, concentration]),
    role!(27, "PCM", "Pressing Central Midfielder", CentreMid, [work_rate, stamina, aggression, tackling, anticipation, acceleration, teamwork, bravery]),
    role!(28, "SCM", "Screening Central Midfielder", CentreMid, [positioning, anticipation, concentration, tackling, marking, teamwork, decisions]),
    role!(32, "OAM", "Outlet Attacking Midfielder", AttMid, [off_the_ball, anticipation, acceleration, pace, composure, first_touch]),
    role!(38, "W", "Winger", Wide, [work_rate, stamina, teamwork, tackling, positioning, pace, anticipation]),
    role!(39, "TW", "Tracking Winger", Wide, [work_rate, stamina, teamwork, tackling, marking, positioning, concentration]),
    role!(40, "IOW", "Inside Outlet Winger", Wide, [off_the_ball, anticipation, acceleration, pace, composure]),
    role!(42, "CFD", "Centre Forward", Striker, [work_rate, teamwork, anticipation, stamina, aggression, strength, pace]),
    role!(43, "TCF", "Tracking Centre Forward", Striker, [work_rate, stamina, teamwork, tackling, aggression, anticipation]),
    role!(44, "OCF", "Outlet Centre Forward", Striker, [off_the_ball, anticipation, pace, acceleration, strength, composure]),
];

pub fn ip_role(bit: u8) -> Option<&'static Role> {
    IP_ROLES.iter().find(|r| r.bit == bit)
}

pub fn oop_role(bit: u8) -> Option<&'static Role> {
    OOP_ROLES.iter().find(|r| r.bit == bit)
}

/// De linie van een rol zonder bal, ook als we de naam niet kennen: uit FM's nummering.
pub fn oop_line(bit: u8) -> Line {
    if let Some(r) = oop_role(bit) {
        return r.line;
    }
    match bit {
        0..=2 => Line::Keeper,
        3..=8 => Line::CentreBack,
        9..=14 => Line::WideBack,
        15..=19 => Line::DefMid,
        20..=28 => Line::CentreMid,
        29..=33 => Line::AttMid,
        34..=41 => Line::Wide,
        _ => Line::Striker,
    }
}

/// Wat bij een linie telt als we de rol niet kennen.
fn generic(line: Line) -> &'static [&'static str] {
    match line {
        Line::Keeper => &["handling", "reflexes", "one_on_ones", "aerial_reach", "command_of_area", "positioning", "concentration"],
        Line::CentreBack => &["heading", "marking", "tackling", "positioning", "strength", "anticipation", "concentration"],
        Line::WideBack => &["tackling", "marking", "positioning", "crossing", "stamina", "pace", "work_rate"],
        Line::DefMid | Line::DefOrCentreMid => &["tackling", "positioning", "anticipation", "passing", "teamwork", "work_rate", "decisions"],
        Line::CentreMid | Line::CentreOrAttMid => &["passing", "first_touch", "decisions", "teamwork", "work_rate", "stamina", "off_the_ball"],
        Line::AttMid => &["first_touch", "passing", "technique", "off_the_ball", "vision", "decisions", "composure"],
        Line::Wide => &["dribbling", "crossing", "pace", "acceleration", "technique", "off_the_ball", "work_rate"],
        Line::Striker => &["finishing", "off_the_ball", "first_touch", "composure", "anticipation", "pace", "strength"],
    }
}

pub fn ip_label(bit: Option<u8>) -> (String, String) {
    match bit {
        Some(b) => match ip_role(b) {
            Some(r) => crate::fmterms::role(r.code, r.name),
            None => (format!("#{b}"), crate::t!("rol die de app nog niet kent", "role the app does not know yet", "função que o app ainda não conhece").to_string()),
        },
        None => ("–".into(), String::new()),
    }
}

pub fn oop_label(bit: Option<u8>) -> (String, String) {
    match bit {
        Some(b) => match oop_role(b) {
            Some(r) => crate::fmterms::role(r.code, r.name),
            None => (format!("#{b}"), crate::t!("rol die de app nog niet kent", "role the app does not know yet", "função que o app ainda não conhece").to_string()),
        },
        None => ("–".into(), String::new()),
    }
}

// ---- plekken ----------------------------------------------------------------------------------

/// Een plek van de tactiek, uitgewerkt: linie, kant en de attributen die tellen.
#[derive(Clone, Debug)]
pub struct Place {
    pub line: Line,
    pub side: Side,
    pub ip: Option<u8>,
    pub oop: Option<u8>,
    pub uid: Option<u32>,
    pub oop_order: u8,
}

/// Linie en kant van elke plek. De linie komt van de rol met bal (FM noemt de plek naar de
/// opstelling met bal) en anders van de rol zonder bal. De kant staat niet in het tactiekblok:
/// FM zet de plekken zonder bal per linie van rechts naar links, dus van twee brede plekken in
/// dezelfde linie is de eerste rechts en de tweede links (zo gezien op de Ajax-save; afgeleid).
pub fn places(t: &Tactic) -> Vec<Place> {
    let mut out: Vec<Place> = t
        .slots
        .iter()
        .map(|s| {
            let line = s.ip.and_then(ip_role).map(|r| r.line).or(s.oop.map(oop_line)).unwrap_or(Line::CentreMid);
            Place { line, side: Side::Centre, ip: s.ip, oop: s.oop, uid: s.uid, oop_order: s.oop_order }
        })
        .collect();
    for wide in [Line::WideBack, Line::Wide] {
        let mut idx: Vec<usize> = (0..out.len()).filter(|&i| out[i].line == wide).collect();
        idx.sort_by_key(|&i| out[i].oop_order);
        // paren: rechts, links; een brede plek zonder tegenhanger houdt beide kanten open
        for pair in idx.chunks(2) {
            if let [r, l] = pair {
                out[*r].side = Side::Right;
                out[*l].side = Side::Left;
            }
        }
    }
    out
}

// ---- score ------------------------------------------------------------------------------------

fn mean(p: &Player, attrs: &[&str]) -> f32 {
    let v: Vec<f32> = attrs.iter().filter_map(|a| p.attributes.get(*a)).map(|&x| x as f32).collect();
    if v.is_empty() { 10.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
}

/// Het positiecijfer (1–20) van de speler voor deze plek: het beste van de posities die tellen.
pub fn position_rating(p: &Player, place: &Place) -> u8 {
    place.line.positions(place.side).iter().filter_map(|&i| p.positions.get(i)).copied().max().unwrap_or(1)
}

/// Hoe goed de attributen bij de rollen passen, 0–100 (gemiddelde van de attributen × 5; met bal
/// telt voor 60 %, zonder bal voor 40 %).
pub fn role_score(p: &Player, ip: Option<u8>, oop: Option<u8>, line: Line) -> f32 {
    let with_ball = mean(p, ip.and_then(ip_role).map(|r| r.attrs).unwrap_or(generic(line)));
    let without = mean(p, oop.and_then(oop_role).map(|r| r.attrs).unwrap_or(generic(oop.map(oop_line).unwrap_or(line))));
    (0.6 * with_ball + 0.4 * without) * 5.0
}

/// Geschiktheid voor een plek, 0–100: de rolscore, gedrukt als de speler de positie niet kent
/// (positiecijfer 20 telt volledig, 1 voor de helft).
pub fn fit(p: &Player, place: &Place) -> f32 {
    let pos = position_rating(p, place) as f32;
    role_score(p, place.ip, place.oop, place.line) * (0.5 + 0.5 * pos / 20.0)
}

/// FM's woorden voor een positiecijfer.
pub fn position_word(r: u8) -> &'static str {
    match r {
        18..=20 => crate::t!("natuurlijk", "natural", "natural"),
        15..=17 => crate::t!("volleerd", "accomplished", "experiente"),
        12..=14 => crate::t!("bekwaam", "competent", "competente"),
        9..=11 => crate::t!("onwennig", "unconvincing", "pouco convincente"),
        5..=8 => crate::t!("ongemakkelijk", "awkward", "desajeitado"),
        _ => crate::t!("kent hij niet", "ineffectual", "ineficaz"),
    }
}

/// De beste rol met bal voor deze speler op deze plek (zelfde linie), als die duidelijk beter
/// past dan de huidige: (rol, score nu, score dan).
pub fn better_role(p: &Player, place: &Place) -> Option<(&'static Role, f32, f32)> {
    let cur = place.ip.and_then(ip_role)?;
    let now = mean(p, cur.attrs) * 5.0;
    let same_line = |r: &&Role| {
        r.line == cur.line
            || matches!(
                (r.line, cur.line),
                (Line::DefOrCentreMid, Line::DefMid | Line::CentreMid) | (Line::DefMid | Line::CentreMid, Line::DefOrCentreMid) | (Line::CentreOrAttMid, Line::CentreMid | Line::AttMid) | (Line::CentreMid | Line::AttMid, Line::CentreOrAttMid)
            )
    };
    let best = IP_ROLES.iter().filter(same_line).filter(|r| r.bit != cur.bit).map(|r| (r, mean(p, r.attrs) * 5.0)).max_by(|a, b| a.1.total_cmp(&b.1))?;
    (best.1 >= now + 4.0).then_some((best.0, now, best.1))
}

/// De toewijzing van spelers aan plekken met de hoogste som (Hongaarse methode). `score[i][j]`:
/// plek i, speler j; er moeten minstens zoveel spelers als plekken zijn. Geeft per plek de speler.
pub fn assign(score: &[Vec<f32>]) -> Vec<usize> {
    let n = score.len();
    let m = score.first().map_or(0, Vec::len);
    if n == 0 || m < n {
        return Vec::new();
    }
    // minimaliseren van (max − score)
    let top = score.iter().flatten().fold(0.0f32, |a, &b| a.max(b));
    let cost = |i: usize, j: usize| (top - score[i][j]) as f64;
    let (mut u, mut v) = (vec![0.0f64; n + 1], vec![0.0f64; m + 1]);
    let (mut p, mut way) = (vec![0usize; m + 1], vec![0usize; m + 1]);
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minv = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let (mut delta, mut j1) = (f64::INFINITY, 0);
            for j in 1..=m {
                if !used[j] {
                    let cur = cost(i0 - 1, j - 1) - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = vec![0usize; n];
    for j in 1..=m {
        if p[j] > 0 {
            out[p[j] - 1] = j - 1;
        }
    }
    out
}

/// Zoveel moet een wissel opleveren voor hij in de beste elf komt: zonder dit stelt de toewijzing
/// ook wissels voor die de som niet of nauwelijks verhogen.
const KEEP_BONUS: f32 = 1.5;

/// De beste elf voor deze plekken uit de spelers die kunnen spelen: per plek de index in
/// `players`. Wie er nu staat, blijft staan tenzij een ander duidelijk beter past. Leeg als er
/// te weinig spelers zijn.
pub fn best_eleven(places: &[Place], players: &[Player]) -> Vec<usize> {
    let fit_idx: Vec<usize> = (0..players.len()).filter(|&j| !players[j].injured && !players[j].banned).collect();
    let score: Vec<Vec<f32>> = places
        .iter()
        .map(|pl| fit_idx.iter().map(|&j| fit(&players[j], pl) + if pl.uid == Some(players[j].uid) { KEEP_BONUS } else { 0.0 }).collect())
        .collect();
    assign(&score).into_iter().map(|j| fit_idx[j]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(uid: u32, pos: &[(usize, u8)], attrs: &[(&str, u8)]) -> Player {
        let mut positions = vec![1u8; 15];
        for &(i, r) in pos {
            positions[i] = r;
        }
        Player { uid, name: format!("P{uid}"), positions, attributes: attrs.iter().map(|&(k, v)| (k.to_string(), v)).collect(), left_foot: 10, right_foot: 20, injured: false, banned: false }
    }

    #[test]
    fn sides_follow_the_out_of_possession_order() {
        // de Ajax-tactiek van 14 nov 2026: met bal BGK CB CB WB WB DLP MPM MPM IF PW F9,
        // zonder bal in veldvolgorde SK PFB CCB CCB PFB DDM W PCM PCM W CFD
        let order = [0u8, 3, 2, 4, 1, 5, 7, 8, 6, 9, 10];
        let ip = [12u8, 1, 1, 3, 3, 15, 57, 57, 27, 43, 40];
        let oop = [1u8, 5, 5, 10, 10, 16, 27, 27, 38, 38, 42];
        let t = Tactic { index: 2, style: String::new(), slots: (0..11).map(|i| Slot { uid: Some(i as u32), ip: Some(ip[i]), oop: Some(oop[i]), oop_order: order[i] }).collect() };
        let p = places(&t);
        // Aït-Nouri (plek 3) is de linksback, El Hilali (plek 4) de rechtsback
        assert_eq!((p[3].line, p[3].side), (Line::WideBack, Side::Left));
        assert_eq!((p[4].line, p[4].side), (Line::WideBack, Side::Right));
        // Tsygankov (plek 8) rechts, Díaz (plek 9) links
        assert_eq!((p[8].line, p[8].side), (Line::Wide, Side::Right));
        assert_eq!((p[9].line, p[9].side), (Line::Wide, Side::Left));
        assert_eq!(p[5].line, Line::DefOrCentreMid);
        assert_eq!(p[10].line, Line::Striker);
    }

    #[test]
    fn position_counts_and_unknown_roles_fall_back() {
        let place = Place { line: Line::Striker, side: Side::Centre, ip: Some(11), oop: Some(42), uid: None, oop_order: 10 };
        let striker = player(1, &[(ST, 20)], &[("finishing", 18), ("off_the_ball", 16), ("work_rate", 12)]);
        let same_but_defender = player(2, &[(DC, 20)], &[("finishing", 18), ("off_the_ball", 16), ("work_rate", 12)]);
        assert!(fit(&striker, &place) > fit(&same_but_defender, &place) + 20.0);
        // een rol die we niet kennen: de linie van de rol zonder bal, zonder paniek
        assert_eq!(oop_line(35), Line::Wide);
        assert_eq!(oop_line(30), Line::AttMid);
        let unknown = Place { line: oop_line(35), side: Side::Centre, ip: Some(49), oop: Some(35), uid: None, oop_order: 8 };
        assert!(fit(&striker, &unknown) > 0.0);
        assert_eq!(ip_label(Some(49)).0, "#49");
    }

    #[test]
    fn assignment_finds_the_best_total() {
        // gretig kiezen (plek 0 pakt speler 0) geeft 10 + 1; het beste is 9 + 9
        let score = vec![vec![10.0, 9.0, 0.0], vec![9.0, 1.0, 0.0]];
        assert_eq!(assign(&score), vec![1, 0]);
        // te weinig spelers: geen antwoord
        assert!(assign(&[vec![1.0], vec![2.0]]).is_empty());
    }

    #[test]
    fn injured_players_are_left_out_of_the_best_eleven() {
        let places = vec![Place { line: Line::Striker, side: Side::Centre, ip: Some(11), oop: Some(42), uid: None, oop_order: 0 }];
        let mut star = player(1, &[(ST, 20)], &[("finishing", 20)]);
        star.injured = true;
        let backup = player(2, &[(ST, 15)], &[("finishing", 12)]);
        assert_eq!(best_eleven(&places, &[star, backup]), vec![1]);
    }

    #[test]
    fn a_swap_that_gains_nothing_is_not_proposed() {
        let mut place = Place { line: Line::Striker, side: Side::Centre, ip: Some(11), oop: Some(42), uid: Some(1), oop_order: 0 };
        let current = player(1, &[(ST, 20)], &[("finishing", 15)]);
        let equal = player(2, &[(ST, 20)], &[("finishing", 15)]);
        let better = player(3, &[(ST, 20)], &[("finishing", 20), ("off_the_ball", 20), ("first_touch", 20)]);
        // gelijke speler eerst in de lijst: de huidige blijft
        assert_eq!(best_eleven(&[place.clone()], &[equal.clone(), current.clone()]), vec![1]);
        // een duidelijk betere wint wel
        assert_eq!(best_eleven(&[place.clone()], &[equal.clone(), current.clone(), better]), vec![2]);
        place.uid = None;
        assert_eq!(best_eleven(&[place], &[equal, current]).len(), 1);
    }
}
