//! Omweg voor een fout in Wine (en dus Proton), alleen actief als de app onder Wine draait.
//!
//! **De fout.** `RedrawWindow(.., RDW_INTERNALPAINT)` verhoogt in wineserver de tekenteller van de
//! berichtenwachtrij. Bij het afgeven van de WM_PAINT wist `find_window_to_repaint` (server/window.c) de
//! vlag `PAINT_INTERNAL` zonder de teller weer te verlagen. Daarna komt de teller nooit meer op nul en blijft
//! `QS_PAINT` voorgoed aan, ook al hoeft geen enkel venster getekend te worden (geen enkele
//! `RedrawWindow(RDW_VALIDATE | RDW_NOINTERNALPAINT)` haalt hem nog weg).
//!
//! **Waarom de app daardoor rondraaide.** winit (onder eframe) vraagt bij elk bericht aan zijn
//! verborgen hulpvenster en bij elke `request_redraw` zo'n interne teken-opdracht aan, en wacht daarna met
//! `MsgWaitForMultipleObjectsEx(.., QS_ALLINPUT, MWMO_INPUTAVAILABLE)`. Die vlag kijkt naar wat er nú in de
//! wachtrij staat, dus de blijvende `QS_PAINT` maakt dat het wachten meteen terugkomt, `PeekMessage` niets
//! vindt, en zo verder: tienduizenden keren per seconde. Gemeten (26 nov 2026, Proton Experimental 11.0-100
//! en GE-Proton 11-12): UI-thread ~55 % van een kern, wineserver ~50 % en ~100k verzoeken/s, bij één beeld
//! per seconde. Gewone `GetMessage`-lussen wachten op *nieuwe* invoer en merken er niets van.
//!
//! **De omweg.** Onder Wine wordt `RedrawWindow` in de importtabel van deze exe omgeleid: een verzoek met
//! alleen `RDW_INTERNALPAINT` wordt `RDW_INVALIDATE | RDW_NOERASE`. Het venster krijgt precies dezelfde ene
//! WM_PAINT, maar via een bijwerkgebied dat wineserver wél netjes telt; winit's WM_PAINT-afhandeling
//! (`DefWindowProcW`/`ValidateRect`) maakt het gebied weer leeg en de teller staat weer op nul. Op echt
//! Windows wordt niets omgeleid.
//!
//! Minimale nabouw van de fout (een klein Win32-programma met de meetresultaten): `tools/research/wine_paintleak.c`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

const RDW_INVALIDATE: u32 = 0x0001;
const RDW_INTERNALPAINT: u32 = 0x0002;
const RDW_VALIDATE: u32 = 0x0008;
const RDW_NOERASE: u32 = 0x0020;
const PAGE_READWRITE: u32 = 0x04;

type RedrawWindowFn = unsafe extern "system" fn(isize, *const c_void, isize, u32) -> i32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
}

/// Het echte `RedrawWindow` uit user32.
static REAL: AtomicUsize = AtomicUsize::new(0);

/// Een verzoek met alleen `RDW_INTERNALPAINT` (zonder (in)validatie) wordt een getelde invalidatie zonder
/// achtergrond wissen; al het andere blijft zoals het is.
fn translate(flags: u32) -> u32 {
    if flags & RDW_INTERNALPAINT != 0 && flags & (RDW_INVALIDATE | RDW_VALIDATE) == 0 {
        (flags & !RDW_INTERNALPAINT) | RDW_INVALIDATE | RDW_NOERASE
    } else {
        flags
    }
}

unsafe extern "system" fn redraw_window(hwnd: isize, rect: *const c_void, rgn: isize, flags: u32) -> i32 {
    let real: RedrawWindowFn = unsafe { std::mem::transmute(REAL.load(Ordering::Relaxed)) };
    unsafe { real(hwnd, rect, rgn, translate(flags)) }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Draait de app onder Wine? (ntdll van Wine exporteert `wine_get_version`, die van Windows niet.)
pub fn running_under_wine() -> bool {
    unsafe {
        let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        ntdll != 0 && !GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_null()
    }
}

/// Zet de omweg aan als de app onder Wine draait. Aanroepen vóór er vensters zijn (dus vóór `run_native`).
/// Geeft terug hoeveel importplekken zijn omgeleid (0 op echt Windows).
pub fn install() -> usize {
    if !running_under_wine() {
        return 0;
    }
    unsafe {
        let user32 = LoadLibraryW(wide("user32.dll").as_ptr());
        let real = GetProcAddress(user32, c"RedrawWindow".as_ptr().cast());
        if user32 == 0 || real.is_null() {
            return 0;
        }
        REAL.store(real as usize, Ordering::Relaxed);
        patch_imports(GetModuleHandleW(std::ptr::null()) as *mut u8, real as usize, redraw_window as *const () as usize)
    }
}

/// Vervangt in de importtabel (IAT) van deze exe elke plek die naar `from` wijst door `to`. Er wordt op
/// adres vergeleken en niet op naam: de exe importeert user32 via meer dan één importbibliotheek
/// (twee versies van windows-sys), en zo worden ze allemaal gevonden.
unsafe fn patch_imports(base: *mut u8, from: usize, to: usize) -> usize {
    let rd32 = |off: usize| unsafe { (base.add(off) as *const u32).read_unaligned() } as usize;
    if unsafe { (base as *const u16).read_unaligned() } != 0x5A4D {
        return 0; // geen "MZ"
    }
    let nt = rd32(0x3C);
    if rd32(nt) != 0x0000_4550 {
        return 0; // geen "PE\0\0"
    }
    let opt = nt + 24;
    if unsafe { (base.add(opt) as *const u16).read_unaligned() } != 0x20B {
        return 0; // alleen PE32+ (64-bit)
    }
    // DataDirectory[1] = importtabel; in PE32+ beginnen de DataDirectories op +112 in de optionele kop
    let imports = rd32(opt + 112 + 8);
    if imports == 0 {
        return 0;
    }
    let mut patched = 0;
    let mut desc = imports;
    // IMAGE_IMPORT_DESCRIPTOR: 5 × u32, Name op +12, FirstThunk (de IAT) op +16; eindigt met nullen
    while rd32(desc + 12) != 0 {
        let mut slot = unsafe { base.add(rd32(desc + 16)) } as *mut usize;
        while unsafe { slot.read() } != 0 {
            if unsafe { slot.read() } == from {
                let mut old = 0u32;
                if unsafe { VirtualProtect(slot.cast(), 8, PAGE_READWRITE, &mut old) } != 0 {
                    unsafe { slot.write(to) };
                    unsafe { VirtualProtect(slot.cast(), 8, old, &mut old) };
                    patched += 1;
                }
            }
            slot = unsafe { slot.add(1) };
        }
        desc += 20;
    }
    patched
}

#[cfg(test)]
mod tests {
    use super::*;

    const RDW_ALLCHILDREN: u32 = 0x0080;
    const RDW_UPDATENOW: u32 = 0x0100;

    #[test]
    fn internal_paint_becomes_counted_invalidation() {
        // zo vraagt winit 0.30 om een beeld (Window::request_redraw en zijn hulpvenster)
        assert_eq!(translate(RDW_INTERNALPAINT), RDW_INVALIDATE | RDW_NOERASE);
        // andere vlaggen blijven staan
        assert_eq!(translate(RDW_INTERNALPAINT | RDW_UPDATENOW), RDW_INVALIDATE | RDW_NOERASE | RDW_UPDATENOW);
    }

    #[test]
    fn other_requests_are_untouched() {
        for f in [
            RDW_INVALIDATE,
            RDW_VALIDATE,
            RDW_INVALIDATE | RDW_INTERNALPAINT,
            RDW_VALIDATE | RDW_INTERNALPAINT,
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW,
            0,
        ] {
            assert_eq!(translate(f), f, "{f:#x}");
        }
    }

    // De importplek (IAT) van RedrawWindow in deze testexe, rechtstreeks uit de importbibliotheek van user32
    // (geen #[link] hier: dan maakt Rust er een dllimport van en zoekt hij `__imp___imp_RedrawWindow`).
    unsafe extern "C" {
        #[link_name = "__imp_RedrawWindow"]
        static IMP_REDRAW_WINDOW: usize;
    }

    /// Onder Wine moet de importplek daarna naar de omweg wijzen en de omweg het echte RedrawWindow
    /// aanroepen. Op echt Windows verandert er niets.
    #[test]
    fn install_redirects_the_import_only_under_wine() {
        let slot = || unsafe { std::ptr::read_volatile(&raw const IMP_REDRAW_WINDOW) };
        let before = slot();
        let n = install();
        if running_under_wine() {
            assert!(n >= 1, "no RedrawWindow import slot patched under Wine");
            assert_eq!(slot(), redraw_window as *const () as usize);
            assert_eq!(REAL.load(Ordering::Relaxed), before);
        } else {
            assert_eq!(n, 0);
            assert_eq!(slot(), before);
        }
    }
}
