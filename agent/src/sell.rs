//! Een speler direct verkopen aan een andere club, met transfersom.
//!
//! Wat er bij een verplaatsing moet veranderen, heeft FMLibreEditor uitgezocht en getest
//! (docs/recon/fm27-transfers.md, op de Mac en op Windows, ook op Steam-build 26340721):
//!
//! - elk team houdt een lijst (begin, end, cap) van zijn spelers; de speler gaat uit elke lijst
//!   van zijn oude club en in die van het eerste elftal van de koper;
//! - de speler wijst naar het team waarvoor hij speelt, zijn contract naar de werkgever: beide
//!   worden het eerste elftal van de koper, met de datum van vandaag, zonder rugnummer en van de
//!   transfer- en huurlijst af;
//! - een aanvoerderschap dat hij had, blijft leeg voor de club om in te vullen;
//! - de opstellingen van een club met een menselijke manager (`simatch::PREPARED_TACTICS_CREATOR_ARRAY`,
//!   26 plekken met de interne id's van personen) houden hem niet vast: zijn plek wordt leeg, zoals
//!   de game zelf doet als een speler vertrekt;
//! - een lijst die vol is, mag groeien tot het einde van het blok dat de allocator van de game
//!   ervoor heeft uitgegeven (de blokgroottes staan in de layout); de bytes daar zijn dan nul.
//!
//! De transfersom gaat van het saldo (en transferbudget) van de koper naar het saldo én het
//! transferbudget van de verkoper. De game zelf zou de transfer ook in de historie en het nieuws
//! zetten; dat doet de app (nog) niet. Een echt bod van een club aanmaken kan (nog) niet: die objecten zijn niet ontleed.

use serde::Serialize;
use std::collections::HashSet;
use std::sync::Mutex;

use crate::game::{self, CLUB_FINANCE, CLUB_UID, Change, FIN_BALANCE, FIN_TRANSFER, FmDate, Patch, TEAM_CLUB, TEAM_PLAYERS};
use crate::mem;
use crate::rtti;

const CLUB_TEAMS: u64 = 0x18;
const CLUB_WOMEN: u64 = 0xB9;
const TEAM_CAPTAIN: u64 = 0x88;
const TEAM_VICE_CAPTAIN: u64 = 0x90;

const C_TEAM: u64 = game::CONTRACT_TEAM;
const C_SENIOR_TEAM: u64 = 0x18;
const C_WAGE: u64 = 0x20;
const C_JOB: u64 = 0x26;
const C_SIGNED: u64 = 0x44;
const C_JOINED: u64 = 0x4C;
const C_LISTING: u64 = 0x57;
const C_SQUAD_NUMBER: u64 = 0x5D;
const PLAYER_JOB: u8 = 1;
const NO_SQUAD_NUMBER: u8 = 255;

const SEL_LIST: u64 = 0x08;
const SEL_SLOTS: u64 = 0x28;
const SEL_COUNT: usize = 26;
const EMPTY_SLOT: u32 = u32::MAX;

pub const MAX_FEE: i64 = 2_000_000_000;
const LONGEST_LIST: u64 = 2_000;

// ---- cache ----------------------------------------------------------------------------------

#[derive(Default)]
struct Cache {
    clubs: Option<Vec<ClubEntry>>,
    selection_vtables: Option<Vec<u64>>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Cache::default))
}

/// Na het opnieuw verbinden (of in tests): alles vergeten.
pub fn reset() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// ---- lijsten --------------------------------------------------------------------------------

/// Een (begin, end, cap)-lijst met pointers, zoals hij nu in het geheugen staat.
#[derive(Clone, Copy, Debug)]
struct List {
    header: u64,
    begin: u64,
    end: u64,
    cap: u64,
}

impl List {
    fn read(header: u64) -> Option<List> {
        let b = mem::read_bytes(header, 24)?;
        let w = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        let (begin, end, cap) = (w(0), w(1), w(2));
        let sane = mem::is_ptr(begin)
            && begin <= end
            && end <= cap
            && (end - begin) % 8 == 0
            && (cap - begin) % 8 == 0
            && cap - begin <= 8 * LONGEST_LIST;
        sane.then_some(List { header, begin, end, cap })
    }

    fn elements(&self) -> Option<Vec<u64>> {
        if self.end == self.begin {
            return Some(Vec::new());
        }
        let b = mem::read_bytes(self.begin, (self.end - self.begin) as usize)?;
        Some(b.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect())
    }

    /// Hoeveel pointers het blok kan houden: de capaciteit, of het hele blok van de allocator als
    /// dat bekend is en er voorbij de capaciteit nog niets staat.
    fn room(&self, blocks: &[u64]) -> u64 {
        let cap = (self.cap - self.begin) / 8;
        let Some(block) = blocks.iter().copied().find(|b| *b >= self.cap - self.begin) else {
            return cap;
        };
        let spare = self.begin + block - self.cap;
        if spare == 0 {
            return cap;
        }
        match mem::read_bytes(self.cap, spare as usize) {
            Some(b) if b.iter().all(|x| *x == 0) => block / 8,
            _ => cap,
        }
    }
}

fn words(v: &[u64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn push(patches: &mut Vec<Patch>, addr: u64, old: Vec<u8>, new: Vec<u8>) {
    if old != new {
        patches.push(Patch { addr, old, new });
    }
}

/// De speler op `index` uit de lijst: wie na hem komt schuift een plek op, de lijst eindigt een
/// plek eerder en de vrijgekomen plek wordt nul. Onderweg staat de laatste speler even dubbel,
/// maar er staat nooit een lege plek in de lijst.
fn removal(list: &List, index: usize, elems: &[u64], patches: &mut Vec<Patch>) {
    let last = elems.len() - 1;
    push(patches, list.begin + index as u64 * 8, words(&elems[index..last]), words(&elems[index + 1..]));
    push(patches, list.header + 8, list.end.to_le_bytes().to_vec(), (list.end - 8).to_le_bytes().to_vec());
    push(patches, list.begin + last as u64 * 8, elems[last].to_le_bytes().to_vec(), vec![0; 8]);
}

/// De speler achteraan in de lijst (die ruimte moet hebben).
fn append(list: &List, player: u64, patches: &mut Vec<Patch>) -> Option<()> {
    let slot = mem::read_bytes(list.end, 8)?;
    push(patches, list.end, slot, player.to_le_bytes().to_vec());
    let end = list.end + 8;
    push(patches, list.header + 8, words(&[list.end, list.cap]), words(&[end, list.cap.max(end)]));
    Some(())
}

// ---- clubs ----------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct ClubEntry {
    pub uid: u32,
    pub name: String,
    pub women: bool,
    #[serde(skip)]
    pub addr: u64,
}

/// Alle clubs van de game, uit de clublijst op zijn vaste plek (per build).
fn all_clubs() -> Result<Vec<ClubEntry>, String> {
    if let Some(c) = with_cache(|c| c.clubs.clone()) {
        return Ok(c);
    }
    let img = mem::game_plugin().ok_or("game_plugin niet geladen / not loaded")?;
    let b = game::profile(img.id.as_deref()).ok_or("onbekende FM-versie: de clublijst is niet bekend / unknown FM build: club list unknown")?;
    let pair = img.start + b.clubs;
    let (begin, end) = (mem::u64_at(pair).ok_or("clublijst niet leesbaar")?, mem::u64_at(pair + 8).ok_or("clublijst niet leesbaar")?);
    if !mem::is_ptr(begin) || end < begin || (end - begin) % 8 != 0 || (end - begin) / 8 > 400_000 {
        return Err("clublijst ziet er niet uit zoals verwacht / club list looks wrong".into());
    }
    let bytes = mem::read_bytes(begin, (end - begin) as usize).ok_or("clublijst niet leesbaar")?;
    let mut out = Vec::new();
    for c in bytes.chunks_exact(8) {
        let addr = u64::from_le_bytes(c.try_into().unwrap());
        if !mem::is_ptr(addr) {
            continue;
        }
        let (Some(uid), Some(name)) = (mem::u32_at(addr + CLUB_UID), game::club_name(addr)) else { continue };
        let women = mem::u8_at(addr + CLUB_WOMEN) == Some(1);
        out.push(ClubEntry { uid, name, women, addr });
    }
    if out.is_empty() {
        return Err("geen clubs gevonden / no clubs found".into());
    }
    with_cache(|c| c.clubs = Some(out.clone()));
    Ok(out)
}

/// Kleine letters, zonder accenten: "Málaga" en "malaga" zijn hetzelfde.
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ı' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ő' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ű' | 'ů' => 'u',
            'ý' | 'ÿ' => 'y',
            'ś' | 'š' | 'ş' => 's',
            'ź' | 'ż' | 'ž' => 'z',
            'ğ' => 'g',
            'ł' => 'l',
            'ř' => 'r',
            'ţ' | 'ť' => 't',
            other => other,
        })
        .collect()
}

/// Clubs waarvan de naam op `query` lijkt: eerst precies, dan beginnend met, dan een los woord,
/// dan ergens in de naam.
pub fn find_clubs(query: &str, limit: usize) -> Result<Vec<ClubEntry>, String> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits: Vec<(u8, usize, ClubEntry)> = all_clubs()?
        .into_iter()
        .filter_map(|c| {
            let n = fold(&c.name);
            let rank = if n == q {
                0
            } else if n.starts_with(&q) {
                1
            } else if n.split(|ch: char| !ch.is_alphanumeric()).any(|w| w == q) {
                2
            } else if n.contains(&q) {
                3
            } else {
                return None;
            };
            Some((rank, c.name.len(), c))
        })
        .collect();
    hits.sort_by(|a, b| (a.0, a.1, &a.2.name).cmp(&(b.0, b.1, &b.2.name)));
    Ok(hits.into_iter().take(limit).map(|(_, _, c)| c).collect())
}

fn club_by_uid(uid: u32) -> Result<ClubEntry, String> {
    all_clubs()?.into_iter().find(|c| c.uid == uid).ok_or_else(|| format!("club {uid} niet gevonden / club not found"))
}

// ---- opstellingen ---------------------------------------------------------------------------

fn selection_vtables() -> Result<Vec<u64>, String> {
    if let Some(v) = with_cache(|c| c.selection_vtables.clone()) {
        return Ok(v);
    }
    let img = mem::game_plugin().ok_or("game_plugin niet geladen")?;
    let v = rtti::vtables_of(&img, &rtti::decorated("simatch", "PREPARED_TACTICS_CREATOR_ARRAY"));
    if !v.is_empty() {
        with_cache(|c| c.selection_vtables = Some(v.clone()));
    }
    Ok(v)
}

/// De interne id van een persoon: 8 bytes in zijn identiteitsblok.
fn person_id(player: u64, shift: u64) -> Option<u32> {
    mem::u32_at(player + game::layout().p_identity + shift + 8)
}

/// De opstellingen van de club met de plek van de vertrekkende speler leeg. Een kopie telt als
/// van de club als de meeste gevulde plekken spelers van de club zijn (een selectie van een
/// nationaal team niet). Objecten zonder geldige lijst (vrijgegeven blokken, kopieën op een stack)
/// blijven buiten schot.
fn selection_patches(person: u32, club_ids: &HashSet<u32>) -> Result<Vec<Patch>, String> {
    let vts = selection_vtables()?;
    if vts.is_empty() {
        return Err("opstellingen van je club niet gevonden: verkopen kan niet veilig / your club's selections were not found: cannot sell safely".into());
    }
    let mut out = Vec::new();
    for obj in rtti::instances(&vts, 4_000) {
        if List::read(obj + SEL_LIST).is_none() {
            continue;
        }
        let Some(before) = mem::read_bytes(obj + SEL_SLOTS, SEL_COUNT * 4) else { continue };
        let slots: Vec<u32> = before.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        if !slots.contains(&person) {
            continue;
        }
        let filled = slots.iter().filter(|s| **s != EMPTY_SLOT).count();
        let club = slots.iter().filter(|s| club_ids.contains(s)).count();
        if 2 * club <= filled {
            continue;
        }
        let after: Vec<u8> = slots.iter().flat_map(|s| if *s == person { EMPTY_SLOT } else { *s }.to_le_bytes()).collect();
        push(&mut out, obj + SEL_SLOTS, before, after);
    }
    Ok(out)
}

// ---- verkopen en kopen ----------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Sale {
    pub player: String,
    pub uid: u32,
    pub from: String,
    pub to: String,
    /// de andere club (koper bij verkopen, verkoper bij kopen)
    pub to_uid: u32,
    /// kopen (naar jouw club) in plaats van verkopen
    pub purchase: bool,
    /// pond
    pub fee: i64,
    /// nieuw salaris in pond per week, als dat verandert
    pub wage: Option<i64>,
    /// opstellingen waarin zijn plek leeg wordt
    pub selections: usize,
    pub was_captain: bool,
    /// de andere club betaalt of ontvangt mee (alleen clubs met volledige financiën)
    pub buyer_pays: bool,
    /// jouw saldo en transferbudget na afloop (pond)
    pub balance_after: i64,
    pub transfer_budget_after: i64,
    /// mag er op deze FM-versie geschreven worden (anders eerst toestaan in Instellingen)
    pub allowed: bool,
    pub done: bool,
}

fn date_patch(contract: u64, off: u64, today: FmDate, patches: &mut Vec<Patch>) {
    // een datum die de game niet heeft gezet, blijft leeg
    let Some(old) = mem::read_bytes(contract + off, 4) else { return };
    let Some(cur) = FmDate::read(contract + off) else { return };
    if cur.year < 1950 {
        return;
    }
    let raw = u16::from_le_bytes([old[0], old[1]]);
    let mut new = ((raw & !0x1FF) | today.doy).to_le_bytes().to_vec();
    new.extend_from_slice(&today.year.to_le_bytes());
    push(patches, contract + off, old, new);
}

/// Het eerste elftal van een club: het eerste team in zijn lijst, dat naar de club terugwijst.
fn first_team(club: &ClubEntry) -> Result<u64, String> {
    List::read(club.addr + CLUB_TEAMS)
        .and_then(|l| l.elements())
        .and_then(|t| t.first().copied())
        .filter(|s| mem::is_ptr(*s) && mem::u64_at(*s + TEAM_CLUB) == Some(club.addr))
        .ok_or_else(|| format!("{} heeft geen eerste elftal / has no senior team", club.name))
}

/// De selectie (eerste elftal) van een club, duurste eerst: om een speler te kiezen om te kopen.
pub fn club_squad(uid: u32) -> Result<Vec<game::PlayerInfo>, String> {
    let club = club_by_uid(uid)?;
    let mut players = game::squad_of(first_team(&club)?)?;
    players.sort_by(|a, b| b.value.cmp(&a.value).then(a.name.cmp(&b.name)));
    Ok(players)
}

/// Verkoopt speler `uid` van je eigen club aan club `to_uid` voor `fee` pond (met `wage` pond per
/// week als nieuw salaris, anders blijft het). Met `dry_run` wordt alleen gecontroleerd.
pub fn sell(uid: u32, to_uid: u32, fee: i64, wage: Option<i64>, dry_run: bool) -> Result<Sale, String> {
    relocate(false, uid, to_uid, fee, wage, dry_run)
}

/// Koopt speler `uid` van club `from_uid` voor `fee` pond: hij komt in je eerste elftal, jij betaalt.
pub fn buy(uid: u32, from_uid: u32, fee: i64, wage: Option<i64>, dry_run: bool) -> Result<Sale, String> {
    relocate(true, uid, from_uid, fee, wage, dry_run)
}

/// Verhuist een speler tussen jouw club en een andere club. Bij verkopen gaat hij van jou naar
/// `other`, bij kopen (`purchase`) van `other` naar jouw eerste elftal.
fn relocate(purchase: bool, uid: u32, other_uid: u32, fee: i64, wage: Option<i64>, dry_run: bool) -> Result<Sale, String> {
    if !(0..=MAX_FEE).contains(&fee) {
        return Err("transfersom tussen 0 en 2 miljard pond / fee must be 0 to 2 billion pounds".into());
    }
    if let Some(w) = wage {
        if !(0..=i32::MAX as i64).contains(&w) {
            return Err("salaris klopt niet / wage out of range".into());
        }
    }
    let l = game::layout();
    let chain = game::chain()?;
    let our = chain.club;
    let our_name = game::club_name(our).unwrap_or_else(|| "?".into());

    // de andere club: niet de eigen, zelfde soort (mannen/vrouwen)
    let other = club_by_uid(other_uid)?;
    if other.addr == our {
        return Err("dat is je eigen club / that is your own club".into());
    }
    if mem::u8_at(our + CLUB_WOMEN) != mem::u8_at(other.addr + CLUB_WOMEN) {
        return Err("mannen- en vrouwenclubs kunnen niet ruilen / men's and women's clubs don't mix".into());
    }
    let (from_club, from_name, to_club, to_name) = if purchase { (other.addr, other.name.clone(), our, our_name.clone()) } else { (our, our_name.clone(), other.addr, other.name.clone()) };

    // alle teams van de club die hij verlaat
    let teams: Vec<u64> = {
        let mut t = List::read(from_club + CLUB_TEAMS).and_then(|l| l.elements()).ok_or_else(|| format!("teams van {from_name} niet leesbaar / teams not readable"))?;
        t.sort_unstable();
        t.dedup();
        t
    };

    // de speler: in je eigen selectie, of ergens bij de andere club
    let p = if purchase {
        let mut found = None;
        for &t in &teams {
            if let Some(p) = game::squad_of(t).ok().and_then(|s| s.into_iter().find(|p| p.uid == uid)) {
                found = Some(p);
                break;
            }
        }
        found.ok_or_else(|| format!("speler niet gevonden bij {from_name} / player not found at that club"))?
    } else {
        game::squad()?.into_iter().find(|p| p.uid == uid).ok_or("speler niet in je selectie / player not in your squad")?
    };
    if p.player_coach {
        return Err(format!("{} is speler-trainer; die kan de app (nog) niet verplaatsen / player-coaches cannot be moved yet", p.name));
    }
    let obj = p.addr;

    // het contract moet van hem zijn, van een speler, met één werkgever: de club die hij verlaat
    let contract = game::player_contract(obj, 0).ok_or("contract niet gevonden / contract not found")?;
    let team = mem::u64_at(obj + l.p_team).filter(|t| mem::is_ptr(*t)).ok_or("team niet gevonden")?;
    let employer = mem::u64_at(contract + C_TEAM).filter(|t| mem::is_ptr(*t)).ok_or("werkgever niet gevonden")?;
    if mem::u64_at(contract + C_SENIOR_TEAM) != Some(employer) || mem::u8_at(contract + C_JOB) != Some(PLAYER_JOB) {
        return Err(format!("{} heeft een bijzonder contract; niet verplaatst / unusual contract, not moved", p.name));
    }
    if mem::u64_at(team + TEAM_CLUB) != Some(from_club) || mem::u64_at(employer + TEAM_CLUB) != Some(from_club) {
        return Err(format!("{} is verhuurd of speelt bij een andere club / is on loan or plays elsewhere", p.name));
    }
    if !teams.contains(&team) || !teams.contains(&employer) {
        return Err(format!("speler staat niet in de lijsten van {from_name} / player not listed with that club"));
    }
    let mut leaving = Vec::new();
    for &t in &teams {
        let Some(list) = List::read(t + TEAM_PLAYERS) else { continue };
        let elems = list.elements().ok_or("teamlijst niet leesbaar")?;
        let found: Vec<usize> = (0..elems.len()).filter(|i| elems[*i] == obj).collect();
        match found[..] {
            [] => {}
            [i] => leaving.push((t, list, i, elems)),
            _ => return Err("speler staat dubbel in een teamlijst / listed twice".into()),
        }
    }
    if !leaving.iter().any(|(t, ..)| *t == team) {
        return Err("speler staat niet in de lijst van zijn team / not in his team's list".into());
    }

    // waar hij heen gaat: het eerste elftal van de ontvangende club, met ruimte in de lijst
    let senior = if purchase { first_team(&ClubEntry { uid: 0, name: our_name.clone(), women: false, addr: our })? } else { first_team(&other)? };
    let target = List::read(senior + TEAM_PLAYERS).ok_or_else(|| format!("selectie van {to_name} niet leesbaar"))?;
    let target_elems = target.elements().ok_or("selectie van de ontvangende club niet leesbaar")?;
    if target_elems.contains(&obj) {
        return Err("speler staat al bij die club / already at that club".into());
    }
    if target.room(l.heap_blocks) <= target_elems.len() as u64 {
        return Err(format!("de selectie van {to_name} zit vol; de app kan hem (nog) niet laten groeien / {to_name}'s squad list is full"));
    }

    // de opstellingen van de club die hij verlaat, als dat de jouwe is (jij bent de menselijke
    // manager): zijn plek wordt leeg
    let mut soft = Vec::new();
    if !purchase {
        let person = person_id(obj, 0).ok_or("id van de speler niet leesbaar")?;
        let mut club_ids = HashSet::new();
        for &t in &teams {
            let Some(elems) = List::read(t + TEAM_PLAYERS).and_then(|l| l.elements()) else { continue };
            for e in elems {
                for shift in [0, l.player_coach_shift] {
                    if let Some(id) = person_id(e, shift) {
                        club_ids.insert(id);
                    }
                }
            }
        }
        soft = selection_patches(person, &club_ids)?;
    }

    let today = game::game_date()?;
    let mut patches = Vec::new();
    for (_, list, i, elems) in &leaving {
        removal(list, *i, elems, &mut patches);
    }
    // aanvoerderschappen die hij had, blijven leeg
    let identity = obj + l.p_identity;
    let mut was_captain = false;
    for &t in &teams {
        for off in [TEAM_CAPTAIN, TEAM_VICE_CAPTAIN] {
            if mem::u64_at(t + off) == Some(identity) {
                was_captain = true;
                push(&mut patches, t + off, identity.to_le_bytes().to_vec(), vec![0; 8]);
            }
        }
    }
    append(&target, obj, &mut patches).ok_or("selectie van de ontvangende club niet beschrijfbaar")?;
    // contract: nieuwe werkgever vanaf vandaag, zonder rugnummer, van de lijsten af
    for off in [C_TEAM, C_SENIOR_TEAM] {
        push(&mut patches, contract + off, employer.to_le_bytes().to_vec(), senior.to_le_bytes().to_vec());
    }
    date_patch(contract, C_SIGNED, today, &mut patches);
    date_patch(contract, C_JOINED, today, &mut patches);
    if let Some(w) = wage {
        let old = mem::read_bytes(contract + C_WAGE, 4).ok_or("salaris niet leesbaar")?;
        push(&mut patches, contract + C_WAGE, old, (w as i32).to_le_bytes().to_vec());
    }
    let number = mem::u8_at(contract + C_SQUAD_NUMBER).ok_or("rugnummer niet leesbaar")?;
    push(&mut patches, contract + C_SQUAD_NUMBER, vec![number], vec![NO_SQUAD_NUMBER]);
    let listing = mem::u8_at(contract + C_LISTING).ok_or("transferlijst niet leesbaar")?;
    push(&mut patches, contract + C_LISTING, vec![listing], vec![listing & !0b11]);
    push(&mut patches, obj + l.p_team, team.to_le_bytes().to_vec(), senior.to_le_bytes().to_vec());

    // geld. Verkopen: de koper betaalt (saldo en transferbudget, niet onder nul), jij ontvangt de
    // som op je saldo én in je transferbudget (FM zelf maakt het deel vrij dat het bestuur
    // toestaat; de app alles, bijstellen kan op de clubpagina). Kopen: andersom.
    let balance = mem::i32_at(chain.finance + FIN_BALANCE).unwrap_or(0) as i64;
    let transfer_budget = mem::i32_at(chain.finance + FIN_TRANSFER).unwrap_or(0).max(0) as i64;
    let mut deltas = Vec::new();
    let (balance_after, transfer_budget_after) = if purchase {
        let left = (transfer_budget - fee).max(0);
        deltas.push((chain.finance + FIN_BALANCE, -fee));
        if left != transfer_budget {
            deltas.push((chain.finance + FIN_TRANSFER, left - transfer_budget));
        }
        (balance - fee, left)
    } else {
        if balance + fee > i32::MAX as i64 || transfer_budget + fee > i32::MAX as i64 {
            return Err("je saldo of transferbudget zou te groot worden / balance or transfer budget would overflow".into());
        }
        deltas.push((chain.finance + FIN_BALANCE, fee));
        deltas.push((chain.finance + FIN_TRANSFER, fee));
        (balance + fee, transfer_budget + fee)
    };
    let mut buyer_pays = false;
    if let Some(f) = mem::u64_at(other.addr + CLUB_FINANCE).filter(|f| mem::is_ptr(*f) && game::finance_looks_real(*f)) {
        buyer_pays = true;
        if purchase {
            deltas.push((f + FIN_BALANCE, fee));
            deltas.push((f + FIN_TRANSFER, fee));
        } else {
            deltas.push((f + FIN_BALANCE, -fee));
            if let Some(tb) = mem::i32_at(f + FIN_TRANSFER).filter(|tb| *tb >= 0) {
                let left = (tb as i64 - fee).max(0);
                if left != tb as i64 {
                    deltas.push((f + FIN_TRANSFER, left - tb as i64));
                }
            }
        }
    }

    let mut sale = Sale {
        player: p.name.clone(),
        uid,
        from: from_name,
        to: to_name.clone(),
        to_uid: other_uid,
        purchase,
        fee,
        wage,
        selections: soft.len(),
        was_captain,
        buyer_pays,
        balance_after,
        transfer_budget_after,
        allowed: game::writes_allowed().is_ok(),
        done: false,
    };
    if dry_run {
        return Ok(sale);
    }
    let label = if purchase { format!("aankoop {} ← {}", p.name, other.name) } else { format!("verkoop {} → {}", p.name, other.name) };
    game::apply_change(Change { label, patches, soft, deltas })?;
    sale.done = true;
    Ok(sale)
}
