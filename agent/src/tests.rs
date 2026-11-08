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

    // de menselijke manager en zijn contract
    let human = h.alloc(0x600);
    let contract = h.alloc(0x80);
    h.w64(human + l.staff_contract + l.human_shift, contract);
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
    World { finance, p1, p2, p1_contract: c1, p1_injuries: inj_rec, p2_absences: rec2 + 0x18, weeks }
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
    assert_eq!(ping["known_build"], false, "Windows is nog niet live getest");
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
