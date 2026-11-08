//! Klassen herkennen aan hun RTTI-naam.
//!
//! - Itanium (clang, macOS): `[vtable-8]` wijst naar de type_info, `[type_info+8]` naar de
//!   gemangelde naam ("N2db16HUMAN_NON_PLAYERE").
//! - MSVC (Windows): `[vtable-8]` wijst naar de Complete Object Locator (signature 1, offset,
//!   cdOffset, RVA van de TypeDescriptor, RVA van de class descriptor, RVA van zichzelf); de
//!   TypeDescriptor heeft de naam op +0x10 (".?AVHUMAN_NON_PLAYER@db@@").

use std::collections::{HashMap, HashSet};

use crate::mem::{self, Abi, Image};

/// De namen waaronder klasse `class` in namespace `ns` in de RTTI staat.
pub fn decorated(ns: &str, class: &str) -> Vec<String> {
    match mem::abi() {
        Abi::Itanium => vec![format!("N{}{}{}{}E", ns.len(), ns, class.len(), class)],
        Abi::Msvc => vec![format!(".?AV{class}@{ns}@@"), format!(".?AU{class}@{ns}@@")],
    }
}

/// Naam van de klasse van het object op `obj`.
pub fn class_name(obj: u64) -> Option<String> {
    match mem::abi() {
        Abi::Itanium => itanium_class_name(obj),
        Abi::Msvc => msvc_class_name(obj),
    }
}

/// Leesbare naam: "N2db4TEAME" of ".?AVTEAM@db@@" → "db::TEAM".
pub fn demangle(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(".?AV").or_else(|| s.strip_prefix(".?AU")) {
        if rest.contains('?') {
            return s.to_string(); // templates e.d.: zoals ze zijn
        }
        let parts: Vec<&str> = rest.trim_end_matches('@').split('@').filter(|p| !p.is_empty()).collect();
        return parts.into_iter().rev().collect::<Vec<_>>().join("::");
    }
    let inner = s.strip_prefix('N').and_then(|x| x.strip_suffix('E')).unwrap_or(s);
    let mut parts = Vec::new();
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let st = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if st == i {
            return s.to_string();
        }
        let n: usize = inner[st..i].parse().unwrap_or(0);
        if i + n > b.len() {
            return s.to_string();
        }
        parts.push(&inner[i..i + n]);
        i += n;
    }
    parts.join("::")
}

/// vtable-adressen (waar objecten naar wijzen) van de klasse met een van deze namen.
/// Alleen primaire vtables (offset 0 in het object).
pub fn vtables_of(img: &Image, names: &[String]) -> Vec<u64> {
    match mem::abi() {
        Abi::Itanium => names.iter().flat_map(|n| itanium_vtables_of(img, n)).collect(),
        Abi::Msvc => msvc_vtables_of(img, names),
    }
}

/// Objecten op de heap waarvan het eerste woord een van deze vtables is.
pub fn instances(vtables: &[u64], limit: usize) -> Vec<u64> {
    let set: HashSet<u64> = vtables.iter().copied().collect();
    mem::scan_u64(&set, limit).into_iter().map(|(a, _)| a).collect()
}

/// Alle klassen in het image waarvan de naam `filter` bevat, met hun vtables (voor onderzoek).
pub fn census(img: &Image, filter: &str, max_classes: usize) -> Vec<(String, Vec<u64>)> {
    match mem::abi() {
        Abi::Itanium => itanium_census(img, filter, max_classes),
        Abi::Msvc => msvc_census(img, filter, max_classes),
    }
}

fn sections<'a>(img: &'a Image, pred: impl Fn(&str) -> bool + 'a) -> impl Iterator<Item = (u64, u64)> + 'a {
    img.segments.iter().filter(move |s| pred(&s.name)).map(|s| (s.start, s.end))
}

/// 8-byte-waarden uit `wanted` in de gegeven gebieden; geeft (adres, waarde zonder bit 63).
fn find_qwords(areas: &[(u64, u64)], wanted: &HashSet<u64>) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let chunk: u64 = 16 << 20;
    for (s, e) in areas {
        let mut a = *s;
        while a < *e {
            let n = (*e - a).min(chunk);
            if let Some(buf) = mem::read_bytes(a, n as usize) {
                for (i, c) in buf.chunks_exact(8).enumerate() {
                    let v = u64::from_le_bytes(c.try_into().unwrap());
                    let masked = v & !(1u64 << 63);
                    if wanted.contains(&v) || wanted.contains(&masked) {
                        out.push((a + i as u64 * 8, masked));
                    }
                }
            }
            a += n;
        }
    }
    out
}

// ---- Itanium (macOS) ------------------------------------------------------------------------

fn itanium_class_name(obj: u64) -> Option<String> {
    let vt = mem::u64_at(obj)?;
    if !mem::is_ptr(vt) {
        return None;
    }
    let ti = mem::u64_at(vt - 8)?;
    if !mem::is_ptr(ti) {
        return None;
    }
    let name = mem::u64_at(ti + 8)? & !(1u64 << 63);
    let s = mem::c_string(name, 128)?;
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_').then_some(s)
}

fn itanium_data(img: &Image) -> Vec<(u64, u64)> {
    sections(img, |n| n.starts_with("__DATA") || n == "__AUTH_CONST" || n == "__AUTH").collect()
}

fn itanium_vtables_of(img: &Image, mangled: &str) -> Vec<u64> {
    let mut needle = vec![0u8];
    needle.extend_from_slice(mangled.as_bytes());
    needle.push(0);
    let names: HashSet<u64> = sections(img, |n| n == "__TEXT").flat_map(|(s, e)| mem::find_in(s, e, &needle)).map(|a| a + 1).collect();
    if names.is_empty() {
        return Vec::new();
    }
    let data = itanium_data(img);
    // type_info = {vtable van __class_type_info, naam}
    let tis: HashSet<u64> = find_qwords(&data, &names).into_iter().map(|(loc, _)| loc - 8).collect();
    if tis.is_empty() {
        return Vec::new();
    }
    let mut vts = Vec::new();
    for (loc, _) in find_qwords(&data, &tis) {
        // [vtable-8] = type_info, [vtable-16] = offset-to-top
        if mem::u64_at(loc - 8) == Some(0) {
            vts.push(loc + 8);
        }
    }
    vts.sort_unstable();
    vts.dedup();
    vts
}

fn itanium_census(img: &Image, filter: &str, max_classes: usize) -> Vec<(String, Vec<u64>)> {
    // type_info's herkennen: hun eerste woord is de vtable van een libc++abi-klasse
    let abi: HashSet<u64> = cxxabi_vtables().into_iter().collect();
    let data = itanium_data(img);
    let mut tis: Vec<(u64, String)> = Vec::new();
    for (loc, _) in find_qwords(&data, &abi) {
        let Some(np) = mem::u64_at(loc + 8) else { continue };
        let Some(name) = mem::c_string(np & !(1u64 << 63), 160) else { continue };
        if name.contains(filter) {
            tis.push((loc, name));
        }
        if tis.len() >= max_classes {
            break;
        }
    }
    let ti_set: HashSet<u64> = tis.iter().map(|(a, _)| *a).collect();
    let ti_name: HashMap<u64, String> = tis.into_iter().collect();
    let mut by_name: HashMap<String, Vec<u64>> = HashMap::new();
    for (loc, ti) in find_qwords(&data, &ti_set) {
        if mem::u64_at(loc - 8) == Some(0) {
            if let Some(n) = ti_name.get(&ti) {
                by_name.entry(n.clone()).or_default().push(loc + 8);
            }
        }
    }
    let mut v: Vec<(String, Vec<u64>)> = by_name.into_iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[cfg(all(target_os = "macos", not(test)))]
fn cxxabi_vtables() -> Vec<u64> {
    unsafe extern "C" {
        fn dlsym(handle: *mut std::ffi::c_void, symbol: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    }
    ["_ZTVN10__cxxabiv117__class_type_infoE", "_ZTVN10__cxxabiv120__si_class_type_infoE", "_ZTVN10__cxxabiv121__vmi_class_type_infoE"]
        .iter()
        .filter_map(|s| {
            let c = std::ffi::CString::new(*s).ok()?;
            // dlsym wil de C-naam: één underscore (in de symbooltabel staan er twee); RTLD_DEFAULT = (void*)-2
            let p = unsafe { dlsym(-2isize as *mut std::ffi::c_void, c.as_ptr()) };
            (!p.is_null()).then_some(p as u64 + 16)
        })
        .collect()
}

#[cfg(not(all(target_os = "macos", not(test))))]
fn cxxabi_vtables() -> Vec<u64> {
    Vec::new()
}

// ---- MSVC (Windows) -------------------------------------------------------------------------

fn msvc_class_name(obj: u64) -> Option<String> {
    let vt = mem::u64_at(obj)?;
    if !mem::is_ptr(vt) {
        return None;
    }
    let col = mem::u64_at(vt - 8)?;
    if !(0x1_0000..0x0000_8000_0000_0000).contains(&col) || col % 4 != 0 {
        return None;
    }
    if mem::u32_at(col)? != 1 {
        return None; // signature van een 64-bit locator
    }
    let td_rva = mem::u32_at(col + 12)? as u64;
    let self_rva = mem::u32_at(col + 20)? as u64;
    let base = col.checked_sub(self_rva)?;
    let name = mem::c_string(base + td_rva + 16, 256)?;
    (name.starts_with(".?A") && name.is_ascii()).then_some(name)
}

/// Complete Object Locators in .rdata die aan `pred(td_rva, offset)` voldoen: (adres, td_rva).
fn msvc_locators(img: &Image, pred: impl Fn(u32, u32) -> bool) -> Vec<(u64, u32)> {
    let base = img.start;
    let mut out = Vec::new();
    let chunk: u64 = 16 << 20;
    for (s, e) in sections(img, |n| n == ".rdata") {
        let mut a = s;
        while a < e {
            let n = (e - a).min(chunk + 24);
            if let Some(buf) = mem::read_bytes(a, n as usize) {
                let u = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
                let mut i = ((4 - (a % 4)) % 4) as usize;
                while i + 24 <= buf.len() {
                    if u(i) == 1 {
                        let pos = a + i as u64;
                        let (offset, td, me) = (u(i + 4), u(i + 12), u(i + 20));
                        if me as u64 == pos - base && pred(td, offset) {
                            out.push((pos, td));
                        }
                    }
                    i += 4;
                }
            }
            if n <= 24 {
                break;
            }
            a += n - 24;
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// vtables die naar deze locators wijzen (`[vtable-8]` = locator).
fn msvc_vtables_for(img: &Image, locators: &HashSet<u64>) -> Vec<(u64, u64)> {
    let rdata: Vec<(u64, u64)> = sections(img, |n| n == ".rdata").collect();
    find_qwords(&rdata, locators).into_iter().map(|(loc, col)| (loc + 8, col)).collect()
}

fn msvc_vtables_of(img: &Image, names: &[String]) -> Vec<u64> {
    let base = img.start;
    let data: Vec<(u64, u64)> = sections(img, |n| n == ".data" || n == ".rdata").collect();
    let mut tds: HashSet<u32> = HashSet::new();
    for n in names {
        let mut needle = n.as_bytes().to_vec();
        needle.push(0);
        for (s, e) in &data {
            for hit in mem::find_in(*s, *e, &needle) {
                let td = hit - 16;
                if td % 8 == 0 && td >= base {
                    tds.insert((td - base) as u32);
                }
            }
        }
    }
    if tds.is_empty() {
        return Vec::new();
    }
    let cols: HashSet<u64> = msvc_locators(img, |td, off| off == 0 && tds.contains(&td)).into_iter().map(|(a, _)| a).collect();
    let mut vts: Vec<u64> = msvc_vtables_for(img, &cols).into_iter().map(|(vt, _)| vt).collect();
    vts.sort_unstable();
    vts.dedup();
    vts
}

fn msvc_census(img: &Image, filter: &str, max_classes: usize) -> Vec<(String, Vec<u64>)> {
    let base = img.start;
    let mut td_name: HashMap<u32, String> = HashMap::new();
    for (s, e) in sections(img, |n| n == ".data" || n == ".rdata") {
        for prefix in [b".?AV", b".?AU"] {
            for hit in mem::find_in(s, e, prefix) {
                if td_name.len() >= max_classes {
                    break;
                }
                let td = hit - 16;
                if td % 8 != 0 || td < base {
                    continue;
                }
                if let Some(name) = mem::c_string(hit, 256) {
                    if name.contains(filter) && name.is_ascii() {
                        td_name.insert((td - base) as u32, name);
                    }
                }
            }
        }
    }
    let cols = msvc_locators(img, |td, off| off == 0 && td_name.contains_key(&td));
    let col_td: HashMap<u64, u32> = cols.iter().copied().collect();
    let col_set: HashSet<u64> = col_td.keys().copied().collect();
    let mut by_name: HashMap<String, Vec<u64>> = HashMap::new();
    for (vt, col) in msvc_vtables_for(img, &col_set) {
        if let Some(n) = col_td.get(&col).and_then(|td| td_name.get(td)) {
            by_name.entry(n.clone()).or_default().push(vt);
        }
    }
    let mut v: Vec<(String, Vec<u64>)> = by_name.into_iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}
