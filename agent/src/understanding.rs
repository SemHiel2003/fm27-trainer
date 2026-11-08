//! Tactisch begrip per speler ("Tactical Understanding", in FM27 de opvolger van tactical
//! familiarity), lezen en maximaliseren zoals FM's eigen editor-actie
//! `TEAM_MAXIMISE_TACTICAL_UNDERSTANDING_ACTION` dat doet.
//!
//! Gevonden door die actie in game_plugin.dll te ontleden (Windows, Steam-build 26340721,
//! 8 nov 2026). Elke speler wijst (Windows `+0x100`, naast het blessure-record) naar een record van
//! 0xE0 bytes:
//!
//! - `+0x00`–`+0xC0`: zes blokken van 0x20 bytes, één per gebied: een (begin, end, cap)-lijst van
//!   u32-items en op `+0x18` het gebiedsnummer 0–5 (Passing Style, Build Up, Chance Creation,
//!   In Possession Shape, Line of Engagement, Pressing Intensity). Een item: bits 0–13 het niveau
//!   (0–10000, 10000 = 100 %), bits 14–19 de instelling van de tactiek (0–20, 10 = gebalanceerd),
//!   de rest is rommel die FM niet leest. Hoogstens 20 items per lijst.
//! - `+0xC0`: een (begin, end, cap)-lijst van 16-byte-items voor Pressing Trap en Out Of Possession
//!   Shape: u16 rol/positie (bit 13 onderscheidt de twee gebieden), 11 bytes vorm van de opstelling,
//!   een u8 met vlaggen en op `+0x0E` het niveau (u16, 0–10000).
//! - `+0xD8` een datum, `+0xDC` een u8 die FM na een aanpassing op 1 zet.
//!
//! Het begrip van een speler voor een instelling is het hoogste item-niveau min 1000 per stap
//! afstand tot die instelling, met 1000 (10 %) als bodem. FM's actie telt bij elk item van de
//! tactieken van de manager 10000 op en begrenst op 10000; dit doet hetzelfde met de items die er
//! zijn, en voegt items toe voor de instellingen die de rest van de selectie kent zolang de lijst
//! ruimte heeft (FM's eigen allocator kan van buitenaf niet aangeroepen worden).

use serde::Serialize;
use std::collections::HashMap;

use crate::game::{self, Change, Patch, PlayerInfo};
use crate::mem;

const AREAS: usize = 6;
const AREA_STRIDE: u64 = 0x20;
const AREA_INDEX: u64 = 0x18;
const SHAPES: u64 = 0xC0;
const SHAPE_ITEM: u64 = 16;
const SHAPE_LEVEL: u64 = 0x0E;
const FLAG: u64 = 0xDC;
const RECORD_SIZE: usize = 0xE0;

pub const MAX_LEVEL: u32 = 10_000;
const BASE_LEVEL: u32 = 1_000;
const STEP_PENALTY: u32 = 1_000;
const LEVEL_MASK: u32 = 0x3FFF;
const KEY_SHIFT: u32 = 14;
const KEY_MASK: u32 = 0x3F;
pub const MAX_KEY: u8 = 20;
const MAX_ITEMS: u64 = 20;
const MAX_SHAPE_ITEMS: u64 = 64;

/// Namen van de acht gebieden, zoals FM ze noemt.
pub const AREA_NAMES: [&str; 8] = [
    "Passing Style",
    "Build Up",
    "Chance Creation",
    "In Possession Shape",
    "Line of Engagement",
    "Pressing Intensity",
    "Pressing Trap",
    "Out Of Possession Shape",
];

#[derive(Clone, Copy, Debug)]
struct List {
    header: u64,
    begin: u64,
    end: u64,
    cap: u64,
}

impl List {
    /// Een lijst met items van `item` bytes, hoogstens `max` stuks; een lege lijst mag nul zijn.
    fn read(header: u64, item: u64, max: u64) -> Option<List> {
        let b = mem::read_bytes(header, 24)?;
        let w = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        let (begin, end, cap) = (w(0), w(1), w(2));
        if begin == 0 && end == 0 {
            return Some(List { header, begin, end, cap: begin.max(cap) });
        }
        let sane = mem::is_ptr(begin & !0xF) && begin % 4 == 0 && begin <= end && end <= cap && (end - begin) % item == 0 && (cap - begin) % item == 0 && (cap - begin) / item <= max;
        sane.then_some(List { header, begin, end, cap })
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        if self.end == self.begin {
            return Some(Vec::new());
        }
        mem::read_bytes(self.begin, (self.end - self.begin) as usize)
    }
}

struct Record {
    addr: u64,
    areas: [List; AREAS],
    shapes: List,
}

fn record_at(addr: u64) -> Option<Record> {
    if !mem::is_ptr(addr) {
        return None;
    }
    let b = mem::read_bytes(addr, RECORD_SIZE)?;
    let mut areas = Vec::with_capacity(AREAS);
    for a in 0..AREAS {
        let o = a * AREA_STRIDE as usize;
        let idx = u64::from_le_bytes(b[o + AREA_INDEX as usize..o + AREA_INDEX as usize + 8].try_into().unwrap());
        if idx != a as u64 {
            return None;
        }
        areas.push(List::read(addr + o as u64, 4, MAX_ITEMS)?);
    }
    let shapes = List::read(addr + SHAPES, SHAPE_ITEM, MAX_SHAPE_ITEMS)?;
    Some(Record { addr, areas: areas.try_into().ok()?, shapes })
}

/// Het begrip-record van een speler: op de plek uit de indeling, anders één woord ervoor of erna
/// (de zes gebiedsnummers maken een vergissing vrijwel onmogelijk).
fn record(p: &PlayerInfo) -> Option<Record> {
    let base = p.addr + game::layout().p_understanding;
    [base, base + 8, base - 8].into_iter().find_map(|a| mem::u64_at(a).filter(|r| mem::is_ptr(*r)).and_then(record_at))
}

fn decode(item: u32) -> (u8, u32) {
    (((item >> KEY_SHIFT) & KEY_MASK) as u8, item & LEVEL_MASK)
}

fn encode(key: u8, level: u32) -> u32 {
    ((key as u32 & KEY_MASK) << KEY_SHIFT) | (level & LEVEL_MASK)
}

fn items(list: &List) -> Option<Vec<u32>> {
    Some(list.bytes()?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
}

/// Het begrip voor instelling `key`, zoals FM het afleidt uit de items.
pub fn level_for(items: &[u32], key: u8) -> u32 {
    let mut best = BASE_LEVEL;
    for &it in items {
        let (k, level) = decode(it);
        let dist = (k as i32 - key as i32).unsigned_abs();
        let v = level.saturating_sub(STEP_PENALTY.saturating_mul(dist));
        best = best.max(v);
    }
    best.min(MAX_LEVEL)
}

#[derive(Serialize, Clone)]
pub struct PlayerUnderstanding {
    pub uid: u32,
    pub name: String,
    /// per gebied 0–5 de items (instelling, niveau)
    pub areas: Vec<Vec<(u8, u32)>>,
    /// Pressing Trap en Out Of Possession Shape: de niveaus van de items per gebied
    pub shapes: [Vec<u32>; 2],
    /// het begrip voor de huidige tactiek (de instelling die de meeste spelers kennen), acht gebieden;
    /// voor de twee vormgebieden het hoogste item-niveau
    pub levels: [u32; 8],
}

#[derive(Serialize)]
pub struct Understanding {
    /// de instelling per gebied 0–5 die de meeste spelers kennen: de huidige tactiek
    pub keys: [u8; AREAS],
    pub players: Vec<PlayerUnderstanding>,
    /// zonder record (nog nooit getraind met dit team)
    pub without_record: usize,
    /// gemiddelde per gebied over de spelers met een record
    pub team: [u32; 8],
}

struct Loaded {
    player: PlayerInfo,
    rec: Record,
    areas: [Vec<u32>; AREAS],
    shapes: Vec<u8>,
}

fn load(players: Vec<PlayerInfo>) -> Result<(Vec<Loaded>, usize), String> {
    let mut out = Vec::new();
    let mut missing = 0;
    for p in players {
        let Some(rec) = record(&p) else {
            missing += 1;
            continue;
        };
        let mut areas: Vec<Vec<u32>> = Vec::with_capacity(AREAS);
        for l in &rec.areas {
            areas.push(items(l).ok_or("begrip-lijst niet leesbaar / understanding list not readable")?);
        }
        let shapes = rec.shapes.bytes().ok_or("begrip-lijst niet leesbaar / understanding list not readable")?;
        out.push(Loaded { player: p, rec, areas: areas.try_into().map_err(|_| "intern")?, shapes });
    }
    Ok((out, missing))
}

/// Per gebied de instelling die de meeste spelers kennen, met hoe vaak hij voorkomt.
fn common_keys(loaded: &[Loaded]) -> [Vec<(u8, usize)>; AREAS] {
    let mut out: Vec<Vec<(u8, usize)>> = Vec::with_capacity(AREAS);
    for a in 0..AREAS {
        let mut count: HashMap<u8, usize> = HashMap::new();
        for l in loaded {
            let mut seen = Vec::new();
            for &it in &l.areas[a] {
                let (k, _) = decode(it);
                if !seen.contains(&k) {
                    seen.push(k);
                    *count.entry(k).or_default() += 1;
                }
            }
        }
        let mut v: Vec<(u8, usize)> = count.into_iter().collect();
        v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
        out.push(v);
    }
    out.try_into().unwrap_or_else(|_| std::array::from_fn(|_| Vec::new()))
}

fn shape_levels(shapes: &[u8]) -> [Vec<u32>; 2] {
    let mut out: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for it in shapes.chunks_exact(SHAPE_ITEM as usize) {
        let setting = u16::from_le_bytes([it[0], it[1]]);
        let level = u16::from_le_bytes([it[SHAPE_LEVEL as usize], it[SHAPE_LEVEL as usize + 1]]) as u32;
        out[((setting >> 13) & 1) as usize].push(level.min(MAX_LEVEL));
    }
    out
}

fn describe(loaded: &[Loaded], keys: &[u8; AREAS]) -> Vec<PlayerUnderstanding> {
    loaded
        .iter()
        .map(|l| {
            let shapes = shape_levels(&l.shapes);
            let mut levels = [0u32; 8];
            for a in 0..AREAS {
                levels[a] = level_for(&l.areas[a], keys[a]);
            }
            for s in 0..2 {
                levels[AREAS + s] = shapes[s].iter().copied().max().unwrap_or(BASE_LEVEL);
            }
            PlayerUnderstanding {
                uid: l.player.uid,
                name: l.player.name.clone(),
                areas: l.areas.iter().map(|v| v.iter().map(|it| decode(*it)).collect()).collect(),
                shapes,
                levels,
            }
        })
        .collect()
}

fn team_average(players: &[PlayerUnderstanding]) -> [u32; 8] {
    let mut team = [0u32; 8];
    if players.is_empty() {
        return team;
    }
    for a in 0..8 {
        team[a] = (players.iter().map(|p| p.levels[a] as u64).sum::<u64>() / players.len() as u64) as u32;
    }
    team
}

/// Tactisch begrip van de selectie.
pub fn understanding() -> Result<Understanding, String> {
    let (loaded, without_record) = load(game::squad()?)?;
    let common = common_keys(&loaded);
    let keys: [u8; AREAS] = std::array::from_fn(|a| common[a].first().map(|(k, _)| *k).unwrap_or(10));
    let players = describe(&loaded, &keys);
    let team = team_average(&players);
    Ok(Understanding { keys, players, without_record, team })
}

#[derive(Serialize)]
pub struct Maximised {
    /// spelers van wie iets is veranderd
    pub players: usize,
    /// items die op het maximum zijn gezet
    pub items: usize,
    /// items die zijn toegevoegd voor instellingen die de speler nog niet kende
    pub added: usize,
    /// instellingen die niet toegevoegd konden worden omdat de lijst vol is
    pub no_room: usize,
    pub without_record: usize,
}

/// Zet het tactisch begrip van de (gekozen) spelers op het maximum, zoals FM's editor-actie.
/// Met `dry_run` wordt alleen geteld wat er zou veranderen.
pub fn maximise(uids: &Option<Vec<u32>>, dry_run: bool) -> Result<Maximised, String> {
    let (all, without_record) = load(game::squad()?)?;
    // de instelling die de meeste spelers kennen is die van de getrainde tactiek: die krijgt ook wie
    // hem nog niet kent (de andere items gaan toch al naar het maximum)
    let common = common_keys(&all);
    let wanted: Vec<Vec<u8>> = common.iter().map(|v| v.iter().take(1).filter(|(k, _)| *k <= MAX_KEY).map(|(k, _)| *k).collect()).collect();
    let chosen: Vec<&Loaded> = all.iter().filter(|l| uids.as_ref().is_none_or(|u| u.contains(&l.player.uid))).collect();

    let mut patches = Vec::new();
    let mut result = Maximised { players: 0, items: 0, added: 0, no_room: 0, without_record };
    for l in chosen {
        let before = patches.len();
        for a in 0..AREAS {
            let list = &l.rec.areas[a];
            let mut known = Vec::new();
            for (i, &it) in l.areas[a].iter().enumerate() {
                let (k, level) = decode(it);
                known.push(k);
                if level != MAX_LEVEL {
                    let new = (it & !LEVEL_MASK) | MAX_LEVEL;
                    patches.push(Patch { addr: list.begin + 4 * i as u64, old: it.to_le_bytes().to_vec(), new: new.to_le_bytes().to_vec() });
                    result.items += 1;
                }
            }
            // ontbrekende instellingen toevoegen zolang de lijst ruimte heeft (FM's lijst, FM's capaciteit)
            let mut end = list.end;
            for &k in wanted[a].iter().filter(|k| !known.contains(k)) {
                if end >= list.cap || list.begin == 0 || (end - list.begin) / 4 >= MAX_ITEMS {
                    result.no_room += 1;
                    continue;
                }
                let old = mem::read_bytes(end, 4).ok_or("niet leesbaar")?;
                patches.push(Patch { addr: end, old, new: encode(k, MAX_LEVEL).to_le_bytes().to_vec() });
                end += 4;
                result.added += 1;
            }
            if end != list.end {
                patches.push(Patch { addr: list.header + 8, old: list.end.to_le_bytes().to_vec(), new: end.to_le_bytes().to_vec() });
            }
        }
        for (i, it) in l.shapes.chunks_exact(SHAPE_ITEM as usize).enumerate() {
            let level = u16::from_le_bytes([it[SHAPE_LEVEL as usize], it[SHAPE_LEVEL as usize + 1]]);
            if level as u32 != MAX_LEVEL {
                let addr = l.rec.shapes.begin + SHAPE_ITEM * i as u64 + SHAPE_LEVEL;
                patches.push(Patch { addr, old: level.to_le_bytes().to_vec(), new: (MAX_LEVEL as u16).to_le_bytes().to_vec() });
                result.items += 1;
            }
        }
        if patches.len() > before {
            result.players += 1;
            // zoals FM na de actie: het record is veranderd
            if let Some(flag) = mem::u8_at(l.rec.addr + FLAG) {
                if flag != 1 {
                    patches.push(Patch { addr: l.rec.addr + FLAG, old: vec![flag], new: vec![1] });
                }
            }
        }
    }
    if !patches.is_empty() && !dry_run {
        game::apply_change(Change { label: "tactisch begrip / tactical understanding".into(), patches, ..Default::default() })?;
    }
    Ok(result)
}
