//! Klassen herkennen aan hun RTTI-naam (Itanium-ABI, zoals clang ze op de Mac neerzet):
//! een object begint met zijn vtable-pointer, `[vtable-8]` wijst naar de type_info en
//! `[type_info+8]` naar de gemangelde naam ("N2db16HUMAN_NON_PLAYERE").

use std::collections::{HashMap, HashSet};

use crate::mem::{self, Image};

/// Naam van de klasse van het object op `obj` (via vtable → type_info → naam).
pub fn class_name(obj: u64) -> Option<String> {
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

/// Leesbare naam: "N2db4TEAME" → "db::TEAM".
pub fn demangle(s: &str) -> String {
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

fn data_segments(img: &Image) -> Vec<(u64, u64)> {
    img.segments
        .iter()
        .filter(|s| s.name.starts_with("__DATA") || s.name == "__AUTH_CONST" || s.name == "__AUTH")
        .map(|s| (s.start, s.end))
        .collect()
}

fn text_segments(img: &Image) -> Vec<(u64, u64)> {
    img.segments.iter().filter(|s| s.name == "__TEXT").map(|s| (s.start, s.end)).collect()
}

/// Zoekt alle C-strings die exact `name` zijn (met een 0 ervoor en erna) in de __TEXT van het image.
fn find_name_strings(img: &Image, name: &str) -> Vec<u64> {
    let mut needle = vec![0u8];
    needle.extend_from_slice(name.as_bytes());
    needle.push(0);
    let mut hits = Vec::new();
    for (s, e) in text_segments(img) {
        let mut a = s;
        let chunk = 16 << 20;
        while a < e {
            let n = ((e - a) as usize).min(chunk + needle.len());
            if let Some(buf) = mem::read_bytes(a, n) {
                let mut i = 0;
                while let Some(p) = buf[i..].windows(needle.len()).position(|w| w == needle.as_slice()) {
                    hits.push(a + (i + p) as u64 + 1);
                    i += p + 1;
                }
            }
            if n <= needle.len() {
                break;
            }
            a += (n - needle.len()) as u64;
        }
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}

/// Zoekt 8-byte-waarden in de datasegmenten van het image.
fn find_qwords_in(img: &Image, wanted: &HashSet<u64>) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for (s, e) in data_segments(img) {
        let Some(buf) = mem::read_bytes(s, (e - s) as usize) else { continue };
        for (i, c) in buf.chunks_exact(8).enumerate() {
            let v = u64::from_le_bytes(c.try_into().unwrap());
            let masked = v & !(1u64 << 63);
            if wanted.contains(&v) || wanted.contains(&masked) {
                out.push((s + i as u64 * 8, masked));
            }
        }
    }
    out
}

/// vtable-adressen (waar objecten naar wijzen) van de klasse met deze gemangelde naam.
/// Alleen primaire vtables (offset-to-top 0).
pub fn vtables_of(img: &Image, mangled: &str) -> Vec<u64> {
    let names: HashSet<u64> = find_name_strings(img, mangled).into_iter().collect();
    if names.is_empty() {
        return Vec::new();
    }
    // type_info = {vtable van __class_type_info, naam}
    let tis: HashSet<u64> = find_qwords_in(img, &names).into_iter().map(|(loc, _)| loc - 8).collect();
    if tis.is_empty() {
        return Vec::new();
    }
    let mut vts = Vec::new();
    for (loc, _) in find_qwords_in(img, &tis) {
        // [vtable-8] = type_info, [vtable-16] = offset-to-top
        if mem::u64_at(loc - 8) == Some(0) {
            vts.push(loc + 8);
        }
    }
    vts.sort_unstable();
    vts.dedup();
    vts
}

/// Objecten op de heap waarvan het eerste woord een van deze vtables is.
pub fn instances(vtables: &[u64], limit: usize) -> Vec<u64> {
    let set: HashSet<u64> = vtables.iter().copied().collect();
    mem::scan_u64(&set, limit).into_iter().map(|(a, _)| a).collect()
}

/// Alle klassenamen in het image die een filter bevatten (voor onderzoek), met hun vtables.
pub fn census(img: &Image, filter: &str, max_classes: usize) -> Vec<(String, Vec<u64>)> {
    // type_info's herkennen: hun eerste woord is de vtable van een libc++abi-klasse
    let abi: Vec<u64> = ["_ZTVN10__cxxabiv117__class_type_infoE", "_ZTVN10__cxxabiv120__si_class_type_infoE", "_ZTVN10__cxxabiv121__vmi_class_type_infoE"]
        .iter()
        .filter_map(|s| {
            let c = std::ffi::CString::new(&s[1..]).ok()?;
            let p = unsafe { libc_dlsym(c.as_ptr()) };
            (!p.is_null()).then_some(p as u64 + 16)
        })
        .collect();
    let abi: HashSet<u64> = abi.into_iter().collect();
    let mut by_name: HashMap<String, Vec<u64>> = HashMap::new();
    let mut tis: Vec<(u64, String)> = Vec::new();
    for (loc, _) in find_qwords_in(img, &abi) {
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
    for (loc, ti) in find_qwords_in(img, &ti_set) {
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

unsafe extern "C" {
    #[link_name = "dlsym"]
    fn dlsym_raw(handle: *mut std::ffi::c_void, symbol: *const std::ffi::c_char) -> *mut std::ffi::c_void;
}

unsafe fn libc_dlsym(sym: *const std::ffi::c_char) -> *mut std::ffi::c_void {
    // RTLD_DEFAULT = (void*)-2 op macOS
    unsafe { dlsym_raw(-2isize as *mut std::ffi::c_void, sym) }
}
