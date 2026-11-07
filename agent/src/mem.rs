//! Geheugen van het eigen proces (FM27), veilig gelezen en geschreven via de Mach-API:
//! een ongeldig adres geeft een foutcode in plaats van een crash.

use std::ffi::{CStr, c_char};

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    static mach_task_self_: u32;
    fn mach_vm_read_overwrite(task: u32, address: u64, size: u64, data: u64, outsize: *mut u64) -> i32;
    fn mach_vm_write(task: u32, address: u64, data: usize, count: u32) -> i32;
    fn mach_vm_region(
        task: u32,
        address: *mut u64,
        size: *mut u64,
        flavor: i32,
        info: *mut i32,
        count: *mut u32,
        object_name: *mut u32,
    ) -> i32;
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_header(i: u32) -> *const u8;
    fn _dyld_get_image_vmaddr_slide(i: u32) -> isize;
    fn _dyld_get_image_name(i: u32) -> *const c_char;
}

fn task() -> u32 {
    unsafe { mach_task_self_ }
}

pub fn read_into(addr: u64, buf: &mut [u8]) -> bool {
    if addr < 0x1000 || buf.is_empty() {
        return false;
    }
    let mut out = 0u64;
    let kr = unsafe { mach_vm_read_overwrite(task(), addr, buf.len() as u64, buf.as_mut_ptr() as u64, &mut out) };
    kr == 0 && out == buf.len() as u64
}

pub fn read_bytes(addr: u64, len: usize) -> Option<Vec<u8>> {
    let mut v = vec![0u8; len];
    read_into(addr, &mut v).then_some(v)
}

pub fn u8_at(addr: u64) -> Option<u8> {
    let mut b = [0u8; 1];
    read_into(addr, &mut b).then_some(b[0])
}
pub fn u16_at(addr: u64) -> Option<u16> {
    let mut b = [0u8; 2];
    read_into(addr, &mut b).then(|| u16::from_le_bytes(b))
}
pub fn i16_at(addr: u64) -> Option<i16> {
    u16_at(addr).map(|v| v as i16)
}
pub fn u32_at(addr: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read_into(addr, &mut b).then(|| u32::from_le_bytes(b))
}
pub fn i32_at(addr: u64) -> Option<i32> {
    u32_at(addr).map(|v| v as i32)
}
pub fn f32_at(addr: u64) -> Option<f32> {
    u32_at(addr).map(f32::from_bits)
}
pub fn u64_at(addr: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read_into(addr, &mut b).then(|| u64::from_le_bytes(b))
}

/// Een pointer die naar gebruikersgeheugen kan wijzen (geen null, geen kleine getallen, geen kernel).
pub fn is_ptr(p: u64) -> bool {
    (0x1_0000..0x0000_8000_0000_0000).contains(&p) && p % 8 == 0
}

/// Schrijft alleen als het geheugen nu nog `expect` bevat. Geeft de oude bytes terug.
pub fn write_checked(addr: u64, expect: &[u8], new: &[u8]) -> Result<(), String> {
    if expect.len() != new.len() {
        return Err("lengte klopt niet".into());
    }
    let now = read_bytes(addr, expect.len()).ok_or_else(|| format!("{addr:#x} niet leesbaar"))?;
    if now != expect {
        return Err(format!("{addr:#x} is intussen veranderd; niets geschreven"));
    }
    let kr = unsafe { mach_vm_write(task(), addr, new.as_ptr() as usize, new.len() as u32) };
    if kr != 0 {
        return Err(format!("schrijven naar {addr:#x} mislukt (kr={kr})"));
    }
    match read_bytes(addr, new.len()) {
        Some(b) if b == new => Ok(()),
        _ => Err(format!("schrijven naar {addr:#x} niet bevestigd")),
    }
}

pub fn fm_string(ptr: u64) -> Option<String> {
    if !is_ptr(ptr & !7) || ptr == 0 {
        return None;
    }
    let len = u32_at(ptr)? as usize;
    if len == 0 || len > 1024 {
        return None;
    }
    let b = read_bytes(ptr + 4, len)?;
    String::from_utf8(b).ok()
}

pub fn c_string(addr: u64, max: usize) -> Option<String> {
    let b = read_bytes(addr, max)?;
    let end = b.iter().position(|c| *c == 0)?;
    String::from_utf8(b[..end].to_vec()).ok()
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub name: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub path: String,
    pub uuid: Option<String>,
    pub start: u64,
    pub end: u64,
    pub segments: Vec<Segment>,
}

/// Geladen Mach-O-images met hun segmenten (adressen inclusief slide).
pub fn images() -> Vec<Image> {
    let mut out = Vec::new();
    let n = unsafe { _dyld_image_count() };
    for i in 0..n {
        let name = unsafe { _dyld_get_image_name(i) };
        if name.is_null() {
            continue;
        }
        let path = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
        let header = unsafe { _dyld_get_image_header(i) } as u64;
        let slide = unsafe { _dyld_get_image_vmaddr_slide(i) } as i64 as u64;
        let Some(ncmds) = u32_at(header + 16) else { continue };
        let mut lc = header + 32;
        let mut segments = Vec::new();
        let mut uuid = None;
        for _ in 0..ncmds.min(512) {
            let (Some(cmd), Some(size)) = (u32_at(lc), u32_at(lc + 4)) else { break };
            if cmd == 0x1B {
                // LC_UUID
                if let Some(b) = read_bytes(lc + 8, 16) {
                    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
                    uuid = Some(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]));
                }
            }
            if cmd == 0x19 {
                let name = c_string(lc + 8, 17).unwrap_or_default();
                let vmaddr = u64_at(lc + 24).unwrap_or(0);
                let vmsize = u64_at(lc + 32).unwrap_or(0);
                if name != "__PAGEZERO" && vmsize > 0 {
                    let start = vmaddr.wrapping_add(slide);
                    segments.push(Segment { name, start, end: start + vmsize });
                }
            }
            if size == 0 {
                break;
            }
            lc += size as u64;
        }
        let start = segments.iter().map(|s| s.start).min().unwrap_or(0);
        let end = segments.iter().map(|s| s.end).max().unwrap_or(0);
        out.push(Image { path, uuid, start, end, segments });
    }
    out
}

pub fn game_plugin() -> Option<Image> {
    images().into_iter().find(|i| i.path.ends_with("/game_plugin"))
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: u64,
    pub end: u64,
    pub prot: i32,
}

/// Alle geheugengebieden met hun bescherming.
pub fn regions() -> Vec<Region> {
    let mut out = Vec::new();
    let mut addr: u64 = 0;
    loop {
        let mut size: u64 = 0;
        let mut info = [0i32; 12];
        let mut count: u32 = 9;
        let mut obj: u32 = 0;
        let kr = unsafe { mach_vm_region(task(), &mut addr, &mut size, 9, info.as_mut_ptr(), &mut count, &mut obj) };
        if kr != 0 {
            break;
        }
        out.push(Region { start: addr, end: addr + size, prot: info[0] });
        addr += size;
        if out.len() > 2_000_000 {
            break;
        }
    }
    out
}

/// Beschrijfbare heap-gebieden (lezen + schrijven), buiten de geladen images.
pub fn heap_regions(exclude: &[(u64, u64)]) -> Vec<Region> {
    regions()
        .into_iter()
        .filter(|r| r.prot & 3 == 3)
        .filter(|r| r.end - r.start <= 8 << 30)
        .filter(|r| !exclude.iter().any(|(s, e)| r.start < *e && r.end > *s))
        .collect()
}

/// Zoekt 8-byte-uitgelijnde u64-waarden uit `wanted` in de heap; geeft (adres, waarde).
pub fn scan_u64(wanted: &std::collections::HashSet<u64>, limit: usize) -> Vec<(u64, u64)> {
    let (lo, hi) = match (wanted.iter().min(), wanted.iter().max()) {
        (Some(a), Some(b)) => (*a, *b),
        _ => return Vec::new(),
    };
    let images: Vec<(u64, u64)> = images().iter().map(|i| (i.start, i.end)).collect();
    let mut hits = Vec::new();
    let mut buf = vec![0u8; 8 << 20];
    for r in heap_regions(&images) {
        let mut a = r.start;
        while a < r.end {
            let n = ((r.end - a) as usize).min(buf.len());
            if read_into(a, &mut buf[..n]) {
                for (i, c) in buf[..n].chunks_exact(8).enumerate() {
                    let v = u64::from_le_bytes(c.try_into().unwrap());
                    if v >= lo && v <= hi && wanted.contains(&v) {
                        hits.push((a + i as u64 * 8, v));
                        if hits.len() >= limit {
                            return hits;
                        }
                    }
                }
            }
            a += n as u64;
        }
    }
    hits
}

/// Zoekt een bytepatroon (2-byte-uitgelijnd) in de heap.
pub fn scan_bytes(pattern: &[u8], align: usize, limit: usize) -> Vec<u64> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let images: Vec<(u64, u64)> = images().iter().map(|i| (i.start, i.end)).collect();
    let mut hits = Vec::new();
    let chunk = 8 << 20;
    let mut buf = vec![0u8; chunk + pattern.len()];
    for r in heap_regions(&images) {
        let mut a = r.start;
        while a < r.end {
            let n = ((r.end - a) as usize).min(chunk + pattern.len());
            if read_into(a, &mut buf[..n]) {
                let mut i = 0;
                while i + pattern.len() <= n {
                    if buf[i] == pattern[0] && buf[i..i + pattern.len()] == *pattern {
                        hits.push(a + i as u64);
                        if hits.len() >= limit {
                            return hits;
                        }
                    }
                    i += align.max(1);
                }
            }
            if n <= pattern.len() {
                break;
            }
            a += (n - pattern.len()) as u64;
        }
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}
