//! FM27-objecten: menselijke manager → contract → team → club → financiën, de selectie,
//! blessures en moraal.
//!
//! Veldposities voor macOS (Steam-build 26340721, arm64) komen uit de openbare geheugenkaart
//! van FMLibreEditor (profiles/fm27/layout-macos.toml) en FMSuperScout (docs/macos.md). Elke
//! keten wordt gecontroleerd (klassenaam, terugwijzende pointers, waarden binnen bereik)
//! voordat er iets gelezen of geschreven wordt.

use serde::Serialize;
use std::sync::Mutex;

use crate::mem;
use crate::rtti;

// ---- veldposities -----------------------------------------------------------
const HUMAN_CLASS: &str = "N2db16HUMAN_NON_PLAYERE";
const HUMAN_SHIFT: u64 = 0x340; // persoon- en identiteitsblok van de menselijke manager
const STAFF_CONTRACT: u64 = 0x170;
const STAFF_IDENTITY: u64 = 0x208;

const CONTRACT_OWNER: u64 = 0x08;
const CONTRACT_TEAM: u64 = 0x10;
const CONTRACT_UNHAPPINESS: u64 = 0x30;
const CONTRACT_HAPPINESS: u64 = 0x5A;

const TEAM_CLUB: u64 = 0x30;
const TEAM_PLAYERS: u64 = 0x38;

const CLUB_UID: u64 = 0x0C;
const CLUB_NAME: u64 = 0xC0;
const CLUB_FINANCE: u64 = 0x150;

const FIN_BALANCE: u64 = 0x0C;
const FIN_TRANSFER: u64 = 0x7C4;
const FIN_TOTAL_WAGE: u64 = 0x808;
const FIN_WAGE: u64 = 0x814;
const FIN_TICKET: u64 = 0x864;
const FIN_CORPORATE: u64 = 0x8A9;

const P_TEAM: u64 = 0x128;
const P_INJURIES: u64 = 0xF0;
const P_SHARPNESS: u64 = 0x250;
const P_CONDITION: u64 = 0x254;
const P_MORALE: u64 = 0x264;
const P_FIRST: u64 = 0x298;
const P_LAST: u64 = 0x2A0;
const P_COMMON: u64 = 0x2A8;
const P_CONTRACT: u64 = 0x2F0;
const P_IDENTITY: u64 = 0x388;
const P_UID: u64 = 0x394;
const PLAYER_COACH_SHIFT: u64 = 0xF8;

const INJ_KIND: u64 = 0x08;
const INJ_KIND_NAME: u64 = 0x18;
const ABSENCES: u64 = 0x18; // lijst van afwezigheden (schorsingen) in het blessure-record
const BAN_MARKER: u64 = 0x16;
const BAN_MATCHES: u64 = 0x1C;

// ---- toestand ---------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Patch {
    pub addr: u64,
    pub old: Vec<u8>,
    pub new: Vec<u8>,
}

#[derive(Default)]
struct State {
    human: Option<u64>,
    training_block: Option<u64>,
    human_vtables: Vec<u64>,
    undo: Vec<(String, Vec<Patch>)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

/// game_plugin-builds waarvoor alle posities gemeten en live getest zijn.
/// FM27 Steam-build 26340721, Apple silicon (gemeten 7 nov 2026).
const KNOWN_BUILDS: &[&str] = &["F4540355-2186-39CB-9119-C706A6A35346"];

/// (UUID van game_plugin, of die build bekend is).
pub fn build() -> (Option<String>, bool) {
    let uuid = mem::game_plugin().and_then(|i| i.uuid);
    let known = uuid.as_deref().is_some_and(|u| KNOWN_BUILDS.contains(&u));
    (uuid, known)
}

fn apply(label: &str, patches: Vec<Patch>) -> Result<usize, String> {
    if !build().1 {
        return Err(format!(
            "deze FM-versie ({}) is nog niet getest: alleen lezen. Update FM27 Trainer. / This FM build is not verified yet: read-only. Please update FM27 Trainer.",
            build().0.unwrap_or_else(|| "?".into())
        ));
    }
    let mut done: Vec<Patch> = Vec::new();
    for p in &patches {
        if let Err(e) = mem::write_checked(p.addr, &p.old, &p.new) {
            // terugdraaien wat al geschreven is
            for d in done.iter().rev() {
                let _ = mem::write_checked(d.addr, &d.new, &d.old);
            }
            return Err(e);
        }
        done.push(p.clone());
    }
    let n = done.len();
    with_state(|s| s.undo.push((label.to_string(), done)));
    Ok(n)
}

pub fn undo_last() -> Result<String, String> {
    let Some((label, patches)) = with_state(|s| s.undo.pop()) else {
        return Err("niets om ongedaan te maken".into());
    };
    for p in patches.iter().rev() {
        mem::write_checked(p.addr, &p.new, &p.old)?;
    }
    Ok(label)
}

// ---- manager en club --------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Chain {
    pub human: u64,
    pub contract: u64,
    pub team: u64,
    pub club: u64,
    pub finance: u64,
}

fn check_chain(h: u64) -> Option<Chain> {
    let contract = mem::u64_at(h + STAFF_CONTRACT + HUMAN_SHIFT)?;
    if !mem::is_ptr(contract) {
        return None;
    }
    // het contract wijst terug naar het identiteitsblok van de manager
    if mem::u64_at(contract + CONTRACT_OWNER)? != h + STAFF_IDENTITY + HUMAN_SHIFT {
        return None;
    }
    let team = mem::u64_at(contract + CONTRACT_TEAM)?;
    if !mem::is_ptr(team) {
        return None;
    }
    let club = mem::u64_at(team + TEAM_CLUB)?;
    if !mem::is_ptr(club) {
        return None;
    }
    let finance = mem::u64_at(club + CLUB_FINANCE)?;
    if !mem::is_ptr(finance) || !finance_looks_real(finance) {
        return None;
    }
    Some(Chain { human: h, contract, team, club, finance })
}

fn finance_looks_real(f: u64) -> bool {
    let (Some(tp), Some(corp), Some(tb), Some(wb), Some(twb)) = (
        mem::f32_at(f + FIN_TICKET),
        mem::u8_at(f + FIN_CORPORATE),
        mem::i32_at(f + FIN_TRANSFER),
        mem::i32_at(f + FIN_WAGE),
        mem::i32_at(f + FIN_TOTAL_WAGE),
    ) else {
        return false;
    };
    tp.is_finite() && (1.0..=1000.0).contains(&tp) && (1..=20).contains(&corp) && tb >= -1 && wb >= 0 && twb >= 0
}

/// Vindt (en onthoudt) de menselijke manager; zoekt de heap af als het nog niet bekend is.
pub fn chain() -> Result<Chain, String> {
    if let Some(h) = with_state(|s| s.human) {
        if let Some(c) = check_chain(h) {
            return Ok(c);
        }
    }
    let img = mem::game_plugin().ok_or("game_plugin niet geladen")?;
    let vts = with_state(|s| s.human_vtables.clone());
    let vts = if vts.is_empty() {
        let v = rtti::vtables_of(&img, HUMAN_CLASS);
        with_state(|s| s.human_vtables = v.clone());
        v
    } else {
        vts
    };
    if vts.is_empty() {
        return Err("klasse db::HUMAN_NON_PLAYER niet gevonden (andere FM-versie?)".into());
    }
    let objs = rtti::instances(&vts, 64);
    for h in objs {
        if let Some(c) = check_chain(h) {
            with_state(|s| s.human = Some(h));
            return Ok(c);
        }
    }
    Err("geen menselijke manager met club gevonden. Is er een save geladen?".into())
}

#[derive(Serialize)]
pub struct ClubInfo {
    pub uid: u32,
    pub name: String,
    pub balance: i32,
    pub transfer_budget: i32,
    pub wage_budget: i32,
    pub total_wage_budget: i32,
}

pub fn club() -> Result<ClubInfo, String> {
    let c = chain()?;
    let name = mem::u64_at(c.club + CLUB_NAME)
        .and_then(mem::fm_string)
        .or_else(|| mem::u64_at(c.club + CLUB_NAME).and_then(mem::u64_at).and_then(mem::fm_string))
        .unwrap_or_else(|| "?".into());
    Ok(ClubInfo {
        uid: mem::u32_at(c.club + CLUB_UID).unwrap_or(0),
        name,
        balance: mem::i32_at(c.finance + FIN_BALANCE).unwrap_or(0),
        transfer_budget: mem::i32_at(c.finance + FIN_TRANSFER).unwrap_or(0),
        wage_budget: mem::i32_at(c.finance + FIN_WAGE).unwrap_or(0),
        total_wage_budget: mem::i32_at(c.finance + FIN_TOTAL_WAGE).unwrap_or(0),
    })
}

fn i32_patch(addr: u64, new: i32) -> Result<Patch, String> {
    let old = mem::read_bytes(addr, 4).ok_or("niet leesbaar")?;
    Ok(Patch { addr, old, new: new.to_le_bytes().to_vec() })
}

pub fn set_finances(transfer: Option<i64>, wage: Option<i64>, balance: Option<i64>) -> Result<ClubInfo, String> {
    let c = chain()?;
    let clamp = |v: i64| v.clamp(0, i32::MAX as i64) as i32;
    let mut patches = Vec::new();
    if let Some(t) = transfer {
        patches.push(i32_patch(c.finance + FIN_TRANSFER, clamp(t))?);
    }
    if let Some(w) = wage {
        let w = clamp(w);
        patches.push(i32_patch(c.finance + FIN_WAGE, w)?);
        // het totale loonbudget mag niet lager zijn dan het loonbudget
        let total = mem::i32_at(c.finance + FIN_TOTAL_WAGE).unwrap_or(0);
        if total < w {
            patches.push(i32_patch(c.finance + FIN_TOTAL_WAGE, w)?);
        }
    }
    if let Some(b) = balance {
        patches.push(i32_patch(c.finance + FIN_BALANCE, b.clamp(i32::MIN as i64, i32::MAX as i64) as i32)?);
    }
    if patches.is_empty() {
        return club();
    }
    apply("financiën", patches)?;
    club()
}

// ---- selectie ---------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct Injury {
    pub name: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct PlayerInfo {
    pub uid: u32,
    pub name: String,
    pub morale: u8,
    pub condition: u16,
    pub sharpness: u16,
    pub happiness: Option<i16>,
    pub unhappy_reasons: bool,
    pub injuries: Vec<Injury>,
    pub bans: u32,
    #[serde(skip)]
    pub addr: u64,
    #[serde(skip)]
    pub shift: u64,
}

fn name_entry(entry: u64) -> Option<String> {
    if !mem::is_ptr(entry) {
        return None;
    }
    mem::fm_string(mem::u64_at(entry)?)
}

fn read_player(p: u64, team: u64) -> Option<PlayerInfo> {
    if mem::u64_at(p + P_TEAM)? != team {
        return None;
    }
    let class = rtti::class_name(p)?;
    if !class.contains("PLAYER") {
        return None;
    }
    let shift = if class.contains("PLAYER_AND_NON_PLAYER") { PLAYER_COACH_SHIFT } else { 0 };
    let uid = mem::u32_at(p + P_UID + shift)?;
    let common = mem::u64_at(p + P_COMMON + shift).and_then(name_entry);
    let first = mem::u64_at(p + P_FIRST + shift).and_then(name_entry);
    let last = mem::u64_at(p + P_LAST + shift).and_then(name_entry);
    let name = common.unwrap_or_else(|| match (first, last) {
        (Some(f), Some(l)) => format!("{f} {l}"),
        (f, l) => f.or(l).unwrap_or_else(|| format!("UID {uid}")),
    });
    let morale = mem::u8_at(p + P_MORALE)?;
    let (mut happiness, mut unhappy) = (None, false);
    if let Some(ct) = mem::u64_at(p + P_CONTRACT + shift).filter(|c| mem::is_ptr(*c)) {
        if mem::u64_at(ct + CONTRACT_OWNER) == Some(p + P_IDENTITY + shift) {
            happiness = mem::i16_at(ct + CONTRACT_HAPPINESS);
            unhappy = mem::u64_at(ct + CONTRACT_UNHAPPINESS).is_some_and(|x| x != 0);
        }
    }
    let mut injuries = Vec::new();
    let mut bans = 0;
    if let Some((_, recs)) = injury_list(p) {
        for r in recs {
            let name = mem::u64_at(r + INJ_KIND)
                .filter(|k| mem::is_ptr(*k))
                .and_then(|k| mem::u64_at(k + INJ_KIND_NAME))
                .and_then(|n| mem::fm_string(n).or_else(|| mem::u64_at(n).and_then(mem::fm_string)));
            injuries.push(Injury { name });
        }
    }
    if let Some((_, recs)) = absence_list(p) {
        bans = recs.iter().filter(|e| mem::u16_at(**e + BAN_MARKER) == Some(0xFFFF)).count() as u32;
    }
    Some(PlayerInfo {
        uid,
        name,
        morale,
        condition: mem::u16_at(p + P_CONDITION).unwrap_or(0),
        sharpness: mem::u16_at(p + P_SHARPNESS).unwrap_or(0),
        happiness,
        unhappy_reasons: unhappy,
        injuries,
        bans,
        addr: p,
        shift,
    })
}

/// (adres van de lijstkop, elementen) van een (begin, end, cap)-lijst met pointers.
fn pointer_list(header: u64, max: usize) -> Option<(u64, Vec<u64>)> {
    let begin = mem::u64_at(header)?;
    let end = mem::u64_at(header + 8)?;
    if begin == 0 && end == 0 {
        return Some((header, Vec::new()));
    }
    if !mem::is_ptr(begin) || end < begin || (end - begin) % 8 != 0 || ((end - begin) / 8) as usize > max {
        return None;
    }
    let n = ((end - begin) / 8) as usize;
    let bytes = mem::read_bytes(begin, n * 8)?;
    let v: Vec<u64> = bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
    v.iter().all(|x| mem::is_ptr(*x)).then_some((header, v))
}

fn injury_list(p: u64) -> Option<(u64, Vec<u64>)> {
    let rec = mem::u64_at(p + P_INJURIES)?;
    if !mem::is_ptr(rec) {
        return None;
    }
    pointer_list(rec, 16)
}

fn absence_list(p: u64) -> Option<(u64, Vec<u64>)> {
    let rec = mem::u64_at(p + P_INJURIES)?;
    if !mem::is_ptr(rec) {
        return None;
    }
    pointer_list(rec + ABSENCES, 32)
}

pub fn squad() -> Result<Vec<PlayerInfo>, String> {
    let c = chain()?;
    let (_, players) = pointer_list(c.team + TEAM_PLAYERS, 200).ok_or("spelerslijst van het team niet leesbaar")?;
    let mut out: Vec<PlayerInfo> = players.into_iter().filter_map(|p| read_player(p, c.team)).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn select(uids: &Option<Vec<u32>>) -> Result<Vec<PlayerInfo>, String> {
    let all = squad()?;
    Ok(match uids {
        None => all,
        Some(u) => all.into_iter().filter(|p| u.contains(&p.uid)).collect(),
    })
}

/// Geneest blessures zoals de game zelf doet: de lijst met huidige blessures leegmaken.
pub fn heal(uids: &Option<Vec<u32>>) -> Result<usize, String> {
    let mut patches = Vec::new();
    for p in select(uids)? {
        if let Some((hdr, recs)) = injury_list(p.addr) {
            if recs.is_empty() {
                continue;
            }
            let begin = mem::u64_at(hdr).ok_or("lijst niet leesbaar")?;
            let end = mem::u64_at(hdr + 8).ok_or("lijst niet leesbaar")?;
            patches.push(Patch { addr: hdr + 8, old: end.to_le_bytes().to_vec(), new: begin.to_le_bytes().to_vec() });
        }
    }
    let n = patches.len();
    if n > 0 {
        apply("blessures genezen", patches)?;
    }
    Ok(n)
}

/// Heft schorsingen op (andere afwezigheden, zoals interlands, blijven staan).
pub fn lift_bans(uids: &Option<Vec<u32>>) -> Result<usize, String> {
    let mut patches = Vec::new();
    let mut count = 0;
    for p in select(uids)? {
        let Some((hdr, recs)) = absence_list(p.addr) else { continue };
        let kept: Vec<u64> = recs.iter().copied().filter(|e| mem::u16_at(*e + BAN_MARKER) != Some(0xFFFF)).collect();
        if kept.len() == recs.len() {
            continue;
        }
        count += recs.len() - kept.len();
        let begin = mem::u64_at(hdr).ok_or("lijst niet leesbaar")?;
        let end = mem::u64_at(hdr + 8).ok_or("lijst niet leesbaar")?;
        let old: Vec<u8> = recs.iter().flat_map(|e| e.to_le_bytes()).collect();
        let mut new: Vec<u8> = kept.iter().flat_map(|e| e.to_le_bytes()).collect();
        new.extend_from_slice(&old[new.len()..]);
        patches.push(Patch { addr: begin, old, new });
        let new_end = begin + 8 * kept.len() as u64;
        patches.push(Patch { addr: hdr + 8, old: end.to_le_bytes().to_vec(), new: new_end.to_le_bytes().to_vec() });
    }
    let _ = BAN_MATCHES;
    if !patches.is_empty() {
        apply("schorsingen opgeheven", patches)?;
    }
    Ok(count)
}

/// Moraal naar 20 (top) en de redenen om ontevreden te zijn weg, zoals FMRTE's "Inspire".
pub fn inspire(uids: &Option<Vec<u32>>, morale: u8, clear_unhappiness: bool) -> Result<usize, String> {
    let morale = morale.clamp(1, 20);
    let mut patches = Vec::new();
    let mut n = 0;
    for p in select(uids)? {
        let mut touched = false;
        let cur = mem::u8_at(p.addr + P_MORALE).ok_or("moraal niet leesbaar")?;
        if !(1..=20).contains(&cur) {
            continue; // past niet bij wat we verwachten: niet aankomen
        }
        if cur != morale {
            patches.push(Patch { addr: p.addr + P_MORALE, old: vec![cur], new: vec![morale] });
            touched = true;
        }
        if clear_unhappiness {
            if let Some(ct) = mem::u64_at(p.addr + P_CONTRACT + p.shift).filter(|c| mem::is_ptr(*c)) {
                if mem::u64_at(ct + CONTRACT_OWNER) == Some(p.addr + P_IDENTITY + p.shift) {
                    let reasons = mem::u64_at(ct + CONTRACT_UNHAPPINESS).unwrap_or(0);
                    if reasons != 0 {
                        patches.push(Patch {
                            addr: ct + CONTRACT_UNHAPPINESS,
                            old: reasons.to_le_bytes().to_vec(),
                            new: 0u64.to_le_bytes().to_vec(),
                        });
                        touched = true;
                    }
                }
            }
        }
        if touched {
            n += 1;
        }
    }
    if !patches.is_empty() {
        apply("moraal/happiness", patches)?;
    }
    Ok(n)
}

// ---- training -----------------------------------------------------------------
//
// TRAINING_MANAGER houdt per team een blok bij waarin de team-pointer staat en, 0x28 verder,
// een lijst (begin, end, cap) met pointers naar weekrecords van 0x60 bytes:
//   +0x0C u16 dag-van-het-jaar (lage 9 bits) + u16 jaar  (maandag van de week)
//   +0x10 7 dagblokken van 8 bytes: u16 0, u16 sessie 1, u16 sessie 2, u16 extra
// Gemeten op FM27 (Steam-build 26340721, arm64) en gecontroleerd tegen de kalender in de save.

const TB_WEEKS: u64 = 0x28;
const WEEK_DATE: u64 = 0x0C;
const WEEK_DAYS: u64 = 0x10;
const TEAM_SCHEDULE: u64 = 0xA0;
const SCHEDULE_DATE: u64 = 0x94;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, serde::Deserialize)]
pub struct FmDate {
    pub year: u16,
    pub doy: u16,
}

impl FmDate {
    fn read(addr: u64) -> Option<FmDate> {
        let raw = mem::u16_at(addr)?;
        let year = mem::u16_at(addr + 2)?;
        let doy = raw & 0x1FF;
        ((1900..=2200).contains(&year) && (1..=366).contains(&doy)).then_some(FmDate { year, doy })
    }
    fn is_leap(y: u16) -> bool {
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }
    /// Datum `n` dagen later.
    pub fn plus(self, n: u16) -> FmDate {
        let mut d = FmDate { year: self.year, doy: self.doy + n };
        let len = if Self::is_leap(d.year) { 366 } else { 365 };
        if d.doy > len {
            d.doy -= len;
            d.year += 1;
        }
        d
    }
}

pub fn game_date() -> Result<FmDate, String> {
    let c = chain()?;
    let s = mem::u64_at(c.team + TEAM_SCHEDULE).filter(|p| mem::is_ptr(*p)).ok_or("schema van het team niet gevonden")?;
    FmDate::read(s + SCHEDULE_DATE).ok_or_else(|| "spel-datum niet leesbaar".into())
}

fn week_ok(rec: u64) -> bool {
    if !mem::is_ptr(rec) || FmDate::read(rec + WEEK_DATE).is_none() {
        return false;
    }
    let Some(b) = mem::read_bytes(rec + WEEK_DAYS, 56) else { return false };
    b.chunks_exact(8).all(|d| {
        let v: Vec<u16> = d.chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect();
        v[0] == 0 && v[1..].iter().all(|c| *c <= 600)
    })
}

fn weeks_header_ok(hdr: u64) -> Option<Vec<u64>> {
    let (_, list) = pointer_list(hdr, 520)?;
    if list.is_empty() || !week_ok(list[0]) || !week_ok(*list.last()?) {
        return None;
    }
    Some(list)
}

/// Het trainingsblok van het team van de manager (adres van de team-pointer erin).
fn training_block(team: u64) -> Result<u64, String> {
    if let Some(b) = with_state(|s| s.training_block) {
        if mem::u64_at(b) == Some(team) && weeks_header_ok(b + TB_WEEKS).is_some() {
            return Ok(b);
        }
    }
    let set: std::collections::HashSet<u64> = [team].into_iter().collect();
    for (a, _) in mem::scan_u64(&set, 20_000) {
        if weeks_header_ok(a + TB_WEEKS).is_some() {
            with_state(|s| s.training_block = Some(a));
            return Ok(a);
        }
    }
    Err("trainingskalender van het team niet gevonden".into())
}

#[derive(Serialize)]
pub struct Week {
    pub addr: String,
    pub start: FmDate,
    pub days: Vec<[u16; 3]>,
}

fn read_week(rec: u64) -> Option<Week> {
    let start = FmDate::read(rec + WEEK_DATE)?;
    let b = mem::read_bytes(rec + WEEK_DAYS, 56)?;
    let days = b
        .chunks_exact(8)
        .map(|d| {
            let u = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
            [u(2), u(4), u(6)]
        })
        .collect();
    Some(Week { addr: format!("{rec:#x}"), start, days })
}

pub fn training_weeks() -> Result<(FmDate, Vec<Week>), String> {
    let c = chain()?;
    let block = training_block(c.team)?;
    let list = weeks_header_ok(block + TB_WEEKS).ok_or("weeklijst niet leesbaar")?;
    let today = game_date()?;
    Ok((today, list.into_iter().filter_map(read_week).collect()))
}

/// Schrijft de sessies van één week, alleen voor dagen ná de huidige spel-datum. Wedstrijd-
/// en reisdagen (500/402) blijven altijd zoals FM ze heeft gezet.
pub fn set_week(start: FmDate, days: &[[u16; 3]]) -> Result<usize, String> {
    if days.len() != 7 {
        return Err("een week heeft 7 dagen".into());
    }
    let c = chain()?;
    let block = training_block(c.team)?;
    let list = weeks_header_ok(block + TB_WEEKS).ok_or("weeklijst niet leesbaar")?;
    let today = game_date()?;
    let rec = list
        .into_iter()
        .find(|r| FmDate::read(r + WEEK_DATE) == Some(start))
        .ok_or("die week staat (nog) niet in de kalender van FM")?;
    let cur = read_week(rec).ok_or("week niet leesbaar")?;
    let mut patches = Vec::new();
    for d in 0..7u16 {
        let date = start.plus(d);
        if date <= today {
            continue; // vandaag en eerder: niet meer aanpassen
        }
        let old = cur.days[d as usize];
        if old.iter().any(|x| *x == 500 || *x == 402) {
            continue; // wedstrijd of reis: van FM
        }
        let new = days[d as usize];
        if new.iter().any(|x| *x == 500 || *x == 402 || *x > 600) || new == old {
            continue;
        }
        let addr = rec + WEEK_DAYS + d as u64 * 8 + 2;
        let ob: Vec<u8> = old.iter().flat_map(|x| x.to_le_bytes()).collect();
        let nb: Vec<u8> = new.iter().flat_map(|x| x.to_le_bytes()).collect();
        patches.push(Patch { addr, old: ob, new: nb });
    }
    let n = patches.len();
    if n > 0 {
        apply("training", patches)?;
    }
    Ok(n)
}
