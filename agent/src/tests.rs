//! Tests tegen een gesimuleerd FM-geheugen: de spellogica met de Windows- en de Mac-indeling,
//! en de MSVC-RTTI-herkenning tegen een echte MSVC-DLL (testdata/rtti_test.dll).

use serde_json::{Value, json};

use crate::game::{self, Layout};
use crate::mem::sys::{Block, with};
use crate::mem::{self, Abi, Image};
use crate::rtti;

/// De module heeft globale toestand (de gevonden manager, de undo-stapel, de toestemming):
/// tests lopen daarom na elkaar.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[must_use]
fn reset(abi: Abi) -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    with(|s| {
        s.abi = abi;
        s.blocks.clear();
        s.images.clear();
    });
    game::reset();
    game::set_allow_untested(false);
    guard
}

/// Een stuk gesimuleerde heap met een eenvoudige allocator.
struct Heap {
    start: u64,
    data: Vec<u8>,
    next: usize,
}

impl Heap {
    fn new(start: u64, size: usize) -> Heap {
        Heap { start, data: vec![0; size], next: 0x100 }
    }
    fn alloc(&mut self, size: usize) -> u64 {
        let a = self.next;
        self.next = (self.next + size + 15) & !15;
        assert!(self.next <= self.data.len(), "heap vol");
        self.start + a as u64
    }
    fn put(&mut self, addr: u64, b: &[u8]) {
        let o = (addr - self.start) as usize;
        self.data[o..o + b.len()].copy_from_slice(b);
    }
    fn w64(&mut self, a: u64, v: u64) {
        self.put(a, &v.to_le_bytes())
    }
    fn w32(&mut self, a: u64, v: u32) {
        self.put(a, &v.to_le_bytes())
    }
    fn w16(&mut self, a: u64, v: u16) {
        self.put(a, &v.to_le_bytes())
    }
    fn w8(&mut self, a: u64, v: u8) {
        self.put(a, &[v])
    }
    fn fm_string(&mut self, s: &str) -> u64 {
        let a = self.alloc(4 + s.len());
        self.w32(a, s.len() as u32);
        self.put(a + 4, s.as_bytes());
        a
    }
    /// Naam-entry: pointer naar een FM-string.
    fn name(&mut self, s: &str) -> u64 {
        let st = self.fm_string(s);
        let e = self.alloc(8);
        self.w64(e, st);
        e
    }
    /// (begin, end, cap)-lijst met pointers op `hdr`.
    fn list(&mut self, hdr: u64, items: &[u64]) {
        let arr = self.alloc(items.len().max(1) * 8);
        for (i, p) in items.iter().enumerate() {
            self.w64(arr + i as u64 * 8, *p);
        }
        self.w64(hdr, arr);
        self.w64(hdr + 8, arr + items.len() as u64 * 8);
        self.w64(hdr + 16, arr + items.len() as u64 * 8);
    }
    fn install(self, private: bool) {
        with(|s| s.blocks.push(Block { start: self.start, data: self.data, private }));
    }
}

fn date_bytes(year: u16, doy: u16) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[..2].copy_from_slice(&doy.to_le_bytes());
    b[2..].copy_from_slice(&year.to_le_bytes());
    b
}

fn call(req: Value) -> Result<Value, String> {
    crate::dispatch(&req)
}

/// Wat de test in het geheugen heeft gezet, om achteraf te controleren.
struct World {
    finance: u64,
    p1: u64,
    p2: u64,
    p1_contract: u64,
    p1_injuries: u64,
    p2_absences: u64,
    weeks: Vec<u64>,
    /// begrip-records van speler 1 en 2
    tu1: u64,
    tu2: u64,
    human: u64,
}

/// Bouwt een FM-wereld: manager → contract → team → club → financiën, twee spelers (een gewone en
/// een speler-trainer), blessures, een schorsing en drie trainingsweken vanaf ma 15 maart 2027.
/// `game_plugin` is het image (met de vaste plekken), `tb_delta` de afstand van de team-pointer
/// tot de weeklijst in het trainingsblok.
fn build_world(l: &Layout, plugin_base: u64, staff_rva: u64, date_rva: Option<u64>, tb_delta: u64) -> World {
    let mut h = Heap::new(0x2_0000_0000, 1 << 20);
    let mut statics = Heap::new(plugin_base + (staff_rva & !0xFFF), 0x2000);
    statics.next = 0x2000; // niet alloceren, alleen schrijven

    // club en financiën
    let finance = h.alloc(0x900);
    h.put(finance + 0x0C, &25_000_000i32.to_le_bytes());
    h.put(finance + 0x7C4, &12_000_000i32.to_le_bytes());
    h.put(finance + 0x808, &900_000i32.to_le_bytes());
    h.put(finance + 0x814, &850_000i32.to_le_bytes());
    h.put(finance + 0x864, &45.0f32.to_le_bytes());
    h.w8(finance + 0x8A9, 15);
    let club = h.alloc(0x160);
    h.w32(club + 0x0C, 992);
    let cname = h.fm_string("AFC Ajax");
    h.w64(club + 0xC0, cname);
    h.w64(club + 0x150, finance);
    let team = h.alloc(0xB0);
    h.w64(team + 0x30, club);

    // de menselijke manager en zijn contract; op +0x440 het HUMAN_NON_PLAYER-subobject (vtable)
    let human = h.alloc(0x600);
    let contract = h.alloc(0x80);
    h.w64(human + l.staff_contract + l.human_shift, contract);
    let hvt = fake_class(mem::abi(), plugin_base, "db", "HUMAN_NON_PLAYER");
    h.w64(human + 0x440, hvt);
    h.w64(contract + 0x08, human + l.staff_identity + l.human_shift);
    h.w64(contract + 0x10, team);
    // andere staf zonder club
    let other = h.alloc(0x600);
    let staff_arr = h.alloc(3 * 8);
    h.w64(staff_arr, other);
    h.w64(staff_arr + 8, human);
    h.w64(staff_arr + 16, other);
    let pair = plugin_base + staff_rva;
    statics.w64(pair, staff_arr);
    statics.w64(pair + 8, staff_arr + 24);

    // speler 1: gewoon, geblesseerd, ontevreden
    let p1 = h.alloc(0x500);
    h.w64(p1 + l.p_team, team);
    h.w16(p1 + l.p_sharpness, 6700);
    h.w16(p1 + l.p_condition, 9000);
    h.w8(p1 + l.p_morale, 9);
    let n = h.name("Kenneth");
    h.w64(p1 + l.p_first, n);
    let n = h.name("Taylor");
    h.w64(p1 + l.p_last, n);
    h.w32(p1 + l.p_uid, 111);
    let c1 = h.alloc(0x80);
    h.w64(p1 + l.p_contract, c1);
    h.w64(c1 + 0x08, p1 + l.p_identity);
    h.w8(c1 + 0x5A, (-4i8) as u8);
    h.w8(c1 + 0x5B, 0x60); // de byte erna hoort er niet bij
    h.w64(c1 + 0x30, 0xDEAD_BEE8);
    let inj_rec = h.alloc(0x40);
    h.w64(p1 + l.p_injuries, inj_rec);
    let kind = h.alloc(0x20);
    let kname = h.fm_string("Hamstring Strain");
    h.w64(kind + 0x18, kname);
    let inj = h.alloc(0x30);
    h.w64(inj + 0x08, kind);
    h.list(inj_rec, &[inj]);

    // speler 2: speler-trainer (identiteit, namen en contract 0xF8 verder), geschorst
    let s = l.player_coach_shift;
    let p2 = h.alloc(0x600);
    h.w64(p2 + l.p_team, team);
    h.w8(p2 + l.p_morale, 14);
    let n = h.name("Jordan Henderson");
    h.w64(p2 + l.p_common + s, n);
    h.w32(p2 + l.p_uid + s, 222);
    let c2 = h.alloc(0x80);
    h.w64(p2 + l.p_contract + s, c2);
    h.w64(c2 + 0x08, p2 + l.p_identity + s);
    let rec2 = h.alloc(0x40);
    h.w64(p2 + l.p_injuries, rec2);
    h.list(rec2, &[]);
    let ban = h.alloc(0x20);
    h.w16(ban + 0x16, 0xFFFF);
    let callup = h.alloc(0x20);
    h.list(rec2 + 0x18, &[ban, callup]);

    // tactisch begrip: speler 1 kent instelling 10 (en voor Passing Style ook 19) en heeft volle
    // lijsten; speler 2 kent 10 behalve voor Passing Style, met in elke lijst ruimte over
    let tu1 = h.alloc(0xE0);
    let tu2 = h.alloc(0xE0);
    for a in 0..6u64 {
        let items: &[u32] = if a == 0 { &[0x4F6 << 20 | 10 << 14 | 4384, 19 << 14 | 600] } else { &[10 << 14 | 5000] };
        let arr = h.alloc(items.len() * 4);
        for (i, it) in items.iter().enumerate() {
            h.w32(arr + i as u64 * 4, *it);
        }
        let blk = tu1 + a * 0x20;
        h.w64(blk, arr);
        h.w64(blk + 8, arr + items.len() as u64 * 4);
        h.w64(blk + 16, arr + items.len() as u64 * 4);
        h.w64(blk + 0x18, a);
        // speler 2: voor Passing Style nog niets (lege lijst met ruimte voor twee items), verder 10
        let arr = h.alloc(8);
        h.w32(arr, 0xDEAD_0000); // rommel in het ongebruikte deel, zoals in FM
        h.w32(arr + 4, 0xDEAD_0000);
        let n = if a == 0 { 0 } else { h.w32(arr, 10 << 14 | 2500); 1 };
        let blk = tu2 + a * 0x20;
        h.w64(blk, arr);
        h.w64(blk + 8, arr + 4 * n);
        h.w64(blk + 16, arr + 8);
        h.w64(blk + 0x18, a);
    }
    // vormen: Pressing Trap (bit 13 van de instelling uit) en Out Of Possession Shape (aan)
    let shapes = h.alloc(32);
    for (i, setting) in [0x18B4u16, 0x3934].iter().enumerate() {
        let it = shapes + i as u64 * 16;
        h.w16(it, *setting);
        h.put(it + 2, &[0xF6, 0xF4, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0]);
        h.w16(it + 0x0E, 3984);
    }
    h.w64(tu1 + 0xC0, shapes);
    h.w64(tu1 + 0xC8, shapes + 32);
    h.w64(tu1 + 0xD0, shapes + 32);
    h.put(tu1 + 0xD8, &date_bytes(2026, 286));
    h.put(tu2 + 0xD8, &date_bytes(2026, 286));
    h.w64(p1 + l.p_understanding, tu1);
    h.w64(p2 + l.p_understanding, tu2);

    // iemand van een ander team in de lijst: hoort niet in de selectie
    let stranger = h.alloc(0x500);
    h.w64(stranger + l.p_team, 0x5_0000_0000);
    h.list(team + 0x38, &[p1, p2, stranger]);

    // spel-datum: di 16 maart 2027 (dag 75), via het team én (als die er is) de vaste plek
    let sched = h.alloc(0xA0);
    h.put(sched + 0x94, &date_bytes(2027, 75));
    h.w64(team + 0xA0, sched);
    if let Some(rva) = date_rva {
        let mut d = Heap::new(plugin_base + (rva & !0xFFF), 0x1000);
        d.next = 0x1000;
        d.put(plugin_base + rva, &date_bytes(2027, 75));
        d.install(false);
    }

    // trainingsblok: team-pointer, dan op `tb_delta` de weeklijst
    let tb = h.alloc(0x100);
    h.w64(tb, team);
    let sname = h.fm_string("Tactical Focus - Balanced");
    let mut weeks = Vec::new();
    for w in 0..3u16 {
        let rec = h.alloc(0x60);
        h.w64(rec, sname);
        h.put(rec + 0x0C, &date_bytes(2027, 74 + 7 * w));
        for d in 0..7u64 {
            let base = rec + 0x10 + d * 8;
            h.w16(base + 2, 300); // Rest
            h.w16(base + 4, 300);
            h.w16(base + 6, 300);
        }
        // zaterdag wedstrijd
        h.w16(rec + 0x10 + 5 * 8 + 4, 500);
        weeks.push(rec);
    }
    h.list(tb + tb_delta, &weeks);

    h.install(true);
    statics.install(false);
    World { finance, p1, p2, p1_contract: c1, p1_injuries: inj_rec, p2_absences: rec2 + 0x18, weeks, tu1, tu2, human }
}

fn i32_at(a: u64) -> i32 {
    mem::i32_at(a).unwrap()
}

/// Het hele traject zoals de app het doorloopt, voor één platformindeling.
fn run_world(l: &'static Layout, w: &World) {
    let ping = call(json!({"cmd": "ping"})).unwrap();
    assert_eq!(ping["pid"], 4242);

    let club = call(json!({"cmd": "club"})).unwrap();
    assert_eq!(club["name"], "AFC Ajax");
    assert_eq!(club["uid"], 992);
    assert_eq!(club["transfer_budget"], 12_000_000);

    let squad = call(json!({"cmd": "squad"})).unwrap();
    let squad = squad.as_array().unwrap();
    assert_eq!(squad.len(), 2, "alleen de spelers van het eigen team: {squad:?}");
    let jordan = &squad[0];
    let kenneth = &squad[1];
    assert_eq!(kenneth["name"], "Kenneth Taylor");
    assert_eq!(kenneth["uid"], 111);
    assert_eq!(kenneth["morale"], 9);
    assert_eq!(kenneth["condition"], 9000);
    assert_eq!(kenneth["sharpness"], 6700);
    assert_eq!(kenneth["happiness"], -4);
    assert_eq!(kenneth["unhappy_reasons"], true);
    assert_eq!(kenneth["injuries"][0]["name"], "Hamstring Strain");
    assert_eq!(jordan["name"], "Jordan Henderson", "speler-trainer: namen liggen verder");
    assert_eq!(jordan["uid"], 222);
    assert_eq!(jordan["bans"], 1);

    // datum en kalender
    let d = call(json!({"cmd": "game_date"})).unwrap();
    assert_eq!(d, json!({"year": 2027, "doy": 75}));
    let tw = call(json!({"cmd": "training_weeks"})).unwrap();
    let weeks = tw["weeks"].as_array().unwrap();
    assert_eq!(weeks.len(), 3);
    assert_eq!(weeks[0]["start"], json!({"year": 2027, "doy": 74}));
    assert_eq!(weeks[0]["days"][5], json!([300, 500, 300]));

    // de week aanpassen: ma (verleden) en di (vandaag) blijven, za (wedstrijd) blijft
    let req = json!({"cmd": "set_week", "start": {"year": 2027, "doy": 74}, "days": vec![[4u16, 5, 1]; 7]});
    let build_tested = call(json!({"cmd": "ping"})).unwrap()["known_build"].as_bool().unwrap();
    if !build_tested {
        // niet-geteste build: eerst geweigerd (en niets veranderd), daarna bewust toegestaan
        assert!(call(req.clone()).unwrap_err().contains("alleen lezen"));
        call(json!({"cmd": "allow_untested", "on": true})).unwrap();
    }
    let r = call(req).unwrap();
    assert_eq!(r["days"], 4, "wo, do, vr en zo");
    let day = |rec: u64, d: u64| -> [u16; 3] {
        let b = rec + 0x10 + d * 8;
        [mem::u16_at(b + 2).unwrap(), mem::u16_at(b + 4).unwrap(), mem::u16_at(b + 6).unwrap()]
    };
    assert_eq!(day(w.weeks[0], 0), [300, 300, 300], "maandag ligt in het verleden");
    assert_eq!(day(w.weeks[0], 1), [300, 300, 300], "vandaag blijft");
    assert_eq!(day(w.weeks[0], 2), [4, 5, 1]);
    assert_eq!(day(w.weeks[0], 5), [300, 500, 300], "wedstrijddag blijft van FM");
    assert_eq!(day(w.weeks[0], 6), [4, 5, 1]);
    call(json!({"cmd": "undo"})).unwrap();
    assert_eq!(day(w.weeks[0], 2), [300, 300, 300], "undo zet de training terug");

    // financiën
    call(json!({"cmd": "set_finances", "transfer_budget": 50_000_000, "wage_budget": 1_000_000})).unwrap();
    assert_eq!(i32_at(w.finance + 0x7C4), 50_000_000);
    assert_eq!(i32_at(w.finance + 0x814), 1_000_000);
    assert_eq!(i32_at(w.finance + 0x808), 1_000_000, "totaal loonbudget nooit lager dan het loonbudget");
    call(json!({"cmd": "undo"})).unwrap();
    assert_eq!(i32_at(w.finance + 0x7C4), 12_000_000);
    assert_eq!(i32_at(w.finance + 0x808), 900_000);

    // blessures, schorsingen, moraal
    assert_eq!(call(json!({"cmd": "heal"})).unwrap()["players"], 1);
    assert_eq!(mem::u64_at(w.p1_injuries), mem::u64_at(w.p1_injuries + 8), "blessurelijst leeg");
    assert_eq!(call(json!({"cmd": "lift_bans"})).unwrap()["bans"], 1);
    let (b, e) = (mem::u64_at(w.p2_absences).unwrap(), mem::u64_at(w.p2_absences + 8).unwrap());
    assert_eq!(e - b, 8, "de oproep blijft, de schorsing is weg");
    assert_eq!(mem::u16_at(mem::u64_at(b).unwrap() + 0x16), Some(0));
    assert_eq!(call(json!({"cmd": "inspire", "uids": [111]})).unwrap()["players"], 1);
    assert_eq!(mem::u8_at(w.p1 + l.p_morale), Some(20));
    assert_eq!(mem::u64_at(w.p1_contract + 0x30), Some(0));
    assert_eq!(mem::u8_at(w.p2 + l.p_morale), Some(14), "alleen de gekozen speler");
    call(json!({"cmd": "undo"})).unwrap();
    assert_eq!(mem::u8_at(w.p1 + l.p_morale), Some(9));
    assert_eq!(mem::u64_at(w.p1_contract + 0x30), Some(0xDEAD_BEE8));

    // spelregels van de manager (editorvlaggen): alleen waar de plek bekend is
    let hf = call(json!({"cmd": "human_flags"})).unwrap();
    assert_eq!(hf["supported"], l.human_flags.is_some());
    if l.human_flags.is_some() {
        assert_eq!(hf["registration_anytime"], false);
        let r = call(json!({"cmd": "set_human_flags", "registration_anytime": true})).unwrap();
        assert_eq!(r["registration_anytime"], true);
        assert_eq!(r["numbers_anytime"], false);
        assert_eq!(mem::u32_at(w.human + 0x3FC), Some(0x100));
        let r = call(json!({"cmd": "set_human_flags", "numbers_anytime": true})).unwrap();
        assert_eq!(mem::u32_at(w.human + 0x3FC), Some(0x300));
        assert_eq!(r["flags"], 0x300);
        call(json!({"cmd": "undo"})).unwrap();
        call(json!({"cmd": "undo"})).unwrap();
        assert_eq!(mem::u32_at(w.human + 0x3FC), Some(0), "twee keer ongedaan maken: alles terug");
    } else {
        assert!(call(json!({"cmd": "set_human_flags", "registration_anytime": true})).is_err());
    }

    // tactisch begrip: lezen
    let u = call(json!({"cmd": "understanding"})).unwrap();
    assert_eq!(u["keys"], json!([10, 10, 10, 10, 10, 10]), "de instelling die de meeste spelers kennen");
    assert_eq!(u["without_record"], 0);
    let by_uid = |u: &Value, uid: u64| u["players"].as_array().unwrap().iter().find(|p| p["uid"] == uid).cloned().unwrap();
    let k = by_uid(&u, 111);
    assert_eq!(k["levels"], json!([4384, 5000, 5000, 5000, 5000, 5000, 3984, 3984]));
    assert_eq!(k["areas"][0], json!([[10, 4384], [19, 600]]));
    let j = by_uid(&u, 222);
    assert_eq!(j["levels"], json!([1000, 2500, 2500, 2500, 2500, 2500, 1000, 1000]), "zonder items geldt de bodem van 1000");
    assert_eq!(u["team"][1], 3750);

    // maximaliseren: eerst alleen tellen, dan echt
    let dry = call(json!({"cmd": "max_understanding", "dry_run": true})).unwrap();
    assert_eq!(dry["players"], 2);
    assert_eq!(dry["items"], 9 + 5, "7 items + 2 vormen van speler 1, 5 items van speler 2");
    assert_eq!(dry["added"], 1, "speler 2 krijgt instelling 10 voor Passing Style erbij");
    assert_eq!(dry["no_room"], 0);
    assert_eq!(mem::u32_at(mem::u64_at(w.tu1).unwrap()), Some(0x4F6 << 20 | 10 << 14 | 4384), "dry run verandert niets");
    let r = call(json!({"cmd": "max_understanding"})).unwrap();
    assert_eq!(r["items"], 14);
    assert_eq!(r["added"], 1);
    let a0 = mem::u64_at(w.tu1).unwrap();
    assert_eq!(mem::u32_at(a0), Some(0x4F6 << 20 | 10 << 14 | 10000), "niveau op 10000, de andere bits blijven");
    assert_eq!(mem::u32_at(a0 + 4), Some(19 << 14 | 10000));
    let shapes = mem::u64_at(w.tu1 + 0xC0).unwrap();
    assert_eq!(mem::u16_at(shapes + 0x0E), Some(10000));
    assert_eq!(mem::u16_at(shapes + 16 + 0x0E), Some(10000));
    assert_eq!(mem::u16_at(shapes + 16), Some(0x3934), "de rest van het item blijft");
    let b0 = mem::u64_at(w.tu2).unwrap();
    assert_eq!(mem::u64_at(w.tu2 + 8), Some(b0 + 4), "lijst van speler 2 heeft nu één item");
    assert_eq!(mem::u32_at(b0), Some(10 << 14 | 10000));
    assert_eq!(mem::u32_at(b0 + 4), Some(0xDEAD_0000), "voorbij het einde blijft alles staan");
    assert_eq!(mem::u8_at(w.tu1 + 0xDC), Some(1), "veranderd-vlag zoals FM hem zet");
    let u = call(json!({"cmd": "understanding"})).unwrap();
    assert_eq!(by_uid(&u, 222)["levels"], json!([10000, 10000, 10000, 10000, 10000, 10000, 1000, 1000]), "zonder vorm-items blijft de bodem");
    assert_eq!(u["team"][0], 10000);
    assert_eq!(call(json!({"cmd": "undo"})).unwrap()["undone"].as_str().unwrap(), "tactisch begrip / tactical understanding");
    assert_eq!(mem::u32_at(a0), Some(0x4F6 << 20 | 10 << 14 | 4384));
    assert_eq!(mem::u64_at(w.tu2 + 8), Some(b0), "lijst van speler 2 is weer leeg");
    assert_eq!(mem::u32_at(b0), Some(0xDEAD_0000), "de rommel staat er weer");
    assert_eq!(mem::u8_at(w.tu1 + 0xDC), Some(0));
    assert_eq!(mem::u16_at(shapes + 0x0E), Some(3984));
}

/// Een PE-header van game_plugin.dll zoals FM27 Steam-build 26340721 hem heeft.
fn fake_pe_header(base: u64) -> Vec<u8> {
    let mut b = vec![0u8; 0x400];
    b[0..2].copy_from_slice(b"MZ");
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    let nt = 0x80;
    b[nt..nt + 4].copy_from_slice(b"PE\0\0");
    b[nt + 6..nt + 8].copy_from_slice(&1u16.to_le_bytes()); // 1 sectie
    b[nt + 8..nt + 12].copy_from_slice(&0x6AE5_65AFu32.to_le_bytes());
    b[nt + 20..nt + 22].copy_from_slice(&240u16.to_le_bytes());
    let opt = nt + 24;
    b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
    b[opt + 24..opt + 32].copy_from_slice(&base.to_le_bytes());
    b[opt + 56..opt + 60].copy_from_slice(&0x1821_C000u32.to_le_bytes());
    let sec = opt + 240;
    b[sec..sec + 6].copy_from_slice(b".data\0");
    b[sec + 8..sec + 12].copy_from_slice(&0x0100_0000u32.to_le_bytes());
    b[sec + 12..sec + 16].copy_from_slice(&0x0500_0000u32.to_le_bytes());
    b
}

#[test]
fn windows_layout_end_to_end() {
    let _g = reset(Abi::Msvc);
    let base = 0x7FF6_0000_0000u64;
    with(|s| s.blocks.push(Block { start: base, data: fake_pe_header(base), private: false }));
    let (segments, id) = mem::pe_layout(base).expect("PE-header");
    assert_eq!(id, "PE-6AE565AF-1821C000", "build-id zoals FMLibreEditor hem noemt");
    assert_eq!(segments[0].name, ".data");
    with(|s| s.images.push(Image { path: r"C:\Steam\steamapps\common\Football Manager 27\fm_Data\Plugins\x86_64\game_plugin.dll".into(), id: Some(id), start: base, end: base + 0x1821_C000, segments }));
    assert!(mem::game_plugin().is_some());

    // trainingsblok op 0x30 in plaats van 0x28: de Windows-versie moet hem toch vinden
    let w = build_world(&game::WIN, base, 0x5705818, Some(0x56AF378), 0x30);
    // de vaste datum wint op Windows: maak die van het team anders om dat te zien
    run_world(&game::WIN, &w);
    let ping = call(json!({"cmd": "ping"})).unwrap();
    assert_eq!(ping["platform"], "windows");
    assert_eq!(ping["known_build"], true, "de Windows-build is live getest (8 nov 2026)");
    assert_eq!(ping["untested_writes"], false, "op de geteste Windows-build is geen toestemming nodig");
}

#[test]
fn mac_layout_end_to_end() {
    let _g = reset(Abi::Itanium);
    let base = 0x1_2000_0000u64;
    with(|s| s.images.push(Image { path: "/x/game_plugin.bundle/Contents/MacOS/game_plugin".into(), id: Some("F4540355-2186-39CB-9119-C706A6A35346".into()), start: base, end: base + 0x6000_0000, segments: Vec::new() }));
    let w = build_world(&game::MAC, base, 0x56D2E60, None, 0x28);
    run_world(&game::MAC, &w);
    let ping = call(json!({"cmd": "ping"})).unwrap();
    assert_eq!(ping["known_build"], true);
    assert_eq!(ping["untested_writes"], false, "op de geteste Mac-build is geen toestemming nodig");
}

#[test]
fn unknown_build_is_read_only() {
    let _g = reset(Abi::Msvc);
    let base = 0x7FF6_0000_0000u64;
    with(|s| s.images.push(Image { path: "game_plugin.dll".into(), id: Some("PE-00000000-1000".into()), start: base, end: base + 0x1000, segments: Vec::new() }));
    // geen bekende build: geen staflijst, geen RTTI → geen manager, en zeker geen schrijfacties
    assert!(call(json!({"cmd": "club"})).unwrap_err().contains("manager"));
    assert_eq!(call(json!({"cmd": "ping"})).unwrap()["known_build"], false);
}

/// Laadt een PE-bestand in het gesimuleerde geheugen op zijn ImageBase (zoals Windows het mapt).
fn map_pe(file: &[u8]) -> u64 {
    let u16le = |o: usize| u16::from_le_bytes(file[o..o + 2].try_into().unwrap()) as usize;
    let u32le = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap()) as usize;
    let nt = u32le(0x3C);
    let nsec = u16le(nt + 6);
    let opt = nt + 24;
    let opt_size = u16le(nt + 20);
    let base = u64::from_le_bytes(file[opt + 24..opt + 32].try_into().unwrap());
    let size = u32le(opt + 56);
    let headers = u32le(opt + 60);
    let mut img = vec![0u8; size];
    img[..headers].copy_from_slice(&file[..headers]);
    for i in 0..nsec {
        let s = opt + opt_size + i * 40;
        let (vsize, va, rawsize, rawptr) = (u32le(s + 8), u32le(s + 12), u32le(s + 16), u32le(s + 20));
        let n = rawsize.min(vsize.max(rawsize)).min(file.len() - rawptr);
        img[va..va + n].copy_from_slice(&file[rawptr..rawptr + n]);
    }
    with(|s| s.blocks.push(Block { start: base, data: img, private: false }));
    let (segments, id) = mem::pe_layout(base).unwrap();
    with(|s| s.images.push(Image { path: r"C:\fm\game_plugin.dll".into(), id: Some(id), start: base, end: base + size as u64, segments }));
    base
}

#[test]
fn msvc_rtti_on_a_real_dll() {
    let _g = reset(Abi::Msvc);
    map_pe(include_bytes!("../testdata/rtti_test.dll"));
    let img = mem::game_plugin().unwrap();
    assert!(img.segments.iter().any(|s| s.name == ".rdata"));

    let names = rtti::decorated("db", "HUMAN_NON_PLAYER");
    assert_eq!(names[0], ".?AVHUMAN_NON_PLAYER@db@@");
    let vts = rtti::vtables_of(&img, &names);
    assert_eq!(vts.len(), 1, "precies één primaire vtable: {vts:x?}");

    // objecten op de heap herkennen
    let mut h = Heap::new(0x3_0000_0000, 0x1000);
    let obj = h.alloc(0x40);
    h.w64(obj, vts[0]);
    let coach_vts = rtti::vtables_of(&img, &rtti::decorated("db", "PLAYER_AND_NON_PLAYER"));
    assert_eq!(coach_vts.len(), 1);
    let coach = h.alloc(0x40);
    h.w64(coach, coach_vts[0]);
    h.install(true);

    assert_eq!(rtti::class_name(obj).as_deref(), Some(".?AVHUMAN_NON_PLAYER@db@@"));
    assert_eq!(rtti::demangle(".?AVHUMAN_NON_PLAYER@db@@"), "db::HUMAN_NON_PLAYER");
    assert_eq!(rtti::class_name(coach).as_deref(), Some(".?AVPLAYER_AND_NON_PLAYER@db@@"));
    assert_eq!(rtti::instances(&vts, 10), vec![obj]);

    let census: Vec<String> = rtti::census(&img, "PLAYER", 100).into_iter().map(|(n, _)| rtti::demangle(&n)).collect();
    for c in ["db::HUMAN_NON_PLAYER", "db::PLAYER", "db::PLAYER_AND_NON_PLAYER"] {
        assert!(census.iter().any(|n| n == c), "{c} ontbreekt in {census:?}");
    }
    assert!(!census.iter().any(|n| n.contains("OTHER")), "filter werkt");
}

#[test]
fn itanium_names() {
    let _g = reset(Abi::Itanium);
    assert_eq!(rtti::decorated("db", "HUMAN_NON_PLAYER"), vec!["N2db16HUMAN_NON_PLAYERE".to_string()]);
    assert_eq!(rtti::demangle("N2db16HUMAN_NON_PLAYERE"), "db::HUMAN_NON_PLAYER");
}

// ---- spelers verkopen -------------------------------------------------------------------------

/// Zet een klasse met RTTI in het gesimuleerde image zoals de compiler van het platform dat doet,
/// en geeft de vtable. `base` is het begin van het image; de secties komen erbij.
fn fake_class(abi: Abi, base: u64, ns: &str, class: &str) -> u64 {
    match abi {
        Abi::Itanium => {
            let name = format!("N{}{}{}{}E", ns.len(), ns, class.len(), class);
            let mut text = Heap::new(base + 0x10_0000, 0x1000);
            let t = text.alloc(name.len() + 2);
            text.put(t + 1, name.as_bytes());
            let mut data = Heap::new(base + 0x20_0000, 0x1000);
            let ti = data.alloc(16);
            data.w64(ti, 0x1234_5670); // vtable van __class_type_info: doet hier niet ter zake
            data.w64(ti + 8, t + 1);
            let vt_block = data.alloc(48);
            data.w64(vt_block, 0); // offset-to-top
            data.w64(vt_block + 8, ti);
            data.w64(vt_block + 16, 0xAAAA_0000);
            let vt = vt_block + 16;
            with(|s| {
                let img = s.images.iter_mut().find(|i| i.start == base).unwrap();
                img.segments.push(mem::Segment { name: "__TEXT".into(), start: text.start, end: text.start + 0x1000 });
                img.segments.push(mem::Segment { name: "__DATA_CONST".into(), start: data.start, end: data.start + 0x1000 });
            });
            text.install(false);
            data.install(false);
            vt
        }
        Abi::Msvc => {
            let name = format!(".?AV{class}@{ns}@@");
            let mut data = Heap::new(base + 0x10_0000, 0x1000);
            let td = data.alloc(16 + name.len() + 1);
            data.w64(td, 0x7FF0_0000_1000); // vftable van type_info
            data.put(td + 16, name.as_bytes());
            let mut rdata = Heap::new(base + 0x20_0000, 0x1000);
            let col = rdata.alloc(24);
            rdata.w32(col, 1);
            rdata.w32(col + 12, (td - base) as u32);
            rdata.w32(col + 20, (col - base) as u32);
            let vt_block = rdata.alloc(32);
            rdata.w64(vt_block, col);
            rdata.w64(vt_block + 8, 0x7FF0_0000_2000);
            with(|s| {
                let img = s.images.iter_mut().find(|i| i.start == base).unwrap();
                img.segments.push(mem::Segment { name: ".data".into(), start: data.start, end: data.start + 0x1000 });
                img.segments.push(mem::Segment { name: ".rdata".into(), start: rdata.start, end: rdata.start + 0x1000 });
            });
            data.install(false);
            rdata.install(false);
            vt_block + 8
        }
    }
}

struct Market {
    finance: u64,
    senior: u64,
    u21: u64,
    wijndal: u64,
    wijndal_contract: u64,
    milan: u64,
    milan_finance: u64,
    /// de eerste speler van AC Milan (uid 100), om te kopen
    milan_player: u64,
    milan_player_contract: u64,
    our_selection: u64,
    national_selection: u64,
}

/// Ajax (de menselijke manager) met Wijndal (aanvoerder, ook in de U21-lijst), twee andere spelers
/// en een speler-trainer; AC Milan met een selectie die nog ruimte heeft; een club met een volle
/// selectie; AC Milan Women; en de opstellingen van Ajax en van een nationaal team.
fn build_market(abi: Abi, l: &Layout, base: u64, staff_rva: u64, clubs_rva: u64, date_rva: Option<u64>) -> Market {
    let mut h = Heap::new(0x2_0000_0000, 1 << 20);
    let mut statics = Heap::new(base + (staff_rva & !0xFFF), 0x2000);
    statics.next = 0x2000;

    let finance = |h: &mut Heap, balance: i32, budget: i32| {
        let f = h.alloc(0x900);
        h.put(f + 0x0C, &balance.to_le_bytes());
        h.put(f + 0x7C4, &budget.to_le_bytes());
        h.put(f + 0x808, &900_000i32.to_le_bytes());
        h.put(f + 0x814, &850_000i32.to_le_bytes());
        h.put(f + 0x864, &45.0f32.to_le_bytes());
        h.w8(f + 0x8A9, 15);
        f
    };
    let club = |h: &mut Heap, uid: u32, name: &str, fin: u64, women: u8| {
        let c = h.alloc(0x160);
        h.w32(c + 0x0C, uid);
        let n = h.fm_string(name);
        h.w64(c + 0xC0, n);
        h.w64(c + 0x150, fin);
        h.w8(c + 0xB9, women);
        c
    };
    let team = |h: &mut Heap, club: u64| {
        let t = h.alloc(0xB0);
        h.w64(t + 0x30, club);
        t
    };
    // een speler (shift 0x0 of de speler-trainer-shift) bij `team`, met contract
    let player = |h: &mut Heap, team: u64, uid: u32, person: u32, name: &str, shift: u64| {
        let p = h.alloc(0x600);
        h.w64(p + l.p_team, team);
        h.w8(p + l.p_morale, 12);
        let n = h.name(name);
        h.w64(p + l.p_common + shift, n);
        h.w32(p + l.p_uid + shift, uid);
        h.w32(p + l.p_identity + shift + 8, person);
        h.put(p + l.p_value, &8_000_000i32.to_le_bytes());
        let c = h.alloc(0xC8);
        h.w64(p + l.p_contract + shift, c);
        h.w64(c + 0x08, p + l.p_identity + shift);
        h.w64(c + 0x10, team);
        h.w64(c + 0x18, team);
        h.put(c + 0x20, &20_000i32.to_le_bytes());
        h.w8(c + 0x26, 1);
        h.put(c + 0x44, &date_bytes(2024, 183 | 0x4000));
        h.put(c + 0x48, &date_bytes(2029, 181));
        h.put(c + 0x4C, &date_bytes(2024, 183 | 0x4000));
        h.w8(c + 0x57, 0b01);
        h.w8(c + 0x5D, 5);
        (p, c)
    };

    // Ajax
    let fin = finance(&mut h, 25_000_000, 12_000_000);
    let ajax = club(&mut h, 992, "AFC Ajax", fin, 0);
    let senior = team(&mut h, ajax);
    let u21 = team(&mut h, ajax);
    h.list(ajax + 0x18, &[senior, u21]);
    let (wijndal, wc) = player(&mut h, senior, 37058888, 5001, "Owen Wijndal", 0);
    let (p3, _) = player(&mut h, senior, 2, 5003, "Davy Klaassen", 0);
    let (p4, _) = player(&mut h, senior, 3, 5004, "Daley Blind", 0);
    let (coach, _) = player(&mut h, senior, 4, 5002, "Jordi Cruijff", l.player_coach_shift);
    // de selectie van Ajax: 4 spelers in een blok van 144 bytes, met ruimte om iemand te kopen
    let ajax_block = h.alloc(144);
    for (i, q) in [p3, wijndal, p4, coach].iter().enumerate() {
        h.w64(ajax_block + i as u64 * 8, *q);
    }
    h.w64(senior + 0x38, ajax_block);
    h.w64(senior + 0x40, ajax_block + 32);
    h.w64(senior + 0x48, ajax_block + 32);
    h.list(u21 + 0x38, &[wijndal]);
    h.w64(senior + 0x88, wijndal + l.p_identity); // aanvoerder
    // de menselijke manager
    let human = h.alloc(0x600);
    let hc = h.alloc(0x80);
    h.w64(human + l.staff_contract + l.human_shift, hc);
    h.w64(hc + 0x08, human + l.staff_identity + l.human_shift);
    h.w64(hc + 0x10, senior);
    let staff = h.alloc(8);
    h.w64(staff, human);
    statics.w64(base + staff_rva, staff);
    statics.w64(base + staff_rva + 8, staff + 8);
    let sched = h.alloc(0xA0);
    h.put(sched + 0x94, &date_bytes(2027, 76));
    h.w64(senior + 0xA0, sched);
    if let Some(rva) = date_rva {
        let mut d = Heap::new(base + (rva & !0xFFF), 0x1000);
        d.next = 0x1000;
        d.put(base + rva, &date_bytes(2027, 76));
        d.install(false);
    }

    // AC Milan: selectie van 3 in een blok van 144 bytes (ruimte voor 18)
    let milan_fin = finance(&mut h, 100_000_000, 30_000_000);
    let milan = club(&mut h, 45, "AC Milan", milan_fin, 0);
    let ms = team(&mut h, milan);
    h.list(milan + 0x18, &[ms]);
    let block = h.alloc(144);
    let (mut milan_player, mut milan_player_contract) = (0, 0);
    for i in 0..3u64 {
        let (q, qc) = player(&mut h, ms, 100 + i as u32, 6000 + i as u32, &format!("Milan player {i}"), 0);
        h.put(q + l.p_value, &(12_000_000i32 - i as i32 * 1_000_000).to_le_bytes());
        if i == 0 {
            (milan_player, milan_player_contract) = (q, qc);
        }
        h.w64(block + i * 8, q);
    }
    h.w64(ms + 0x38, block);
    h.w64(ms + 0x40, block + 24);
    h.w64(ms + 0x48, block + 24);
    // een club met een volle selectie: na de capaciteit staat iets anders
    let full_fin = finance(&mut h, 1_000_000, 0);
    let full = club(&mut h, 77, "Volle Club", full_fin, 0);
    let fs = team(&mut h, full);
    h.list(full + 0x18, &[fs]);
    let fblock = h.alloc(144);
    let (q, _) = player(&mut h, fs, 200, 7000, "Full player", 0);
    h.w64(fblock, q);
    h.w64(fblock + 8, 0xDEAD_BEEF_0000);
    h.w64(fs + 0x38, fblock);
    h.w64(fs + 0x40, fblock + 8);
    h.w64(fs + 0x48, fblock + 8);
    let inter = club(&mut h, 46, "FC Internazionale Milano", 0, 0);
    let women = club(&mut h, 47, "AC Milan Women", 0, 1);
    let wt = team(&mut h, women);
    h.list(women + 0x18, &[wt]);
    h.list(wt + 0x38, &[]);
    let clubs = h.alloc(5 * 8);
    for (i, c) in [ajax, milan, full, inter, women].iter().enumerate() {
        h.w64(clubs + i as u64 * 8, *c);
    }
    statics.w64(base + clubs_rva, clubs);
    statics.w64(base + clubs_rva + 8, clubs + 40);

    // opstellingen: die van Ajax, die van een nationaal team, en een vrijgegeven blok
    let vt = fake_class(abi, base, "simatch", "PREPARED_TACTICS_CREATOR_ARRAY");
    let selection = |h: &mut Heap, ids: &[u32], sound: bool| {
        let o = h.alloc(0x90);
        h.w64(o, vt);
        if sound {
            let a = h.alloc(8);
            h.w64(o + 8, a);
            h.w64(o + 16, a + 8);
            h.w64(o + 24, a + 8);
        }
        for i in 0..26u64 {
            h.w32(o + 0x28 + i * 4, *ids.get(i as usize).unwrap_or(&u32::MAX));
        }
        o
    };
    let our_selection = selection(&mut h, &[5003, 5001, 5004, 5002], true);
    let national_selection = selection(&mut h, &[9001, 5001, 9002, 9003, 9004, 9005], true);
    let _freed = selection(&mut h, &[5001, 5003, 5004], false);

    h.install(true);
    statics.install(false);
    Market { finance: fin, senior, u21, wijndal, wijndal_contract: wc, milan, milan_finance: milan_fin, milan_player, milan_player_contract, our_selection, national_selection }
}

fn snapshot() -> Vec<Vec<u8>> {
    with(|s| s.blocks.iter().map(|b| b.data.clone()).collect())
}

fn list_items(hdr: u64) -> Vec<u64> {
    let (b, e) = (mem::u64_at(hdr).unwrap(), mem::u64_at(hdr + 8).unwrap());
    (b..e).step_by(8).map(|a| mem::u64_at(a).unwrap()).collect()
}

fn run_market(l: &'static Layout, m: &Market) {
    // clubs zoeken
    let hits = call(json!({"cmd": "clubs", "query": "milan"})).unwrap();
    let names: Vec<&str> = hits.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["AC Milan", "AC Milan Women", "FC Internazionale Milano"]);
    assert_eq!(hits[0]["uid"], 45);
    assert_eq!(hits[1]["women"], true);

    // de selectie toont waarde, salaris, contract en transferlijst
    let squad = call(json!({"cmd": "squad"})).unwrap();
    let w = squad.as_array().unwrap().iter().find(|p| p["uid"] == 37058888).unwrap();
    assert_eq!(w["value"], 8_000_000);
    assert_eq!(w["wage"], 20_000);
    assert_eq!(w["listed"], 1);
    assert_eq!(w["contract_ends"], json!({"year": 2029, "doy": 181}));
    assert_eq!(squad.as_array().unwrap().iter().find(|p| p["uid"] == 4).unwrap()["player_coach"], true);

    // eerst alleen controleren: er verandert niets
    let before = snapshot();
    let plan = call(json!({"cmd": "sell", "uid": 37058888, "club": 45, "fee": 10_000_000, "dry_run": true})).unwrap();
    assert_eq!(plan["done"], false);
    assert_eq!(plan["selections"], 1, "alleen de opstelling van Ajax, niet die van het nationale team");
    assert_eq!(plan["was_captain"], true);
    assert_eq!(plan["buyer_pays"], true);
    assert_eq!(plan["balance_after"], 35_000_000);
    assert_eq!(plan["transfer_budget_after"], 22_000_000, "de som komt ook in je transferbudget");
    assert_eq!(plan["to"], "AC Milan");
    assert!(snapshot() == before, "een controle schrijft niets");

    // kopen: de selectie van AC Milan bekijken, controleren, kopen, ongedaan maken
    let ms_squad = call(json!({"cmd": "club_squad", "club": 45})).unwrap();
    let ms_squad = ms_squad.as_array().unwrap();
    assert_eq!(ms_squad.len(), 3);
    assert_eq!(ms_squad[0]["name"], "Milan player 0", "duurste eerst");
    assert_eq!(ms_squad[0]["value"], 12_000_000);
    let plan = call(json!({"cmd": "buy", "uid": 100, "club": 45, "fee": 5_000_000, "dry_run": true})).unwrap();
    assert_eq!(plan["purchase"], true);
    assert_eq!(plan["from"], "AC Milan");
    assert_eq!(plan["to"], "AFC Ajax");
    assert_eq!(plan["balance_after"], 20_000_000);
    assert_eq!(plan["transfer_budget_after"], 7_000_000);
    assert_eq!(plan["buyer_pays"], true, "AC Milan ontvangt het geld");
    assert_eq!(plan["selections"], 0);
    assert!(snapshot() == before, "een controle schrijft niets");
    let err = |req: Value| call(req).unwrap_err();
    assert!(err(json!({"cmd": "buy", "uid": 37058888, "club": 992, "fee": 1})).contains("eigen club"));
    assert!(err(json!({"cmd": "buy", "uid": 999, "club": 45, "fee": 1})).contains("niet gevonden bij AC Milan"));
    assert!(err(json!({"cmd": "buy", "uid": 1, "club": 47, "fee": 1})).contains("vrouwenclubs"));
    if call(json!({"cmd": "ping"})).unwrap()["known_build"] == false {
        call(json!({"cmd": "allow_untested", "on": true})).unwrap();
    }
    let bought = call(json!({"cmd": "buy", "uid": 100, "club": 45, "fee": 5_000_000, "wage": 25_000})).unwrap();
    assert_eq!(bought["done"], true);
    let senior_list = list_items(m.senior + 0x38);
    assert_eq!(senior_list.len(), 5);
    assert_eq!(senior_list[4], m.milan_player, "achteraan in je selectie");
    let ms = mem::u64_at(mem::u64_at(m.milan + 0x18).unwrap()).unwrap();
    let milan_list = list_items(ms + 0x38);
    assert_eq!(milan_list.len(), 2);
    assert!(!milan_list.contains(&m.milan_player));
    assert_eq!(mem::u64_at(m.milan_player + l.p_team), Some(m.senior));
    assert_eq!(mem::u64_at(m.milan_player_contract + 0x10), Some(m.senior));
    assert_eq!(mem::u64_at(m.milan_player_contract + 0x18), Some(m.senior));
    assert_eq!(mem::i32_at(m.milan_player_contract + 0x20), Some(25_000));
    assert_eq!(mem::u8_at(m.milan_player_contract + 0x5D), Some(255), "geen rugnummer");
    assert_eq!(mem::u8_at(m.milan_player_contract + 0x57), Some(0), "van de transferlijst");
    assert_eq!(mem::i32_at(m.finance + 0x0C), Some(20_000_000), "jij betaalt");
    assert_eq!(mem::i32_at(m.finance + 0x7C4), Some(7_000_000));
    assert_eq!(mem::i32_at(m.milan_finance + 0x0C), Some(105_000_000), "AC Milan ontvangt");
    assert_eq!(mem::i32_at(m.milan_finance + 0x7C4), Some(35_000_000));
    let squad = call(json!({"cmd": "squad"})).unwrap();
    assert!(squad.as_array().unwrap().iter().any(|p| p["uid"] == 100), "nu in je selectie");
    let undone = call(json!({"cmd": "undo"})).unwrap();
    assert!(undone["undone"].as_str().unwrap().starts_with("aankoop Milan player 0"));
    assert!(snapshot() == before, "na ongedaan maken is alles weer precies zoals ervoor");

    // wat niet mag
    assert!(err(json!({"cmd": "sell", "uid": 4, "club": 45, "fee": 1})).contains("speler-trainer"));
    assert!(err(json!({"cmd": "sell", "uid": 37058888, "club": 992, "fee": 1})).contains("eigen club"));
    assert!(err(json!({"cmd": "sell", "uid": 37058888, "club": 47, "fee": 1})).contains("vrouwenclubs"));
    assert!(err(json!({"cmd": "sell", "uid": 37058888, "club": 77, "fee": 1})).contains("zit vol"));
    assert!(err(json!({"cmd": "sell", "uid": 37058888, "club": 45, "fee": -5})).contains("transfersom"));
    assert!(snapshot() == before, "geweigerde verkopen schrijven niets");

    // verkopen
    let req = json!({"cmd": "sell", "uid": 37058888, "club": 45, "fee": 10_000_000, "wage": 30_000});
    if call(json!({"cmd": "ping"})).unwrap()["known_build"] == false {
        assert!(err(req.clone()).contains("alleen lezen"));
        assert!(snapshot() == before);
        call(json!({"cmd": "allow_untested", "on": true})).unwrap();
    }
    let sale = call(req).unwrap();
    assert_eq!(sale["done"], true);

    let senior_list = list_items(m.senior + 0x38);
    assert!(!senior_list.contains(&m.wijndal));
    assert_eq!(senior_list.len(), 3, "de anderen blijven, in dezelfde volgorde");
    assert!(list_items(m.u21 + 0x38).is_empty(), "ook uit de U21-lijst");
    let ms = mem::u64_at(mem::u64_at(m.milan + 0x18).unwrap()).unwrap();
    let milan_list = list_items(ms + 0x38);
    assert_eq!(milan_list.len(), 4);
    assert_eq!(milan_list[3], m.wijndal);
    assert_eq!(mem::u64_at(ms + 0x48), mem::u64_at(ms + 0x40), "capaciteit groeit mee binnen het blok");
    assert_eq!(mem::u64_at(m.wijndal + l.p_team), Some(ms));
    let c = m.wijndal_contract;
    assert_eq!(mem::u64_at(c + 0x10), Some(ms));
    assert_eq!(mem::u64_at(c + 0x18), Some(ms));
    assert_eq!(mem::read_bytes(c + 0x44, 4).unwrap(), date_bytes(2027, 76 | 0x4000), "getekend vandaag, vlaggen blijven");
    assert_eq!(mem::read_bytes(c + 0x4C, 4).unwrap(), date_bytes(2027, 76 | 0x4000));
    assert_eq!(mem::i32_at(c + 0x20), Some(30_000));
    assert_eq!(mem::u8_at(c + 0x5D), Some(255), "geen rugnummer");
    assert_eq!(mem::u8_at(c + 0x57), Some(0), "van de transferlijst");
    assert_eq!(mem::u64_at(m.senior + 0x88), Some(0), "aanvoerdersband vrij");
    assert_eq!(mem::u32_at(m.our_selection + 0x28 + 4), Some(u32::MAX), "plek in de opstelling leeg");
    assert_eq!(mem::u32_at(m.our_selection + 0x28), Some(5003), "de rest blijft staan");
    assert_eq!(mem::u32_at(m.national_selection + 0x28 + 4), Some(5001), "nationaal team blijft");
    assert_eq!(mem::i32_at(m.finance + 0x0C), Some(35_000_000));
    assert_eq!(mem::i32_at(m.finance + 0x7C4), Some(22_000_000), "transferbudget groeit met de som");
    assert_eq!(mem::i32_at(m.milan_finance + 0x0C), Some(90_000_000));
    assert_eq!(mem::i32_at(m.milan_finance + 0x7C4), Some(20_000_000));
    let squad = call(json!({"cmd": "squad"})).unwrap();
    assert!(!squad.as_array().unwrap().iter().any(|p| p["uid"] == 37058888), "niet meer in je selectie");

    // ongedaan maken zet alles precies terug
    let undone = call(json!({"cmd": "undo"})).unwrap();
    assert!(undone["undone"].as_str().unwrap().starts_with("verkoop Owen Wijndal"));
    assert!(snapshot() == before, "na ongedaan maken is het geheugen weer precies zoals ervoor");

    // als de game intussen geld heeft geboekt, zet ongedaan maken alleen de transfersom terug
    call(json!({"cmd": "sell", "uid": 37058888, "club": 45, "fee": 10_000_000})).unwrap();
    let now = mem::i32_at(m.finance + 0x0C).unwrap();
    mem::write_checked(m.finance + 0x0C, &now.to_le_bytes(), &(now - 123).to_le_bytes()).unwrap();
    call(json!({"cmd": "undo"})).unwrap();
    assert_eq!(mem::i32_at(m.finance + 0x0C), Some(25_000_000 - 123));
}

#[test]
fn sell_on_mac() {
    let _g = reset(Abi::Itanium);
    let base = 0x1_2000_0000u64;
    with(|s| s.images.push(Image { path: "/x/game_plugin".into(), id: Some("F4540355-2186-39CB-9119-C706A6A35346".into()), start: base, end: base + 0x6000_0000, segments: Vec::new() }));
    let m = build_market(Abi::Itanium, &game::MAC, base, 0x56D2E60, 0x56D2FD0, None);
    run_market(&game::MAC, &m);
}

#[test]
fn sell_on_windows() {
    let _g = reset(Abi::Msvc);
    let base = 0x7FF6_0000_0000u64;
    with(|s| s.images.push(Image { path: r"C:\fm\game_plugin.dll".into(), id: Some("PE-6AE565AF-1821C000".into()), start: base, end: base + 0x1821_C000, segments: Vec::new() }));
    let m = build_market(Abi::Msvc, &game::WIN, base, 0x5705818, 0x5705988, Some(0x56AF378));
    run_market(&game::WIN, &m);
}
