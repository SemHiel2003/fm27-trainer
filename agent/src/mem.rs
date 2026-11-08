//! Het geheugen van FM27, veilig gelezen en geschreven: een ongeldig adres geeft een fout in
//! plaats van een crash.
//!
//! - macOS: de module draait ín FM; het eigen proces via de Mach-API.
//! - Windows: de app leest fm.exe van buitenaf (ReadProcessMemory / WriteProcessMemory).
//! - tests: een gesimuleerd geheugen, zodat de spellogica zonder FM te testen is.

/// Hoe de C++-compiler van FM klassen en RTTI opslaat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Abi {
    /// clang op macOS
    Itanium,
    /// MSVC op Windows
    Msvc,
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
    /// Wat de build identificeert: LC_UUID op macOS, "PE-<linktijd>-<grootte>" op Windows.
    pub id: Option<String>,
    pub start: u64,
    pub end: u64,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: u64,
    pub end: u64,
    /// 1 lezen, 2 schrijven, 4 uitvoeren
    pub prot: i32,
    /// privé geheugen van het proces (de heap), geen gemapt bestand
    pub private: bool,
}

pub use sys::{abi, images, pid, read_into, regions};
#[cfg(any(windows, test))]
pub use sys::{attach, find_fm};

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

/// Schrijft alleen als het geheugen nu nog `expect` bevat, en controleert het resultaat.
pub fn write_checked(addr: u64, expect: &[u8], new: &[u8]) -> Result<(), String> {
    if expect.len() != new.len() {
        return Err("lengte klopt niet".into());
    }
    let now = read_bytes(addr, expect.len()).ok_or_else(|| format!("{addr:#x} niet leesbaar"))?;
    if now != expect {
        return Err(format!("{addr:#x} is intussen veranderd; niets geschreven"));
    }
    if !sys::write_raw(addr, new) {
        return Err(format!("schrijven naar {addr:#x} mislukt"));
    }
    match read_bytes(addr, new.len()) {
        Some(b) if b == new => Ok(()),
        _ => Err(format!("schrijven naar {addr:#x} niet bevestigd")),
    }
}

/// FM-string: u32 lengte, dan UTF-8.
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
    // aan het eind van een gebied kan een lange leesactie mislukken: dan korter proberen
    let b = read_bytes(addr, max).or_else(|| read_bytes(addr, 64))?;
    let end = b.iter().position(|c| *c == 0)?;
    String::from_utf8(b[..end].to_vec()).ok()
}

/// De native kern van FM27: game_plugin (macOS) of game_plugin.dll (Windows).
pub fn game_plugin() -> Option<Image> {
    images().into_iter().find(|i| {
        let file = i.path.rsplit(['/', '\\']).next().unwrap_or("");
        file.eq_ignore_ascii_case("game_plugin") || file.eq_ignore_ascii_case("game_plugin.dll")
    })
}

/// Secties van een PE-image (Windows) en zijn build-id, gelezen uit de headers in het geheugen.
#[allow(dead_code)]
pub fn pe_layout(base: u64) -> Option<(Vec<Segment>, String)> {
    if u16_at(base)? != 0x5A4D {
        return None; // "MZ"
    }
    let nt = base + u32_at(base + 0x3C)? as u64;
    if u32_at(nt)? != 0x4550 {
        return None; // "PE\0\0"
    }
    let nsec = u16_at(nt + 6)?;
    let stamp = u32_at(nt + 8)?;
    let opt_size = u16_at(nt + 20)? as u64;
    let opt = nt + 24;
    if u16_at(opt)? != 0x20B {
        return None; // alleen PE32+ (64-bit)
    }
    let size_of_image = u32_at(opt + 56)?;
    let mut segs = Vec::new();
    for i in 0..nsec.min(96) as u64 {
        let s = opt + opt_size + i * 40;
        let raw = read_bytes(s, 8)?;
        let name: String = raw.iter().take_while(|c| **c != 0).map(|c| *c as char).collect();
        let vsize = u32_at(s + 8)? as u64;
        let va = u32_at(s + 12)? as u64;
        segs.push(Segment { name, start: base + va, end: base + va + vsize });
    }
    Some((segs, format!("PE-{stamp:08X}-{size_of_image:X}")))
}

/// Beschrijfbare heap-gebieden (lezen + schrijven, privé), buiten de geladen images.
pub fn heap_regions(exclude: &[(u64, u64)]) -> Vec<Region> {
    regions()
        .into_iter()
        .filter(|r| r.prot & 3 == 3 && r.private)
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

/// Zoekt een bytepatroon (met uitlijning `align`) in de heap.
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

/// Zoekt `needle` in [start, end) van een image (in blokken, met overlap). Geeft de adressen.
pub fn find_in(start: u64, end: u64, needle: &[u8]) -> Vec<u64> {
    let mut hits = Vec::new();
    if needle.is_empty() || end <= start {
        return hits;
    }
    let chunk = 16 << 20;
    let mut a = start;
    while a < end {
        let n = ((end - a) as usize).min(chunk + needle.len());
        if let Some(buf) = read_bytes(a, n) {
            let mut i = 0;
            while let Some(p) = buf[i..].windows(needle.len()).position(|w| w == needle) {
                hits.push(a + (i + p) as u64);
                i += p + 1;
            }
        }
        if n <= needle.len() {
            break;
        }
        a += (n - needle.len()) as u64;
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}

// ---- macOS: het eigen proces -------------------------------------------------------------

#[cfg(all(target_os = "macos", not(test)))]
mod sys {
    use super::{Abi, Image, Region, Segment, c_string, read_bytes, u32_at, u64_at};
    use std::ffi::{CStr, c_char};

    #[allow(non_upper_case_globals)]
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn mach_vm_read_overwrite(task: u32, address: u64, size: u64, data: u64, outsize: *mut u64) -> i32;
        fn mach_vm_write(task: u32, address: u64, data: usize, count: u32) -> i32;
        fn mach_vm_region(task: u32, address: *mut u64, size: *mut u64, flavor: i32, info: *mut i32, count: *mut u32, object_name: *mut u32) -> i32;
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_header(i: u32) -> *const u8;
        fn _dyld_get_image_vmaddr_slide(i: u32) -> isize;
        fn _dyld_get_image_name(i: u32) -> *const c_char;
    }

    fn task() -> u32 {
        unsafe { mach_task_self_ }
    }

    pub fn abi() -> Abi {
        Abi::Itanium
    }

    pub fn pid() -> u32 {
        std::process::id()
    }

    pub fn read_into(addr: u64, buf: &mut [u8]) -> bool {
        if addr < 0x1000 || buf.is_empty() {
            return false;
        }
        let mut out = 0u64;
        let kr = unsafe { mach_vm_read_overwrite(task(), addr, buf.len() as u64, buf.as_mut_ptr() as u64, &mut out) };
        kr == 0 && out == buf.len() as u64
    }

    pub fn write_raw(addr: u64, data: &[u8]) -> bool {
        unsafe { mach_vm_write(task(), addr, data.as_ptr() as usize, data.len() as u32) == 0 }
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
            let mut id = None;
            for _ in 0..ncmds.min(512) {
                let (Some(cmd), Some(size)) = (u32_at(lc), u32_at(lc + 4)) else { break };
                if cmd == 0x1B {
                    // LC_UUID
                    if let Some(b) = read_bytes(lc + 8, 16) {
                        let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
                        id = Some(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]));
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
            out.push(Image { path, id, start, end, segments });
        }
        out
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
            out.push(Region { start: addr, end: addr + size, prot: info[0], private: true });
            addr += size;
            if out.len() > 2_000_000 {
                break;
            }
        }
        out
    }
}

// ---- Windows: fm.exe van buitenaf ----------------------------------------------------------

#[cfg(all(windows, not(test)))]
mod sys {
    use super::{Abi, Image, Region, pe_layout};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HMODULE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_PRIVATE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
        PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY, VirtualQueryEx,
    };
    use windows_sys::Win32::System::ProcessStatus::{EnumProcessModulesEx, GetModuleFileNameExW, GetModuleInformation, LIST_MODULES_ALL, MODULEINFO};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    static HANDLE_: AtomicUsize = AtomicUsize::new(0);
    static PID: AtomicU32 = AtomicU32::new(0);
    const STILL_ACTIVE: u32 = 259;

    fn handle() -> HANDLE {
        HANDLE_.load(Ordering::Acquire) as HANDLE
    }

    pub fn abi() -> Abi {
        Abi::Msvc
    }

    pub fn pid() -> u32 {
        PID.load(Ordering::Acquire)
    }

    /// pid van fm.exe, als FM27 draait.
    pub fn find_fm() -> Option<u32> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return None;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                    if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case("fm.exe") {
                        found = Some(e.th32ProcessID);
                        break;
                    }
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            found
        }
    }

    fn alive(h: HANDLE) -> bool {
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE }
    }

    /// Verbindt met fm.exe, opnieuw als FM intussen opnieuw is gestart.
    /// Geeft (pid, of het een nieuwe verbinding is).
    pub fn attach() -> Result<(u32, bool), String> {
        let h = handle();
        if alive(h) {
            return Ok((pid(), false));
        }
        if !h.is_null() {
            unsafe { CloseHandle(h) };
            HANDLE_.store(0, Ordering::Release);
            PID.store(0, Ordering::Release);
        }
        let pid = find_fm().ok_or("FM27 (fm.exe) draait niet / FM27 (fm.exe) is not running")?;
        let full = PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION;
        let mut h = unsafe { OpenProcess(full, 0, pid) };
        if h.is_null() {
            // dan in elk geval lezen
            h = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
        }
        if h.is_null() {
            let e = unsafe { GetLastError() };
            return Err(format!(
                "fm.exe (pid {pid}) niet te openen (fout {e}). Draait FM als administrator? / Cannot open fm.exe (error {e}). Is FM running as administrator?"
            ));
        }
        HANDLE_.store(h as usize, Ordering::Release);
        PID.store(pid, Ordering::Release);
        Ok((pid, true))
    }

    pub fn read_into(addr: u64, buf: &mut [u8]) -> bool {
        let h = handle();
        if addr < 0x1000 || buf.is_empty() || h.is_null() {
            return false;
        }
        let mut n = 0usize;
        let ok = unsafe { ReadProcessMemory(h, addr as *const c_void, buf.as_mut_ptr() as *mut c_void, buf.len(), &mut n) };
        ok != 0 && n == buf.len()
    }

    pub fn write_raw(addr: u64, data: &[u8]) -> bool {
        let h = handle();
        if h.is_null() {
            return false;
        }
        let mut n = 0usize;
        let ok = unsafe { WriteProcessMemory(h, addr as *const c_void, data.as_ptr() as *const c_void, data.len(), &mut n) };
        ok != 0 && n == data.len()
    }

    pub fn images() -> Vec<Image> {
        let h = handle();
        let mut out = Vec::new();
        if h.is_null() {
            return out;
        }
        let mut mods: Vec<HMODULE> = vec![std::ptr::null_mut(); 2048];
        let mut needed = 0u32;
        let ok = unsafe { EnumProcessModulesEx(h, mods.as_mut_ptr(), (mods.len() * std::mem::size_of::<HMODULE>()) as u32, &mut needed, LIST_MODULES_ALL) };
        if ok == 0 {
            return out;
        }
        let n = (needed as usize / std::mem::size_of::<HMODULE>()).min(mods.len());
        for m in &mods[..n] {
            let mut name = [0u16; 1024];
            let len = unsafe { GetModuleFileNameExW(h, *m, name.as_mut_ptr(), name.len() as u32) } as usize;
            let path = String::from_utf16_lossy(&name[..len.min(name.len())]);
            let mut mi: MODULEINFO = unsafe { std::mem::zeroed() };
            if unsafe { GetModuleInformation(h, *m, &mut mi, std::mem::size_of::<MODULEINFO>() as u32) } == 0 {
                continue;
            }
            let start = mi.lpBaseOfDll as u64;
            let end = start + mi.SizeOfImage as u64;
            let (segments, id) = match pe_layout(start) {
                Some((s, id)) => (s, Some(id)),
                None => (Vec::new(), None),
            };
            out.push(Image { path, id, start, end, segments });
        }
        out
    }

    pub fn regions() -> Vec<Region> {
        let h = handle();
        let mut out = Vec::new();
        if h.is_null() {
            return out;
        }
        let mut addr: u64 = 0x1_0000;
        loop {
            let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
            let r = unsafe { VirtualQueryEx(h, addr as *const c_void, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
            if r == 0 {
                break;
            }
            let base = mbi.BaseAddress as u64;
            let size = mbi.RegionSize as u64;
            if mbi.State == MEM_COMMIT && mbi.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0 {
                let p = mbi.Protect & 0xFF;
                let prot = if p == PAGE_READONLY {
                    1
                } else if p == PAGE_READWRITE || p == PAGE_WRITECOPY {
                    3
                } else if p == PAGE_EXECUTE {
                    4
                } else if p == PAGE_EXECUTE_READ {
                    5
                } else if p == PAGE_EXECUTE_READWRITE || p == PAGE_EXECUTE_WRITECOPY {
                    7
                } else {
                    0
                };
                out.push(Region { start: base, end: base + size, prot, private: mbi.Type == MEM_PRIVATE });
            }
            let next = base.saturating_add(size);
            if next <= addr || next >= 0x7FFF_FFFF_0000 || out.len() > 4_000_000 {
                break;
            }
            addr = next;
        }
        out
    }
}

// ---- tests: gesimuleerd geheugen ----------------------------------------------------------

#[cfg(test)]
pub mod sys {
    use super::{Abi, Image, Region};
    use std::cell::RefCell;

    pub struct Block {
        pub start: u64,
        pub data: Vec<u8>,
        pub private: bool,
    }

    pub struct Sim {
        pub abi: Abi,
        pub blocks: Vec<Block>,
        pub images: Vec<Image>,
    }

    thread_local! {
        pub static SIM: RefCell<Sim> = RefCell::new(Sim { abi: Abi::Itanium, blocks: Vec::new(), images: Vec::new() });
    }

    pub fn with<R>(f: impl FnOnce(&mut Sim) -> R) -> R {
        SIM.with(|s| f(&mut s.borrow_mut()))
    }

    pub fn abi() -> Abi {
        with(|s| s.abi)
    }

    pub fn pid() -> u32 {
        4242
    }

    pub fn find_fm() -> Option<u32> {
        Some(4242)
    }

    pub fn attach() -> Result<(u32, bool), String> {
        Ok((4242, false))
    }

    pub fn read_into(addr: u64, buf: &mut [u8]) -> bool {
        with(|s| {
            for b in &s.blocks {
                if addr >= b.start && addr + buf.len() as u64 <= b.start + b.data.len() as u64 {
                    let o = (addr - b.start) as usize;
                    buf.copy_from_slice(&b.data[o..o + buf.len()]);
                    return true;
                }
            }
            false
        })
    }

    pub fn write_raw(addr: u64, data: &[u8]) -> bool {
        with(|s| {
            for b in &mut s.blocks {
                if addr >= b.start && addr + data.len() as u64 <= b.start + b.data.len() as u64 {
                    let o = (addr - b.start) as usize;
                    b.data[o..o + data.len()].copy_from_slice(data);
                    return true;
                }
            }
            false
        })
    }

    pub fn images() -> Vec<Image> {
        with(|s| s.images.clone())
    }

    pub fn regions() -> Vec<Region> {
        with(|s| s.blocks.iter().map(|b| Region { start: b.start, end: b.start + b.data.len() as u64, prot: 3, private: b.private }).collect())
    }
}
