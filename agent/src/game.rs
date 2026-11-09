//! FM27-objecten: menselijke manager → contract → team → club → financiën, de selectie,
//! blessures, moraal en de trainingskalender.
//!
//! Veldposities komen uit de openbare geheugenkaarten van FMLibreEditor (profiles/fm27:
//! layout.toml voor Windows, layout-macos.toml voor macOS) en FMSuperScout (docs/macos.md).
//! Clubs, financiën, contracten, teams en blessures liggen op beide platforms hetzelfde; spelers
//! en staf niet (MSVC en clang leggen klassen met virtuele bases anders uit). Elke keten wordt
//! gecontroleerd (terugwijzende pointers, waarden binnen bereik) voordat er iets gelezen of
//! geschreven wordt.

use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::mem::{self, Abi};
use crate::rtti;

// ---- veldposities die per platform verschillen ------------------------------------

pub struct Layout {
    /// staf: contract-pointer en het blok waar een contract naar terugwijst
    pub staff_contract: u64,
    pub staff_identity: u64,
    /// in het object van de menselijke manager liggen die velden zoveel verder
    pub human_shift: u64,
    pub p_team: u64,
    pub p_injuries: u64,
    pub p_sharpness: u64,
    pub p_condition: u64,
    pub p_morale: u64,
    pub p_first: u64,
    pub p_last: u64,
    pub p_common: u64,
    pub p_contract: u64,
    pub p_identity: u64,
    pub p_uid: u64,
    /// speler-trainers hebben een extra blok: hun persoons- en identiteitsvelden liggen verder
    pub player_coach_shift: u64,
    /// i32 pond: transferwaarde en vraagprijs (-1 = geen)
    pub p_value: u64,
    pub p_asking: u64,
    /// pointer naar het record met tactisch begrip (zie understanding.rs)
    pub p_understanding: u64,
    /// u32 met de editorvlaggen van de manager (registratie/rugnummers altijd), als bekend
    pub human_flags: Option<u64>,
    /// blokgroottes van de allocator van de game (FMLibreEditor [heap]): een lijst van n pointers
    /// zit in het kleinste blok dat 8n bytes houdt
    pub heap_blocks: &'static [u64],
}

/// macOS (clang), Steam-build 26340721: gemeten en live getest.
pub const MAC: Layout = Layout {
    staff_contract: 0x170,
    staff_identity: 0x208,
    human_shift: 0x340,
    p_team: 0x128,
    p_injuries: 0xF0,
    p_sharpness: 0x250,
    p_condition: 0x254,
    p_morale: 0x264,
    p_first: 0x298,
    p_last: 0x2A0,
    p_common: 0x2A8,
    p_contract: 0x2F0,
    p_identity: 0x388,
    p_uid: 0x394,
    player_coach_shift: 0xF8,
    p_value: 0x22C,
    p_asking: 0x230,
    // niet gemeten op de Mac: Windows 0x100 min het vbptr-woord; het record wordt streng gecontroleerd
    p_understanding: 0xF8,
    human_flags: None,
    heap_blocks: &[144, 160, 192, 224, 256, 288, 320, 384, 448, 512, 640, 768, 896, 1024, 1280, 1536, 2048, 2560, 3072, 3584, 4096],
};

/// Windows (MSVC), FMLibreEditor layout.toml (geverifieerd op de Steam- en Game Pass-builds), door dit
/// project live getest op Steam-build 26340721 (8 nov 2026): de eigen velden van een speler liggen
/// 8 verder (vbptr), het persoonsblok 0x40 verder en het identiteitsblok, waar contracten naar
/// wijzen, komt vóór de persoon.
pub const WIN: Layout = Layout {
    staff_contract: 0x1A8,
    staff_identity: 0x100,
    human_shift: 0x340,
    p_team: 0x130,
    p_injuries: 0xF8,
    p_sharpness: 0x258,
    p_condition: 0x25C,
    p_morale: 0x26C,
    p_first: 0x2D8,
    p_last: 0x2E0,
    p_common: 0x2E8,
    p_contract: 0x330,
    p_identity: 0x288,
    p_uid: 0x294,
    player_coach_shift: 0xF8,
    p_value: 0x234,
    p_asking: 0x238,
    // gemeten (8 nov 2026) door FM's editor-actie "Maximise Tactical Understanding" te ontleden
    p_understanding: 0x100,
    human_flags: Some(0x3FC),
    heap_blocks: &[144, 160, 192, 224, 256, 288, 320, 384, 448, 512],
};

pub fn layout() -> &'static Layout {
    match mem::abi() {
        Abi::Itanium => &MAC,
        Abi::Msvc => &WIN,
    }
}

// ---- veldposities die op beide platforms gelijk zijn --------------------------------

pub(crate) const CONTRACT_OWNER: u64 = 0x08;
pub(crate) const CONTRACT_TEAM: u64 = 0x10;
const CONTRACT_WAGE: u64 = 0x20;
const CONTRACT_UNHAPPINESS: u64 = 0x30;
const CONTRACT_EXPIRES: u64 = 0x48;
const CONTRACT_LISTING: u64 = 0x57;
const CONTRACT_HAPPINESS: u64 = 0x5A;

pub(crate) const TEAM_CLUB: u64 = 0x30;
pub(crate) const TEAM_PLAYERS: u64 = 0x38;

pub(crate) const CLUB_UID: u64 = 0x0C;
const CLUB_NAME: u64 = 0xC0;
pub(crate) const CLUB_FINANCE: u64 = 0x150;

pub(crate) const FIN_BALANCE: u64 = 0x0C;
pub(crate) const FIN_TRANSFER: u64 = 0x7C4;
const FIN_TOTAL_WAGE: u64 = 0x808;
const FIN_WAGE: u64 = 0x814;
const FIN_TICKET: u64 = 0x864;
const FIN_CORPORATE: u64 = 0x8A9;

const INJ_KIND: u64 = 0x08;
const INJ_KIND_NAME: u64 = 0x18;
const ABSENCES: u64 = 0x18; // lijst van afwezigheden (schorsingen) in het blessure-record
const BAN_MARKER: u64 = 0x16;

// ---- builds ---------------------------------------------------------------------------

/// Een FM-build waarvan de vaste adressen bekend zijn.
pub struct Build {
    pub id: &'static str,
    pub label: &'static str,
    /// alle aanpassingen zijn op deze build live getest
    pub writes_tested: bool,
    /// RVA in game_plugin van de spel-datum (u16 dag van het jaar + u16 jaar)
    pub current_date: u64,
    /// RVA in game_plugin van het (begin, end)-paar van de staflijst
    pub staff: u64,
    /// RVA in game_plugin van het (begin, end)-paar van de clublijst
    pub clubs: u64,
}

pub const BUILDS: &[Build] = &[
    // gemeten en live getest (7 nov 2026); RVA's uit FMLibreEditor steam-26340721-macos-arm64.toml
    Build { id: "F4540355-2186-39CB-9119-C706A6A35346", label: "Steam 26340721 macOS arm64", writes_tested: true, current_date: 0x56B1DB8, staff: 0x56D2E60, clubs: 0x56D2FD0 },
    // FMLibreEditor steam-26340721.toml: game_plugin.dll met SizeOfImage 0x1821C000 en linktijd 0x6AE565AF;
    // live getest (8 nov 2026, Ajax-save): uitlezen, budget, moraal, genezen, training, verkopen, undo
    Build { id: "PE-6AE565AF-1821C000", label: "Steam 26340721 Windows", writes_tested: true, current_date: 0x56AF378, staff: 0x5705818, clubs: 0x5705988 },
    // Game Pass / Microsoft Store / Xbox-app 0.9.2409.0 (update van 2 nov 2026), FMLibreEditor
    // gamepass-0.9.2409.0.toml: dezelfde update als Steam 26340721 (526 s eerder gelinkt), objectindeling
    // gelijk, alleen de vaste plekken liggen anders. Niet door dit project live getest; aanpassen staat
    // aan omdat elke keten en lijst vóór het schrijven op dezelfde manier gecontroleerd wordt als op Steam.
    Build { id: "PE-6AE563A1-175DA000", label: "Game Pass 0.9.2409.0 Windows", writes_tested: true, current_date: 0x56257C8, staff: 0x5683E08, clubs: 0x5683F78 },
];

/// Aanpassen toestaan op een build die (nog) niet live getest is; de gebruiker zet dit zelf aan.
static ALLOW_UNTESTED: AtomicBool = AtomicBool::new(false);

pub fn set_allow_untested(on: bool) {
    ALLOW_UNTESTED.store(on, Ordering::Release);
}

pub fn allow_untested() -> bool {
    ALLOW_UNTESTED.load(Ordering::Acquire)
}

pub(crate) fn profile(id: Option<&str>) -> Option<&'static Build> {
    BUILDS.iter().find(|b| Some(b.id) == id)
}

// ---- toestand ---------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Patch {
    pub addr: u64,
    pub old: Vec<u8>,
    pub new: Vec<u8>,
}

/// Eén wijziging zoals hij ongedaan kan worden gemaakt.
#[derive(Clone, Debug, Default)]
pub(crate) struct Change {
    pub label: String,
    /// geschreven alleen als het geheugen nog het verwachte bevat; ongedaan maken net zo
    pub patches: Vec<Patch>,
    /// zoals `patches`, maar ongedaan maken slaat ze over als de game ze intussen heeft veranderd
    /// (de opstellingen)
    pub soft: Vec<Patch>,
    /// geldbedragen (i32) die met zoveel veranderen; ongedaan maken trekt het verschil eraf van
    /// wat er dan staat, want de game boekt elke dag zelf bij en af
    pub deltas: Vec<(u64, i64)>,
}

#[derive(Default)]
struct State {
    human: Option<u64>,
    /// (adres van de team-pointer in het trainingsblok, afstand tot de weeklijst)
    training_block: Option<(u64, u64)>,
    human_vtables: Vec<u64>,
    undo: Vec<Change>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

/// (build-id van game_plugin, of aanpassen op deze build live getest is).
pub fn build() -> (Option<String>, bool) {
    let id = mem::game_plugin().and_then(|i| i.id);
    let tested = profile(id.as_deref()).is_some_and(|b| b.writes_tested);
    (id, tested)
}

/// Wordt gebruikt na het opnieuw verbinden met FM (Windows): alles wat onthouden is, is weg.
pub fn reset() {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    *g = None;
    drop(g);
    crate::sell::reset();
}

/// Mag er op deze build geschreven worden?
pub(crate) fn writes_allowed() -> Result<(), String> {
    let (id, tested) = build();
    if !tested && !allow_untested() {
        return Err(format!(
            "deze FM-versie ({}) is nog niet getest: alleen lezen. Zet in Instellingen 'Aanpassen op niet-geteste FM-versie' aan om het toch te proberen. / This FM build ({0}) is not verified yet: read-only. Turn on 'Allow changes on an untested FM build' in Settings to try anyway.",
            id.unwrap_or_else(|| "?".into())
        ));
    }
    Ok(())
}

fn add_i32(addr: u64, by: i64) -> Result<(), String> {
    let now = mem::i32_at(addr).ok_or_else(|| format!("{addr:#x} niet leesbaar"))?;
    let next = i32::try_from(now as i64 + by).map_err(|_| "bedrag past niet / amount out of range".to_string())?;
    mem::write_checked(addr, &now.to_le_bytes(), &next.to_le_bytes())
}

fn apply(label: &str, patches: Vec<Patch>) -> Result<usize, String> {
    let n = patches.len();
    apply_change(Change { label: label.to_string(), patches, ..Default::default() })?;
    Ok(n)
}

/// Schrijft een wijziging (eerst `soft`, dan `patches`, dan de bedragen). Lukt één stap niet,
/// dan wordt alles wat al geschreven is teruggezet.
pub(crate) fn apply_change(change: Change) -> Result<(), String> {
    writes_allowed()?;
    let mut done: Vec<&Patch> = Vec::new();
    let mut paid: Vec<(u64, i64)> = Vec::new();
    let rollback = |done: &[&Patch], paid: &[(u64, i64)]| {
        for (a, by) in paid.iter().rev() {
            let _ = add_i32(*a, -by);
        }
        for d in done.iter().rev() {
            let _ = mem::write_checked(d.addr, &d.new, &d.old);
        }
    };
    for p in change.soft.iter().chain(&change.patches) {
        if let Err(e) = mem::write_checked(p.addr, &p.old, &p.new) {
            rollback(&done, &paid);
            return Err(e);
        }
        done.push(p);
    }
    for (a, by) in &change.deltas {
        if let Err(e) = add_i32(*a, *by) {
            rollback(&done, &paid);
            return Err(e);
        }
        paid.push((*a, *by));
    }
    with_state(|s| s.undo.push(change.clone()));
    Ok(())
}

pub fn undo_last() -> Result<String, String> {
    let Some(change) = with_state(|s| s.undo.pop()) else {
        return Err("niets om ongedaan te maken".into());
    };
    for (a, by) in change.deltas.iter().rev() {
        add_i32(*a, -by)?;
    }
    for p in change.patches.iter().rev() {
        mem::write_checked(p.addr, &p.new, &p.old)?;
    }
    for p in change.soft.iter().rev() {
        let _ = mem::write_checked(p.addr, &p.new, &p.old);
    }
    Ok(change.label)
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
    let l = layout();
    let contract = mem::u64_at(h + l.staff_contract + l.human_shift)?;
    if !mem::is_ptr(contract) {
        return None;
    }
    // het contract wijst terug naar het identiteitsblok van de manager
    if mem::u64_at(contract + CONTRACT_OWNER)? != h + l.staff_identity + l.human_shift {
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

pub(crate) fn finance_looks_real(f: u64) -> bool {
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

/// Zoekt de manager via de vtables van db::HUMAN_NON_PLAYER (RTTI).
fn human_via_rtti(img: &mem::Image) -> Option<Chain> {
    let vts = with_state(|s| s.human_vtables.clone());
    let vts = if vts.is_empty() {
        let v = rtti::vtables_of(img, &rtti::decorated("db", "HUMAN_NON_PLAYER"));
        with_state(|s| s.human_vtables = v.clone());
        v
    } else {
        vts
    };
    if vts.is_empty() {
        return None;
    }
    rtti::instances(&vts, 64).into_iter().find_map(check_chain)
}

/// Zoekt de manager in de staflijst van het spel (vaste plek per build).
fn human_via_staff(img: &mem::Image) -> Option<Chain> {
    let b = profile(img.id.as_deref())?;
    let pair = img.start + b.staff;
    let (begin, end) = (mem::u64_at(pair)?, mem::u64_at(pair + 8)?);
    if !mem::is_ptr(begin) || end < begin || (end - begin) % 8 != 0 || (end - begin) / 8 > 400_000 {
        return None;
    }
    let bytes = mem::read_bytes(begin, (end - begin) as usize)?;
    bytes
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .filter(|p| mem::is_ptr(*p)) // lege plekken overslaan
        .find_map(check_chain)
}

/// Vindt (en onthoudt) de menselijke manager.
pub fn chain() -> Result<Chain, String> {
    if let Some(h) = with_state(|s| s.human) {
        if let Some(c) = check_chain(h) {
            return Ok(c);
        }
    }
    let img = mem::game_plugin().ok_or("game_plugin niet geladen (laadt FM nog?) / game_plugin not loaded yet")?;
    // op Windows eerst de staflijst (geen RTTI nodig), op de Mac eerst de geteste RTTI-weg
    let found = match mem::abi() {
        Abi::Msvc => human_via_staff(&img).or_else(|| human_via_rtti(&img)),
        Abi::Itanium => human_via_rtti(&img).or_else(|| human_via_staff(&img)),
    };
    match found {
        Some(c) => {
            with_state(|s| s.human = Some(c.human));
            Ok(c)
        }
        None => Err("geen menselijke manager met club gevonden. Is er een save geladen? / No human manager with a club found. Is a save loaded?".into()),
    }
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

/// Naam van een club (FM-string, soms via nog een pointer).
pub(crate) fn club_name(club: u64) -> Option<String> {
    mem::u64_at(club + CLUB_NAME)
        .and_then(mem::fm_string)
        .or_else(|| mem::u64_at(club + CLUB_NAME).and_then(mem::u64_at).and_then(mem::fm_string))
}

pub fn club() -> Result<ClubInfo, String> {
    let c = chain()?;
    let name = club_name(c.club).unwrap_or_else(|| "?".into());
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
    /// transferwaarde en vraagprijs in pond (-1 = geen vraagprijs)
    pub value: i32,
    pub asking_price: i32,
    /// salaris in pond per week en einde contract
    pub wage: Option<i32>,
    pub contract_ends: Option<FmDate>,
    /// op de transferlijst (bit 0) of huurlijst (bit 1)
    pub listed: u8,
    /// speler-trainer: (nog) niet te verkopen
    pub player_coach: bool,
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

/// Het contract van een speler, als het terugwijst naar zijn identiteitsblok.
pub(crate) fn player_contract(p: u64, shift: u64) -> Option<u64> {
    let l = layout();
    let ct = mem::u64_at(p + l.p_contract + shift).filter(|c| mem::is_ptr(*c))?;
    (mem::u64_at(ct + CONTRACT_OWNER)? == p + l.p_identity + shift).then_some(ct)
}

/// Gewone speler (0) of speler-trainer (extra blok): het contract dat terugwijst beslist,
/// anders de klassenaam.
fn player_shift(p: u64, class: Option<&str>) -> u64 {
    let coach = layout().player_coach_shift;
    if player_contract(p, 0).is_some() {
        return 0;
    }
    if player_contract(p, coach).is_some() {
        return coach;
    }
    if class.is_some_and(|c| c.contains("PLAYER_AND_NON_PLAYER")) { coach } else { 0 }
}

fn read_player(p: u64, team: u64) -> Option<PlayerInfo> {
    let l = layout();
    if mem::u64_at(p + l.p_team)? != team {
        return None;
    }
    let class = rtti::class_name(p);
    if class.as_deref().is_some_and(|c| !c.contains("PLAYER")) {
        return None;
    }
    let shift = player_shift(p, class.as_deref());
    let uid = mem::u32_at(p + l.p_uid + shift)?;
    let common = mem::u64_at(p + l.p_common + shift).and_then(name_entry);
    let first = mem::u64_at(p + l.p_first + shift).and_then(name_entry);
    let last = mem::u64_at(p + l.p_last + shift).and_then(name_entry);
    let name = common.unwrap_or_else(|| match (first, last) {
        (Some(f), Some(l)) => format!("{f} {l}"),
        (f, l) => f.or(l).unwrap_or_else(|| format!("UID {uid}")),
    });
    let morale = mem::u8_at(p + l.p_morale)?;
    let (mut happiness, mut unhappy) = (None, false);
    let (mut wage, mut contract_ends, mut listed) = (None, None, 0);
    if let Some(ct) = player_contract(p, shift) {
        // i8, -100..100 (FMLibreEditor: happiness 0x5A, playing-time happiness 0x5C)
        happiness = mem::u8_at(ct + CONTRACT_HAPPINESS).map(|v| v as i8 as i16);
        unhappy = mem::u64_at(ct + CONTRACT_UNHAPPINESS).is_some_and(|x| x != 0);
        wage = mem::i32_at(ct + CONTRACT_WAGE);
        contract_ends = FmDate::read(ct + CONTRACT_EXPIRES);
        listed = mem::u8_at(ct + CONTRACT_LISTING).unwrap_or(0) & 0b11;
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
        condition: mem::u16_at(p + l.p_condition).unwrap_or(0),
        sharpness: mem::u16_at(p + l.p_sharpness).unwrap_or(0),
        happiness,
        unhappy_reasons: unhappy,
        injuries,
        bans,
        value: mem::i32_at(p + l.p_value).unwrap_or(0),
        asking_price: mem::i32_at(p + l.p_asking).unwrap_or(-1),
        wage,
        contract_ends,
        listed,
        player_coach: shift != 0,
        addr: p,
        shift,
    })
}

/// (adres van de lijstkop, elementen) van een (begin, end, cap)-lijst met pointers.
pub(crate) fn pointer_list(header: u64, max: usize) -> Option<(u64, Vec<u64>)> {
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

fn injury_record(p: u64) -> Option<u64> {
    mem::u64_at(p + layout().p_injuries).filter(|r| mem::is_ptr(*r))
}

fn injury_list(p: u64) -> Option<(u64, Vec<u64>)> {
    pointer_list(injury_record(p)?, 16)
}

fn absence_list(p: u64) -> Option<(u64, Vec<u64>)> {
    pointer_list(injury_record(p)? + ABSENCES, 32)
}

/// De spelers van een team (alleen wie zelf naar dat team wijst), op naam.
pub(crate) fn squad_of(team: u64) -> Result<Vec<PlayerInfo>, String> {
    let (_, players) = pointer_list(team + TEAM_PLAYERS, 200).ok_or("spelerslijst van het team niet leesbaar / team's player list not readable")?;
    let mut out: Vec<PlayerInfo> = players.into_iter().filter_map(|p| read_player(p, team)).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn squad() -> Result<Vec<PlayerInfo>, String> {
    squad_of(chain()?.team)
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
    if !patches.is_empty() {
        apply("schorsingen opgeheven", patches)?;
    }
    Ok(count)
}

/// Moraal naar 20 (top) en de redenen om ontevreden te zijn weg, zoals FMRTE's "Inspire".
pub fn inspire(uids: &Option<Vec<u32>>, morale: u8, clear_unhappiness: bool) -> Result<usize, String> {
    let l = layout();
    let morale = morale.clamp(1, 20);
    let mut patches = Vec::new();
    let mut n = 0;
    for p in select(uids)? {
        let mut touched = false;
        let cur = mem::u8_at(p.addr + l.p_morale).ok_or("moraal niet leesbaar")?;
        if !(1..=20).contains(&cur) {
            continue; // past niet bij wat we verwachten: niet aankomen
        }
        if cur != morale {
            patches.push(Patch { addr: p.addr + l.p_morale, old: vec![cur], new: vec![morale] });
            touched = true;
        }
        if clear_unhappiness {
            if let Some(ct) = player_contract(p.addr, p.shift) {
                let reasons = mem::u64_at(ct + CONTRACT_UNHAPPINESS).unwrap_or(0);
                if reasons != 0 {
                    patches.push(Patch { addr: ct + CONTRACT_UNHAPPINESS, old: reasons.to_le_bytes().to_vec(), new: 0u64.to_le_bytes().to_vec() });
                    touched = true;
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

// ---- spelregels van de manager (vlaggen van FM's in-game editor) --------------------------
//
// FM's in-game editor kent per manager "Allow squad registration anytime" en "Allow squad numbers
// anytime": bits in een u32 op manager+0x3FC (Windows; gevonden op 8 nov 2026 door de editor-actie
// TEAM_CONTAINER_ALLOW_SQUAD_REGISTRATION_ANYTIME_ACTION te disassembleren: die zet of wist het bit
// op het object dat de HUMAN-record teruggeeft, en dat is het managerobject zelf). Het subobject op
// manager+0x440 is het HUMAN_NON_PLAYER-deel met de vtable die de editor gebruikt; dat is de
// controle vóór het schrijven. Of een gezet bit "altijd toestaan" betekent, is afgeleid uit de
// actie (standaard staat het bit uit); de gebruiker ziet het in FM's registratiescherm.

pub const FLAG_REGISTRATION_ANYTIME: u32 = 0x100;
pub const FLAG_NUMBERS_ANYTIME: u32 = 0x200;
const HUMAN_VTABLE_SUB: u64 = 0x440;

#[derive(Serialize)]
pub struct HumanFlags {
    pub supported: bool,
    pub flags: u32,
    pub registration_anytime: bool,
    pub numbers_anytime: bool,
}

fn human_flags_addr() -> Result<u64, String> {
    let off = layout().human_flags.ok_or("op dit platform nog niet bekend / not known on this platform yet")?;
    let c = chain()?;
    let sub = rtti::class_name(c.human + HUMAN_VTABLE_SUB);
    if !sub.as_deref().is_some_and(|s| s.contains("HUMAN_NON_PLAYER")) {
        return Err("managerobject ziet er anders uit dan verwacht / manager object looks different than expected".into());
    }
    Ok(c.human + off)
}

pub fn human_flags() -> Result<HumanFlags, String> {
    let Ok(a) = human_flags_addr() else {
        chain()?;
        return Ok(HumanFlags { supported: false, flags: 0, registration_anytime: false, numbers_anytime: false });
    };
    let f = mem::u32_at(a).ok_or("vlaggen niet leesbaar / flags not readable")?;
    Ok(HumanFlags { supported: true, flags: f, registration_anytime: f & FLAG_REGISTRATION_ANYTIME != 0, numbers_anytime: f & FLAG_NUMBERS_ANYTIME != 0 })
}

/// Zet de editorvlaggen van de manager (alleen wat meegegeven is), met undo.
pub fn set_human_flags(registration: Option<bool>, numbers: Option<bool>) -> Result<HumanFlags, String> {
    let a = human_flags_addr()?;
    let cur = mem::u32_at(a).ok_or("vlaggen niet leesbaar / flags not readable")?;
    let mut new = cur;
    for (on, bit) in [(registration, FLAG_REGISTRATION_ANYTIME), (numbers, FLAG_NUMBERS_ANYTIME)] {
        match on {
            Some(true) => new |= bit,
            Some(false) => new &= !bit,
            None => {}
        }
    }
    if new != cur {
        apply("spelregels / rules", vec![Patch { addr: a, old: cur.to_le_bytes().to_vec(), new: new.to_le_bytes().to_vec() }])?;
    }
    human_flags()
}

// ---- training -----------------------------------------------------------------
//
// TRAINING_MANAGER houdt per team een blok bij waarin de team-pointer staat en, 0x28 verder,
// een lijst (begin, end, cap) met pointers naar weekrecords van 0x60 bytes:
//   +0x00 pointer naar de naam van het schema (FM-string)
//   +0x0C u16 dag-van-het-jaar (lage 9 bits) + u16 jaar  (maandag van de week)
//   +0x10 7 dagblokken van 8 bytes: u16 0, u16 sessie 1, u16 sessie 2, u16 extra
// Gemeten op FM27 macOS (Steam-build 26340721) en gecontroleerd tegen de kalender in de save; op
// Windows (8 nov 2026) live gemeten: dezelfde afstand 0x28. Voor het geval een build afwijkt, zoekt
// de app op Windows ook 0x08–0x80 af, met een strenge controle van de weeklijst (geldige datums,
// precies een week uit elkaar, geldige sessiecodes).

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
    pub(crate) fn read(addr: u64) -> Option<FmDate> {
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

/// Een datum die het team bijhoudt: dicht bij de spel-datum tijdens het seizoen, maar niet gelijk
/// (alleen als reserve).
fn date_via_team(team: u64) -> Option<FmDate> {
    let s = mem::u64_at(team + TEAM_SCHEDULE).filter(|p| mem::is_ptr(*p))?;
    FmDate::read(s + SCHEDULE_DATE)
}

/// De spel-datum op zijn vaste plek in game_plugin (per build, FMLibreEditor).
fn date_via_static() -> Option<FmDate> {
    let img = mem::game_plugin()?;
    let b = profile(img.id.as_deref())?;
    FmDate::read(img.start + b.current_date).filter(|d| d.year >= 2000)
}

pub fn game_date() -> Result<FmDate, String> {
    let c = chain()?;
    // De vaste plek van de build eerst: de datum bij het team is een andere (rond de laatste
    // wedstrijd), wat buiten het seizoen weken kan schelen (gezien op 28 juli: team 23 mei).
    let d = date_via_static().or_else(|| date_via_team(c.team));
    d.ok_or_else(|| "spel-datum niet leesbaar / game date not readable".into())
}

/// Een sessiecode zoals FM27 ze kent (0–7, 100–108, 200–228, 300–307, 400–402, 500).
fn known_code(c: u16) -> bool {
    c <= 600
}

/// Een weekrecord: een geldige maandag en sessiecodes in de dagblokken. Het eerste woord van een
/// dagblok is 0 in alle saves die gemeten zijn; het wordt nooit geschreven en hier alleen als klein
/// getal verwacht (een vlag of een intensiteit zou passen). Een enkele onbekende code (een leeg
/// vak, een nieuw soort dag) keurt de week niet af: `set_week` laat zo'n dag toch ongemoeid.
fn week_ok(rec: u64) -> bool {
    if !mem::is_ptr(rec) || FmDate::read(rec + WEEK_DATE).is_none() {
        return false;
    }
    let Some(b) = mem::read_bytes(rec + WEEK_DAYS, 56) else { return false };
    let words: Vec<u16> = b.chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect();
    if words.chunks_exact(4).any(|d| d[0] > 0xFF) {
        return false;
    }
    let codes: Vec<u16> = words.chunks_exact(4).flat_map(|d| d[1..].to_vec()).collect();
    let known = codes.iter().filter(|c| known_code(**c)).count();
    // een echte week heeft altijd rust/herstel, wedstrijden of trainingen (codes boven 0)
    let nonzero = codes.iter().filter(|c| known_code(**c) && **c != 0).count();
    known * 3 >= codes.len() * 2 && nonzero >= 5
}

/// Lengte van de weeklijst: FM houdt de weken van het seizoen bij (en mogelijk de geschiedenis).
const MAX_WEEKS: usize = 2_000;

fn weeks_header_ok(hdr: u64) -> Option<Vec<u64>> {
    let (_, list) = pointer_list(hdr, MAX_WEEKS)?;
    if list.is_empty() || !week_ok(list[0]) || !week_ok(*list.last()?) {
        return None;
    }
    // de maandagen lopen op (eerste, tweede en laatste week)
    let first = FmDate::read(list[0] + WEEK_DATE)?;
    let last = FmDate::read(*list.last()? + WEEK_DATE)?;
    if list.len() >= 2 {
        let second = FmDate::read(list[1] + WEEK_DATE)?;
        if !(first < second && second <= last) {
            return None;
        }
    }
    Some(list)
}

/// Strenger, voor een afstand die nog niet gemeten is: elke week geldig, met oplopende maandagen.
fn weeks_header_strict(hdr: u64) -> Option<Vec<u64>> {
    let list = weeks_header_ok(hdr)?;
    if list.len() < 2 || !list.iter().all(|r| week_ok(*r)) {
        return None;
    }
    let dates: Vec<FmDate> = list.iter().filter_map(|r| FmDate::read(r + WEEK_DATE)).collect();
    (dates.len() == list.len() && dates.windows(2).all(|w| w[0] < w[1])).then_some(list)
}

fn weeks_at(hdr: u64, delta: u64) -> Option<Vec<u64>> {
    if delta == TB_WEEKS { weeks_header_ok(hdr) } else { weeks_header_strict(hdr) }
}

/// Afstanden van de team-pointer tot de weeklijst die geprobeerd worden: eerst de gemeten 0x28.
fn tb_candidates() -> Vec<u64> {
    let mut v = vec![TB_WEEKS];
    if mem::abi() == Abi::Msvc {
        v.extend((0x08..=0x80).step_by(8).filter(|d| *d != TB_WEEKS));
    }
    v
}

/// Lijkt `hdr` op een weeklijst (lijst van pointers naar records met een geldige maandag), maar
/// keurt de controle hem af? Dan waarom, kort, voor in de foutmelding: zo kan een gebruiker
/// melden wat er in zijn save anders is.
fn weeks_rejection(hdr: u64) -> Option<String> {
    let begin = mem::u64_at(hdr)?;
    let end = mem::u64_at(hdr + 8)?;
    if !mem::is_ptr(begin) || end <= begin || (end - begin) % 8 != 0 || (end - begin) / 8 > 100_000 {
        return None;
    }
    let n = (end - begin) / 8;
    let first = mem::u64_at(begin).filter(|r| mem::is_ptr(*r))?;
    FmDate::read(first + WEEK_DATE)?;
    if n as usize > MAX_WEEKS {
        return Some(format!("{n} weken"));
    }
    let last = mem::u64_at(end - 8)?;
    let bad = [first, last].into_iter().find(|r| !week_ok(*r))?;
    let date = FmDate::read(bad + WEEK_DATE).map(|d| format!("{}/{}", d.year, d.doy)).unwrap_or_else(|| "?".into());
    let days: Vec<String> = mem::read_bytes(bad + WEEK_DAYS, 56)
        .map(|b| b.chunks_exact(8).map(|d| format!("{:?}", d.chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect::<Vec<_>>())).collect())
        .unwrap_or_default();
    Some(format!("{n} weken, week {date}: {}", days.join(" ")))
}

/// De weeklijst van het team van de manager (adres van de lijstkop).
///
/// Elke plek in de heap die naar het team wijst, is een kandidaat; het trainingsblok heeft de
/// weeklijst 0x28 verder. Er wordt niet meer bij een vast aantal treffers gestopt: bij een grote
/// club (veel wedstrijden, historie, nieuws) wijzen tienduizenden plekken naar het team, en het
/// trainingsblok kan daar ver achter liggen.
fn training_weeks_header(team: u64) -> Result<u64, String> {
    if let Some((b, d)) = with_state(|s| s.training_block) {
        if mem::u64_at(b) == Some(team) && weeks_at(b + d, d).is_some() {
            return Ok(b + d);
        }
    }
    let set: std::collections::HashSet<u64> = [team].into_iter().collect();
    let others: Vec<u64> = tb_candidates().into_iter().filter(|d| *d != TB_WEEKS).collect();
    let (mut hits, mut found, mut fallback, mut rejected) = (0usize, None, None, Vec::<String>::new());
    mem::scan_u64_each(&set, &mut |a, _| {
        hits += 1;
        // snel voorfilteren: één leesactie voor alle afstanden, alleen een echte lijstkop verder bekijken
        let Some(buf) = mem::read_bytes(a, 0x90) else { return false };
        let word = |o: u64| u64::from_le_bytes(buf[o as usize..o as usize + 8].try_into().unwrap());
        let list_like = |d: u64| {
            let (b, e) = (word(d), word(d + 8));
            mem::is_ptr(b) && e > b && (e - b) % 8 == 0
        };
        if !list_like(TB_WEEKS) && others.iter().all(|d| !list_like(*d)) {
            return false;
        }
        if list_like(TB_WEEKS) && weeks_at(a + TB_WEEKS, TB_WEEKS).is_some() {
            found = Some((a, TB_WEEKS));
            return true;
        }
        if rejected.len() < 2 && list_like(TB_WEEKS) {
            if let Some(why) = weeks_rejection(a + TB_WEEKS) {
                rejected.push(why);
            }
        }
        // een andere afstand alleen als reserve: de gemeten 0x28 verderop gaat voor
        if fallback.is_none() {
            fallback = others.iter().find(|d| list_like(**d) && weeks_at(a + **d, **d).is_some()).map(|d| (a, *d));
        }
        false
    });
    if let Some((a, d)) = found.or(fallback) {
        with_state(|s| s.training_block = Some((a, d)));
        return Ok(a + d);
    }
    let detail = if rejected.is_empty() { String::new() } else { format!("; afgekeurd / rejected: {}", rejected.join(" | ")) };
    Err(format!(
        "trainingskalender van het team niet gevonden / training calendar not found (team ×{hits}{detail})"
    ))
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
    let hdr = training_weeks_header(c.team)?;
    let list = weeks_header_ok(hdr).ok_or("weeklijst niet leesbaar")?;
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
    let hdr = training_weeks_header(c.team)?;
    let list = weeks_header_ok(hdr).ok_or("weeklijst niet leesbaar")?;
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
        if old.iter().any(|x| !known_code(*x)) {
            continue; // iets wat de app niet kent: niet aankomen
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
