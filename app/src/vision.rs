//! Leest het Tactical Effectiveness "Breakdown"-scherm van FM27 uit beeld.
//!
//! FM27 is een Unity-app zonder toegankelijke UI-elementen, dus: venster vastleggen (xcap),
//! tekst + posities vinden met Apple's Vision-OCR, en per factor de taartmeter rechts van het
//! label uitmeten (aandeel gekleurd vs. grijs en de tint).

use anyhow::{Context, Result, anyhow, bail};
use image::RgbaImage;
use serde::Serialize;

use crate::model::{Component, Effectiveness, Factor};

#[derive(Clone, Debug, Serialize)]
pub struct OcrLine {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub confidence: f32,
}

impl OcrLine {
    fn height(&self) -> f32 {
        self.y1 - self.y0
    }
    fn yc(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct GaugeReading {
    pub factor: Factor,
    pub score: f32,
    pub fill: f32,
    pub hue: f32,
    pub bbox: [u32; 4],
    /// de gemeten vulling past niet bij de kleur van de meter: nakijken
    pub doubt: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub gauges: Vec<GaugeReading>,
    pub components: Vec<(Component, f32)>,
    pub missing: Vec<Factor>,
    pub image_size: (u32, u32),
}

impl Analysis {
    pub fn to_effectiveness(&self, source: &str) -> Effectiveness {
        let mut e = Effectiveness::default();
        e.components.clear();
        for g in &self.gauges {
            e.set(g.factor, g.score);
        }
        e.components = self.components.clone();
        e.source = source.to_string();
        e
    }
}

// ---------------------------------------------------------------- vastleggen

/// Legt het FM27-venster vast (vereist "Schermopname"-toestemming in Systeeminstellingen).
pub fn capture_fm_window() -> Result<RgbaImage> {
    let windows = xcap::Window::all().context("vensters opvragen mislukt")?;
    let mut best: Option<(u64, xcap::Window)> = None;
    for w in windows {
        let app = w.app_name().unwrap_or_default();
        let title = w.title().unwrap_or_default();
        if !(app.contains("Football Manager") || title.contains("Football Manager")) {
            continue;
        }
        if w.is_minimized().unwrap_or(false) {
            continue;
        }
        let area = w.width().unwrap_or(0) as u64 * w.height().unwrap_or(0) as u64;
        if best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, w));
        }
    }
    let (_, w) = best.ok_or_else(|| anyhow!("{}", crate::t!("Geen Football Manager-venster gevonden. Draait FM27?", "No Football Manager window found. Is FM27 running?")))?;
    let img = w.capture_image().map_err(|e| {
        anyhow!("{} ({e})", crate::t!(
            "Vastleggen mislukt. Geef deze app toestemming bij Systeeminstellingen → Privacy en beveiliging → Schermopname.",
            "Capture failed. Allow this app under System Settings → Privacy & Security → Screen Recording."
        ))
    })?;
    if img.width() < 200 {
        bail!("{}", crate::t!("Vastgelegd beeld is leeg; staat Schermopname-toestemming aan?", "Captured image is empty; is Screen Recording permission enabled?"));
    }
    Ok(img)
}

// ---------------------------------------------------------------- OCR

/// Tekstherkenning: Apple Vision op macOS, Windows.Media.Ocr op Windows.
/// Coördinaten in pixels, oorsprong linksboven.
pub fn ocr(img: &RgbaImage) -> Result<Vec<OcrLine>> {
    platform::ocr(img)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::OcrLine;
    use anyhow::{Context, Result, anyhow};
    use image::{ImageEncoder, RgbaImage};
    use objc2::AnyThread;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

    pub fn ocr(img: &RgbaImage) -> Result<Vec<OcrLine>> {
        let (w, h) = (img.width() as f32, img.height() as f32);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
            .context("PNG")?;
        autoreleasepool(|_| {
            let data = NSData::with_bytes(&png);
            let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &NSDictionary::new());
            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(false);
            let as_request: Retained<VNRequest> = Retained::into_super(Retained::into_super(request.clone()));
            let requests = NSArray::from_retained_slice(&[as_request]);
            handler
                .performRequests_error(&requests)
                .map_err(|e| anyhow!("Vision OCR: {}", e.localizedDescription()))?;
            let mut out = Vec::new();
            if let Some(results) = request.results() {
                for obs in results.iter() {
                    let cands = obs.topCandidates(1);
                    let Some(best) = cands.firstObject() else { continue };
                    let bb = unsafe { obs.boundingBox() };
                    out.push(OcrLine {
                        text: best.string().to_string(),
                        x0: bb.origin.x as f32 * w,
                        x1: (bb.origin.x + bb.size.width) as f32 * w,
                        y0: (1.0 - (bb.origin.y + bb.size.height) as f32) * h,
                        y1: (1.0 - bb.origin.y as f32) * h,
                        confidence: best.confidence(),
                    });
                }
            }
            Ok(out)
        })
    }
}

#[cfg(windows)]
mod platform {
    use super::OcrLine;
    use anyhow::{Result, anyhow};
    use image::RgbaImage;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;

    pub fn ocr(img: &RgbaImage) -> Result<Vec<OcrLine>> {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        let mut bgra = img.as_raw().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buffer = writer.DetachBuffer()?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, img.width() as i32, img.height() as i32)?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| anyhow!("Windows OCR: {e}"))?;
        let result = engine.RecognizeAsync(&bmp)?.join()?;
        let mut out = Vec::new();
        for line in result.Lines()? {
            let text = line.Text()?.to_string();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for w in line.Words()? {
                let r = w.BoundingRect()?;
                x0 = x0.min(r.X);
                y0 = y0.min(r.Y);
                x1 = x1.max(r.X + r.Width);
                y1 = y1.max(r.Y + r.Height);
            }
            if x1 > x0 {
                out.push(OcrLine { text, x0, y0, x1, y1, confidence: 1.0 });
            }
        }
        Ok(out)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::OcrLine;
    pub fn ocr(_img: &image::RgbaImage) -> anyhow::Result<Vec<OcrLine>> {
        anyhow::bail!("OCR is only available on macOS and Windows")
    }
}

// ---------------------------------------------------------------- matching

fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// Titelregel: exact de titel, eventueel met een kort icoon-voorvoegsel dat OCR als tekst ziet.
fn is_title(text: &str, alias: &str) -> bool {
    let t = norm(text);
    t.ends_with(alias) && t.len() <= alias.len() + 3 && {
        let p = t.len() - alias.len();
        p == 0 || t.as_bytes()[p - 1] == b' '
    }
}

/// Bewerkingsafstand (Levenshtein) tussen twee korte teksten.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Vindt de OCR-regel voor een label; geeft de regel en de x waar het label eindigt.
///
/// Op een klein scherm leest de OCR een letter nogal eens verkeerd ("Pressing IntensiW",
/// "Tacticd Complexity"): een reeks woorden die op het label lijkt (een paar letters verschil,
/// naar rato van de lengte) telt daarom ook, met voorkeur voor een exacte treffer.
fn find_label<'a>(lines: &'a [OcrLine], label: &str) -> Option<(&'a OcrLine, f32)> {
    let want = norm(label);
    let want_words = want.split(' ').count();
    let allowed = if want.len() >= 8 { want.len() / 6 } else { 0 };
    let mut best: Option<(i32, &OcrLine, f32)> = None;
    for l in lines {
        let t = norm(&l.text);
        let words: Vec<&str> = t.split(' ').filter(|w| !w.is_empty()).collect();
        if words.len() < want_words {
            continue;
        }
        for start in 0..=words.len() - want_words {
            let window = words[start..start + want_words].join(" ");
            let dist = if window == want { 0 } else { edit_distance(&window, &want) };
            if dist > allowed {
                continue;
            }
            let extra = (words.len() - want_words) as i32;
            let score = 1000 - 100 * dist as i32 - extra;
            // waar het label eindigt: naar rato van de tekens tot en met het venster
            let upto: usize = words[..start + want_words].iter().map(|w| w.chars().count()).sum::<usize>() + start + want_words - 1;
            let n = words.iter().map(|w| w.chars().count()).sum::<usize>() + words.len().saturating_sub(1);
            let label_end = l.x0 + (l.x1 - l.x0) * upto as f32 / n.max(1) as f32;
            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, l, label_end));
            }
        }
    }
    best.map(|(_, l, e)| (l, e))
}

/// De kolommen van de Breakdown, elk met zijn key factors van boven naar beneden. Een label dat de
/// OCR mist, krijgt zijn plek uit de rij-afstand en de meter-kolom van de gevonden labels.
const COLUMNS: &[&[Factor]] = &[
    &[
        Factor::InPossessionShape,
        Factor::PassingStyle,
        Factor::BuildUp,
        Factor::ChanceCreation,
        Factor::OutOfPossessionShape,
        Factor::LineOfEngagement,
        Factor::PressingIntensity,
        Factor::PressingTrap,
    ],
    &[Factor::PlayerTacticalAbility, Factor::TacticalComplexity],
    &[Factor::PlayerCapability, Factor::PlayerRoleSuitability],
    &[Factor::ManagerialSupport, Factor::TeammatesAndStaffRelationships, Factor::ManagerTendencies],
    &[Factor::QualityOfNextOppositionReport, Factor::OppositionPlayerKnowledge],
];

// ---------------------------------------------------------------- pixels

fn hsv(p: &image::Rgba<u8>) -> (f32, f32, f32) {
    let r = p[0] as f32 / 255.0;
    let g = p[1] as f32 / 255.0;
    let b = p[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max > 0.0 { d / max } else { 0.0 };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, max)
}

/// Gekleurd deel van een meter: verzadigd, helder, tint rood…groen.
fn is_colored(p: &image::Rgba<u8>) -> bool {
    let (h, s, v) = hsv(p);
    // na het (bilineair) herschalen van een kleine meter zijn de kleuren iets flauwer
    s >= 0.36 && v >= 0.42 && (h <= 160.0 || h >= 345.0)
}

/// Middelgrijs (leeg) deel van een meter, zoals eerder op macOS gemeten. Let op: de lichtere
/// achtergrond van een gemarkeerde rij in de Breakdown valt hier ook onder.
fn is_gauge_grey(p: &image::Rgba<u8>) -> bool {
    let (_, s, v) = hsv(p);
    s < 0.16 && (0.30..=0.66).contains(&v)
}

/// Bijna wit: het lege deel van een taartmeter zoals FM27 hem (op Windows gemeten) tekent, en de
/// witte markering in een balk. Nooit achtergrond.
fn is_light(p: &image::Rgba<u8>) -> bool {
    let (_, s, v) = hsv(p);
    s < 0.25 && v > 0.66
}

type Pred = dyn Fn(&image::Rgba<u8>) -> bool;

fn column_hits(img: &RgbaImage, x: u32, y0: u32, y1: u32, part: &Pred) -> u32 {
    (y0..y1).filter(|&y| part(img.get_pixel(x, y))).count() as u32
}

/// Zoekt de eerste taartmeter rechts van `x_start` binnen de rij rond `yc`.
///
/// Eerst streng: een meter bestaat uit gekleurde en bijna witte pixels (de lege segmenten), die
/// nooit in de achtergrond voorkomen. Lukt dat niet (oudere weergave met middelgrijze lege
/// segmenten), dan zoals voorheen met grijs; de achtergrond van een gemarkeerde rij geeft dan een
/// veel te lange reeks en valt af.
fn find_pie(img: &RgbaImage, x_start: f32, x_limit: f32, yc: f32, th: f32) -> Option<GaugeShape> {
    find_pies(img, x_start, x_limit, yc, th).into_iter().next()
}

/// De laatste taartmeter op de rij (voor een rij waarvan het label niet gelezen is: het label staat
/// links, de meter is het laatste ronde blok vóór de volgende kolom).
fn find_pie_last(img: &RgbaImage, x_start: f32, x_limit: f32, yc: f32, th: f32) -> Option<GaugeShape> {
    find_pies(img, x_start, x_limit, yc, th).into_iter().last()
}

/// Alle taartmeters tussen `x_start` en `x_limit` op de rij rond `yc`, van links naar rechts.
fn find_pies(img: &RgbaImage, x_start: f32, x_limit: f32, yc: f32, th: f32) -> Vec<GaugeShape> {
    let strict: Box<Pred> = Box::new(|p| is_colored(p) || is_light(p));
    let loose: Box<Pred> = Box::new(|p| is_colored(p) || is_gauge_grey(p));
    let v = find_pies_with(img, x_start, x_limit, yc, th, &*strict, &is_light);
    if !v.is_empty() {
        return v;
    }
    find_pies_with(img, x_start, x_limit, yc, th, &*loose, &is_gauge_grey)
}

/// Kleine gaten (de donkere spaken tussen de taartpunten) breken een meter niet op.
fn find_pies_with(img: &RgbaImage, x_start: f32, x_limit: f32, yc: f32, th: f32, part: &Pred, empty: &Pred) -> Vec<GaugeShape> {
    let (w, h) = (img.width(), img.height());
    let band = (th * 1.0).max(6.0);
    let y0 = (yc - band).max(0.0) as u32;
    let y1 = ((yc + band) as u32).min(h);
    let xs = x_start.max(0.0) as u32;
    let xe = (x_limit as u32).min(w);
    let need = ((th * 0.30) as u32).max(2);
    let min_run = ((th * 0.55) as u32).max(3);
    let max_run = (th * 3.2) as u32 + 4;
    let max_gap = ((th * 0.15) as u32).max(2);

    let mut out = Vec::new();
    let mut run: Option<(u32, u32)> = None; // (begin, laatste treffer)
    let mut finish = |run: Option<(u32, u32)>, out: &mut Vec<GaugeShape>| {
        if let Some((rs, last)) = run {
            let len = last + 1 - rs;
            if len >= min_run && len <= max_run {
                if let Some(g) = measure_pie(img, rs, last + 1, y0, y1, yc, empty) {
                    out.push(g);
                }
            }
        }
    };
    for x in xs..xe {
        if column_hits(img, x, y0, y1, part) >= need {
            run = Some(match run {
                Some((rs, last)) if x - last <= max_gap + 1 => (rs, x),
                other => {
                    finish(other, &mut out);
                    (x, x)
                }
            });
        }
    }
    finish(run, &mut out);
    out
}

#[derive(Clone, Debug)]
struct GaugeShape {
    fill: f32,
    hue: f32,
    bbox: [u32; 4],
}

/// Meet een taartmeter: FM vult hem met de klok mee vanaf 12 uur, in 10 segmenten. De vulling
/// wordt per hoeksector bepaald (36 sectoren van 10°), zodat de spaken tussen de punten niet
/// meetellen; `empty` herkent de lege segmenten.
fn measure_pie(img: &RgbaImage, xa: u32, xb: u32, y0: u32, y1: u32, yc: f32, empty: &Pred) -> Option<GaugeShape> {
    // Verticale grenzen van de meter binnen de kolommen: de aaneengesloten reeks rijen rond het
    // midden van het label. Valt er in de zoekband ook een stuk van de meter erboven of eronder
    // (grotere tekst in FM, kleinere rijafstand), dan hoort dat er niet bij.
    let has: Vec<bool> = (y0..y1)
        .map(|y| {
            (xa..xb).any(|x| {
                let p = img.get_pixel(x, y);
                is_colored(p) || empty(p)
            })
        })
        .collect();
    let rows: Vec<usize> = (0..has.len()).filter(|i| has[*i]).collect();
    let mid = (yc - y0 as f32).max(0.0);
    let start = *rows.iter().min_by(|a, b| (**a as f32 - mid).abs().total_cmp(&(**b as f32 - mid).abs()))?;
    let (mut lo, mut hi) = (start, start);
    while lo > 0 && (has[lo - 1] || (lo > 1 && has[lo - 2])) {
        lo -= 1;
    }
    while hi + 1 < has.len() && (has[hi + 1] || (hi + 2 < has.len() && has[hi + 2])) {
        hi += 1;
    }
    let (ty0, ty1) = (y0 + lo as u32, y0 + hi as u32);
    // een taart is rond: een blok dat veel breder dan hoog is (of andersom) is iets anders
    let (bw, bh) = ((xb - xa) as f32, (ty1 - ty0 + 1) as f32);
    if bw > bh * 1.6 || bh > bw * 1.6 {
        return None;
    }
    // Altijd op een vaste maat meten: op een klein scherm (1366×768, of 1080p met een kleine
    // UI-schaal) heeft een meter maar 12–18 px doorsnede, en dan hebben veel hoeksectoren geen
    // enkele pixel. Vergroten (bilineair) voegt geen informatie toe, maar maakt de telling per
    // sector wel betrouwbaar; de drempels hieronder gelden dan op elke resolutie.
    const SIZE: f32 = 72.0;
    let (sw, sh) = (xb - xa, ty1 - ty0 + 1);
    let k = SIZE / sw.max(sh) as f32;
    let sub = image::imageops::crop_imm(img, xa, ty0, sw, sh).to_image();
    let pie = image::imageops::resize(&sub, ((sw as f32 * k).round() as u32).max(1), ((sh as f32 * k).round() as u32).max(1), image::imageops::FilterType::Triangle);
    let (pw, ph) = (pie.width(), pie.height());
    let cx = (pw as f32 - 1.0) / 2.0;
    let cy = (ph as f32 - 1.0) / 2.0;
    let r = pw.max(ph) as f32 / 2.0;

    const BINS: usize = 36;
    // (sector, gekleurd, tint, binnenring) per pixel van de ring tussen het midden en de rand
    let mut px: Vec<(usize, bool, f32, bool)> = Vec::new();
    let mut gry = [0u32; BINS];
    for y in 0..ph {
        for x in 0..pw {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < r * 0.25 || d > r * 1.05 {
                continue; // midden (alle spaken komen samen) en rand overslaan
            }
            // hoek met de klok mee vanaf 12 uur
            let mut a = dx.atan2(-dy).to_degrees();
            if a < 0.0 {
                a += 360.0;
            }
            let bin = ((a / 360.0 * BINS as f32) as usize).min(BINS - 1);
            let p = pie.get_pixel(x, y);
            if is_colored(p) {
                px.push((bin, true, hsv(p).0, d < r * 0.85));
            } else if empty(p) {
                gry[bin] += 1;
            }
        }
    }
    // De vulling heeft één kleur (oranje, geel of groen, naar het niveau). Die tint komt uit het
    // binnenste van de ring, waar geen achtergrond doorheen mengt; alleen pixels in die tint tellen
    // als gevuld. Een rand die met een gekleurde achtergrond (clubkleuren) mengt, telt zo niet mee.
    let hue_dist = |a: f32, b: f32| {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    };
    let mut hist = [0u32; 36];
    for (_, _, h, inner) in &px {
        if *inner {
            hist[((h / 10.0) as usize).min(35)] += 1;
        }
    }
    let peak = (0..36).max_by_key(|i| hist[*i]).unwrap_or(0);
    let fill_hue = if hist[peak] == 0 {
        None
    } else {
        let (mut sx, mut sy) = (0f32, 0f32);
        for (_, _, h, inner) in &px {
            if *inner && hue_dist(*h, peak as f32 * 10.0 + 5.0) <= 15.0 {
                sx += h.to_radians().cos();
                sy += h.to_radians().sin();
            }
        }
        Some(sy.atan2(sx).to_degrees().rem_euclid(360.0))
    };
    let mut col = [0u32; BINS];
    let (mut hx, mut hy, mut ncol) = (0f32, 0f32, 0u32);
    for (bin, _, h, _) in &px {
        if fill_hue.is_some_and(|f| hue_dist(*h, f) <= 25.0) {
            col[*bin] += 1;
            hx += h.to_radians().cos();
            hy += h.to_radians().sin();
            ncol += 1;
        } else {
            gry[*bin] += 1;
        }
    }
    let seen = (0..BINS).filter(|i| col[*i] + gry[*i] > 0).count();
    if seen < BINS / 2 {
        return None;
    }
    // De vulling loopt aaneengesloten met de klok mee vanaf 12 uur. Loop de sectoren langs: één
    // sector zonder echte kleur is een lichte spaak tussen twee punten (of het lichte kruis in het
    // midden) en breekt de reeks niet, twee op rij wel. De laatste gekleurde sector vóór die
    // onderbreking bepaalt het gevulde deel; kleur die door het vergroten over de 12-uur-grens
    // heen lekt (de sector vlak vóór 12 uur bij een niet-volle meter) telt zo niet mee.
    let colored = |i: usize| {
        let total = (col[i] + gry[i]).max(1);
        col[i] >= 3 && col[i] * 4 >= total
    };
    let (mut last, mut gap) = (None, 0);
    for i in 0..BINS {
        if colored(i) {
            last = Some(i);
            gap = 0;
        } else {
            gap += 1;
            if gap >= 2 {
                break;
            }
        }
    }
    let mut fill = match last {
        Some(i) => {
            let total = (col[i] + gry[i]).max(1);
            let frac = (col[i] as f32 / total as f32).min(1.0);
            (i as f32 + frac) / BINS as f32
        }
        None => 0.0,
    };
    // 10 segmenten: de lichte spaak op de grens snoept een paar procent af, dus afronden op een
    // heel segment als de meting daar dichtbij zit
    let tenths = (fill * 10.0).round();
    if (fill * 10.0 - tenths).abs() <= 0.6 {
        fill = tenths / 10.0;
    }
    let mut hue = hy.atan2(hx).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }
    if hue > 300.0 {
        hue = 0.0; // roodtinten rond 360° → 0
    }
    Some(GaugeShape {
        fill: fill.clamp(0.0, 1.0),
        hue: if ncol == 0 { 0.0 } else { hue },
        bbox: [xa, ty0, xb, ty1 + 1],
    })
}

/// Welk deel van een taart (0–1) bij de kleur van de vulling past. FM kleurt een meter naar zijn
/// niveau (gemeten op 2560×1440, Ajax- en Sunderland-save): 20–40 % oranje (tint ~43°), 50–70 %
/// geel (~60°), 90 % groen (~120°); 40 % kan ook geel zijn. De grenzen hier zijn ruim genomen.
fn color_band(hue: f32) -> (f32, f32) {
    if hue < 50.0 {
        (0.0, 0.5)
    } else if hue < 90.0 {
        (0.3, 0.9)
    } else {
        (0.7, 1.0)
    }
}

/// Meter-score: het gevulde deel van de taart (4 van de 10 segmenten = 40), en of dat bij de kleur
/// past. Past het niet (een oranje meter die als 70 gemeten wordt), dan is de meting fout gegaan:
/// de score wordt naar de rand van wat de kleur toelaat gezet en de meter gemarkeerd, zodat de
/// gebruiker hem nakijkt.
fn gauge_score(fill: f32, hue: f32, has_color: bool) -> (f32, bool) {
    if !has_color {
        return (0.0, false);
    }
    let (lo, hi) = color_band(hue);
    let doubt = fill < lo - 0.01 || fill > hi + 0.01;
    ((fill.clamp(lo, hi) * 100.0).clamp(0.0, 100.0), doubt)
}

/// Balkmeter rechts van een onderdeel-titel: een rij streepjes (11 plekken) waarvan er één
/// gekleurd is (de stand) en één wit (een referentie). De gekleurde streep vervangt een grijs
/// streepje, dus de witte en gekleurde streep tellen mee voor de breedte van de balk.
fn read_bar(img: &RgbaImage, x_start: f32, x_limit: f32, yc: f32, th: f32) -> Option<f32> {
    let (w, h) = (img.width(), img.height());
    let y0 = (yc - th * 0.9).max(0.0) as u32;
    let y1 = ((yc + th * 0.9) as u32).min(h);
    let xs = x_start.max(0.0) as u32;
    let xe = (x_limit as u32).min(w);
    if xe <= xs + 4 || y1 <= y0 + 2 {
        return None;
    }
    let need = ((th * 0.45) as u32).max(3);
    // Per kolom: de gekleurde streep (de stand), de helderheid van het lichtste derde van de pixels
    // (een streepje is daar een piek in, ook als het op een klein scherm uitgesmeerd is en de
    // tussenruimte niet meer echt donker wordt) en hoeveel van de kolom licht is (de rand van het
    // kader loopt over de hele hoogte, een streepje maar over een deel).
    let mut marker: Vec<u32> = Vec::new();
    let mut prof: Vec<f32> = Vec::with_capacity((xe - xs) as usize);
    let mut tall: Vec<f32> = Vec::with_capacity((xe - xs) as usize);
    for x in xs..xe {
        let mut vs: Vec<f32> = Vec::with_capacity((y1 - y0) as usize);
        let mut c = 0;
        for y in y0..y1 {
            let p = img.get_pixel(x, y);
            if is_colored(p) {
                c += 1;
            }
            vs.push(hsv(p).2);
        }
        if c >= need {
            marker.push(x);
        }
        tall.push(vs.iter().filter(|v| **v > 0.45).count() as f32 / vs.len() as f32);
        vs.sort_by(|a, b| b.total_cmp(a));
        let top = (vs.len() / 3).max(1);
        prof.push(vs[..top].iter().sum::<f32>() / top as f32);
    }
    if marker.is_empty() {
        return None;
    }
    let m = marker.iter().sum::<u32>() as f32 / marker.len() as f32;
    // licht gladstrijken, dan de plaatselijke maxima die duidelijk boven hun omgeving uitsteken
    let n = prof.len();
    let smooth: Vec<f32> = (0..n).map(|i| (prof[i.saturating_sub(1)] + prof[i] + prof[(i + 1).min(n - 1)]) / 3.0).collect();
    let pitch_guess = (th * 0.3).max(3.0);
    let win = (pitch_guess * 0.7).round().max(1.0) as usize;
    let mut centers: Vec<f32> = Vec::new();
    let mut i = 1;
    while i + 1 < n {
        let v = smooth[i];
        if v >= smooth[i - 1] && v >= smooth[i + 1] {
            // een plateau is één piek: tot waar de waarde gelijk blijft
            let mut j = i;
            while j + 1 < n && smooth[j + 1] == v {
                j += 1;
            }
            let lo_l = smooth[i.saturating_sub(win)..i].iter().copied().fold(v, f32::min);
            let lo_r = smooth[j + 1..(j + 1 + win).min(n)].iter().copied().fold(v, f32::min);
            // een streepje beslaat ruwweg de halve hoogte van de band, de rand van het kader veel meer
            let border = tall[i..=j].iter().copied().fold(0.0, f32::max) >= 0.6;
            if v - lo_l.max(lo_r) >= 0.05 && !border {
                centers.push(xs as f32 + (i + j) as f32 / 2.0);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    centers.push(m);
    centers.sort_by(|a, b| a.total_cmp(b));
    centers.dedup_by(|a, b| (*a - *b).abs() < pitch_guess * 0.5);
    // Vanaf de marker naar buiten lopen zolang de afstand tussen streepjes regelmatig blijft (één
    // gemist streepje mag); de rand van het kader is al weg, losse vlakken verderop liggen verder
    // uit elkaar en vallen af.
    let mi = centers.iter().position(|c| (*c - m).abs() < pitch_guess * 0.5)?;
    let gaps: Vec<f32> = centers.windows(2).map(|w| w[1] - w[0]).collect();
    let mut sorted = gaps.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let typical = *sorted.get(sorted.len() / 2)?;
    let limit = typical * 2.6;
    let (mut lo_i, mut hi_i) = (mi, mi);
    while lo_i > 0 && centers[lo_i] - centers[lo_i - 1] <= limit {
        lo_i -= 1;
    }
    while hi_i + 1 < centers.len() && centers[hi_i + 1] - centers[hi_i] <= limit {
        hi_i += 1;
    }
    let (lo, hi) = (centers[lo_i], centers[hi_i]);
    let span = hi - lo;
    if hi_i - lo_i < 3 || typical < 2.0 || span <= 0.0 {
        return None;
    }
    if std::env::var_os("FM27_VISION_DEBUG").is_some() {
        eprintln!("balk y={yc:.0} marker={m:.1} streepjes={centers:?} keten={lo:.1}..{hi:.1} stap={typical:.2}");
    }
    // FM27 tekent 11 plekken: de stand volgt uit de plek van de gekleurde streep tussen het eerste
    // en het laatste streepje, zodat een streepje dat in de marker opgaat niet meetelt. Alleen als
    // de reeks duidelijk korter is (een deel van de balk in ruis), telt de stap vanaf het eerste.
    let slots = (span / typical).round() as usize + 1;
    let idx = if (9..=12).contains(&slots) {
        (m - lo) / span * 10.0
    } else {
        (m - lo) / typical
    };
    Some((idx.round().max(0.0) / 10.0 * 100.0).clamp(0.0, 100.0))
}

// ---------------------------------------------------------------- analyse

/// Hoogte van de labels van de key factors zoals de OCR ze las (mediaan).
fn label_height(lines: &[OcrLine]) -> Option<f32> {
    let mut hs: Vec<f32> = Factor::ALL.iter().filter_map(|f| find_label(lines, f.label()).map(|(l, _)| l.height())).collect();
    if hs.is_empty() {
        return None;
    }
    hs.sort_by(|a, b| a.total_cmp(b));
    Some(hs[hs.len() / 2])
}

/// De tekstgrootte waarop de meting is afgesteld: de labels van FM27 op 2560×1440 met 100 % schaal.
const REF_TEXT: f32 = 20.0;

/// Leest de Breakdown. Op een ander scherm (of een andere UI-schaal in FM) is alles groter of
/// kleiner, en op een klein scherm leest de OCR slechter en zijn de meters te klein om te meten:
/// dan nog eens op een kopie die naar de referentiegrootte is geschaald, en het beste resultaat
/// telt. Zonder één herkend label worden twee vergrotingen geprobeerd.
pub fn analyze_image(img: &RgbaImage) -> Result<(Analysis, Vec<OcrLine>)> {
    let lines = ocr(img)?;
    let mut best = (analyze_with_lines(img, &lines), lines);
    let scales: Vec<f32> = match label_height(&best.1) {
        Some(th) if (th - REF_TEXT).abs() / REF_TEXT > 0.12 => vec![REF_TEXT / th],
        Some(_) => Vec::new(),
        None => vec![1.5, 2.0],
    };
    for k in scales {
        let (w, h) = ((img.width() as f32 * k).round() as u32, (img.height() as f32 * k).round() as u32);
        if w < 64 || h < 64 || w > 10_000 || h > 10_000 {
            continue;
        }
        let big = image::imageops::resize(img, w, h, image::imageops::FilterType::CatmullRom);
        let lines2 = ocr(&big)?;
        let mut a2 = analyze_with_lines(&big, &lines2);
        // terug naar de maten van de opname
        for g in &mut a2.gauges {
            g.bbox = g.bbox.map(|v| (v as f32 / k).round() as u32);
        }
        a2.image_size = (img.width(), img.height());
        let lines2: Vec<OcrLine> = lines2.into_iter().map(|l| OcrLine { x0: l.x0 / k, y0: l.y0 / k, x1: l.x1 / k, y1: l.y1 / k, ..l }).collect();
        // op de referentiegrootte zijn de drempels afgesteld: die meting wint bij gelijke stand
        let better = a2.gauges.len() >= best.0.gauges.len() && a2.components.len() >= best.0.components.len();
        if better {
            best = (a2, lines2);
        }
    }
    Ok(best)
}

pub fn analyze_with_lines(img: &RgbaImage, lines: &[OcrLine]) -> Analysis {
    let w = img.width() as f32;
    let mut gauges = Vec::new();
    let mut missing = Vec::new();

    // Begin van elke label op dezelfde rij: de meter van een kolom ligt vóór het volgende label.
    let label_hits: Vec<(Factor, &OcrLine, f32)> = Factor::ALL
        .iter()
        .filter_map(|f| find_label(lines, f.label()).map(|(l, e)| (*f, l, e)))
        .collect();

    for f in Factor::ALL {
        let Some((_, line, label_end)) = label_hits.iter().find(|(x, _, _)| *x == f).cloned()
        else {
            missing.push(f);
            continue;
        };
        let th = line.height().max(4.0);
        let next_label_x = label_hits
            .iter()
            .filter(|(_, l, _)| (l.yc() - line.yc()).abs() < th && l.x0 > label_end + th)
            .map(|(_, l, _)| l.x0 - th * 0.3)
            .fold(w, f32::min);
        let limit = next_label_x.min(label_end + th * 22.0);
        match find_pie(img, label_end + th * 0.4, limit, line.yc(), th) {
            Some(g) => {
                let (score, doubt) = gauge_score(g.fill, g.hue, g.fill > 0.0);
                gauges.push(GaugeReading { factor: f, score, fill: g.fill, hue: g.hue, bbox: g.bbox, doubt });
            }
            None => missing.push(f),
        }
    }

    // Labels die de OCR niet (goed) las: de rijen van een kolom liggen even ver uit elkaar en de
    // meters van een kolom staan recht onder elkaar, dus uit de gevonden rijen volgt waar de
    // andere moeten zitten. De meter wordt dan alleen in die smalle kolom gezocht.
    let row = |f: Factor| -> Option<(usize, usize)> { COLUMNS.iter().enumerate().find_map(|(c, col)| col.iter().position(|x| *x == f).map(|i| (c, i))) };
    let found: Vec<(Factor, f32, f32, [u32; 4])> = gauges
        .iter()
        .filter_map(|g| label_hits.iter().find(|(f, _, _)| *f == g.factor).map(|(_, l, _)| (g.factor, l.yc(), l.height().max(4.0), g.bbox)))
        .collect();
    let mut pitches: Vec<f32> = Vec::new();
    for a in &found {
        for b in &found {
            if let (Some((ca, ia)), Some((cb, ib))) = (row(a.0), row(b.0)) {
                if ca == cb && ib > ia {
                    pitches.push((b.1 - a.1) / (ib - ia) as f32);
                }
            }
        }
    }
    pitches.sort_by(|a, b| a.total_cmp(b));
    let pitch = pitches.get(pitches.len() / 2).copied();

    // De kop "Key Factors" boven elke kolom: voor een kolom waarvan geen enkel label gelezen is,
    // geeft die de plek van de eerste rij (2,15 × de kophoogte eronder) en de linkerkant van de
    // kolom; de volgende kop (of de rand) begrenst de kolom rechts.
    let mut heads: Vec<&OcrLine> = lines
        .iter()
        .filter(|l| {
            let t = norm(&l.text);
            t == "key factors" || (t.len() >= 9 && edit_distance(&t, "key factors") <= 2)
        })
        .collect();
    // in leesvolgorde: op een smal scherm kunnen de panelen in twee rijen staan
    let band = |l: &OcrLine| (l.yc() / (l.height().max(4.0) * 8.0)).floor();
    heads.sort_by(|a, b| band(a).total_cmp(&band(b)).then(a.x0.total_cmp(&b.x0)));
    let mut head_of: Vec<Option<&OcrLine>> = vec![None; COLUMNS.len()];
    for (c, col) in COLUMNS.iter().enumerate() {
        // de kop staat boven de labels van de kolom, op ongeveer dezelfde x
        if let Some((_, l0, _)) = label_hits.iter().find(|(f, _, _)| col.contains(f)) {
            head_of[c] = heads
                .iter()
                .copied()
                .filter(|k| k.yc() < l0.yc() && (k.x0 - l0.x0).abs() < l0.height() * 4.0)
                .min_by(|a, b| (l0.yc() - a.yc()).total_cmp(&(l0.yc() - b.yc())));
        }
    }
    let open: Vec<usize> = (0..COLUMNS.len()).filter(|c| head_of[*c].is_none()).collect();
    let rest: Vec<&OcrLine> = heads.iter().copied().filter(|k| !head_of.iter().flatten().any(|a| std::ptr::eq(*a, *k))).collect();
    if rest.len() == open.len() {
        // de overgebleven koppen horen op volgorde bij de overgebleven kolommen
        for (c, k) in open.into_iter().zip(rest) {
            head_of[c] = Some(k);
        }
    }

    let still_missing: Vec<Factor> = missing.clone();
    for f in still_missing {
        let Some((c, i)) = row(f) else { continue };
        let in_col: Vec<&(Factor, f32, f32, [u32; 4])> = found.iter().filter(|(g, _, _, _)| row(*g).is_some_and(|(cg, _)| cg == c)).collect();
        let hit = if let Some(near) = in_col.iter().min_by_key(|(g, _, _, _)| row(*g).map(|(_, ig)| ig.abs_diff(i)).unwrap_or(usize::MAX)) {
            // zelfde kolom, andere rij: de meters staan recht onder elkaar
            let (g, yc, th, bbox) = **near;
            let Some((_, ig)) = row(g) else { continue };
            let y = yc + (i as f32 - ig as f32) * pitch.unwrap_or(th * 2.45);
            (y >= 0.0 && y < img.height() as f32).then(|| find_pie(img, bbox[0] as f32 - th, bbox[2] as f32 + th, y, th)).flatten()
        } else if let Some(k) = head_of[c] {
            // niets gelezen in deze kolom: vanaf de kop, de meter is het laatste ronde blok op de rij
            let th = k.height().max(4.0);
            let y = k.yc() + th * 2.15 + i as f32 * pitch.unwrap_or(th * 2.9);
            let right = heads.iter().filter(|o| o.x0 > k.x0 + th && (o.yc() - k.yc()).abs() < th * 8.0).map(|o| o.x0 - th).fold(w, f32::min);
            (y >= 0.0 && y < img.height() as f32).then(|| find_pie_last(img, k.x0, right, y, th * 1.15)).flatten()
        } else {
            None
        };
        if let Some(g) = hit {
            let (score, doubt) = gauge_score(g.fill, g.hue, g.fill > 0.0);
            gauges.push(GaugeReading { factor: f, score, fill: g.fill, hue: g.hue, bbox: g.bbox, doubt });
            missing.retain(|m| *m != f);
        }
    }
    gauges.sort_by_key(|g| Factor::ALL.iter().position(|f| *f == g.factor));

    let mut components = Vec::new();
    for c in Component::ALL {
        let hit = c.ocr_aliases().iter().find_map(|a| {
            lines.iter().find(|l| is_title(&l.text, a)).map(|l| (l, l.x1))
        });
        let Some((line, end)) = hit else { continue };
        let th = line.height().max(4.0);
        let next_title = lines
            .iter()
            .filter(|l| (l.yc() - line.yc()).abs() < th && l.x0 > end + th * 2.0)
            .filter(|l| Component::ALL.iter().any(|c2| c2.ocr_aliases().iter().any(|a| is_title(&l.text, a))))
            .map(|l| l.x0 - th)
            .fold(w, f32::min);
        if let Some(v) = read_bar(img, end + th * 0.5, next_title, line.yc(), th) {
            components.push((c, v));
        }
    }

    Analysis { gauges, components, missing, image_size: (img.width(), img.height()) }
}

pub fn analyze_file(path: &std::path::Path) -> Result<(Analysis, Vec<OcrLine>)> {
    let img = image::open(path).with_context(|| path.display().to_string())?.to_rgba8();
    analyze_image(&img)
}

pub fn analyze_screen() -> Result<(Analysis, RgbaImage)> {
    let img = capture_fm_window()?;
    // de opname bewaren, zodat een verkeerde uitlezing achteraf na te spelen is:
    // `fm27-trainer analyze "<map>\last-capture.png" --ocr`
    let dir = crate::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = img.save(dir.join("last-capture.png"));
    let (a, _) = analyze_image(&img)?;
    if a.gauges.len() >= 4 {
        // de laatste opname waarop echt een Breakdown stond, blijft bewaard
        let _ = img.save(dir.join("last-breakdown.png"));
    }
    if a.gauges.len() < 4 {
        bail!(
            "{}",
            crate::t!(
                format!("Breakdown-scherm niet herkend ({} van 17 meters gevonden). Open in FM27 het Tactical Effectiveness-overzicht met de Breakdown (Tactical Understanding / Player Ability / Player Buy-In / Next Match Info) en probeer opnieuw.", a.gauges.len()),
                format!("Breakdown screen not recognised ({} of 17 gauges found). Open the Tactical Effectiveness overview with the Breakdown (Tactical Understanding / Player Ability / Player Buy-In / Next Match Info) in FM27 and try again.", a.gauges.len())
            )
        );
    }
    Ok((a, img))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Een taartmeter zoals FM27 hem tekent: 10 segmenten met lichte spaken, `filled` gevuld met de
    /// klok mee vanaf 12 uur in `fill`, de rest bijna wit, met zachte randen (4×4 deelpixels) op de
    /// achtergrond `bg`. Midden op (cx, cy), straal `r`.
    fn draw_pie(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, filled: u32, fill: [u8; 3], bg: [u8; 3]) {
        const EMPTY: [f32; 3] = [218.0, 206.0, 208.0];
        const SPOKE: [f32; 3] = [190.0, 210.0, 222.0];
        for y in 0..img.height() {
            for x in 0..img.width() {
                let mut acc = [0f32; 3];
                for sy in 0..4 {
                    for sx in 0..4 {
                        let dx = x as f32 + (sx as f32 + 0.5) / 4.0 - cx;
                        let dy = y as f32 + (sy as f32 + 0.5) / 4.0 - cy;
                        let d = (dx * dx + dy * dy).sqrt();
                        let c: [f32; 3] = if d > r {
                            bg.map(|v| v as f32)
                        } else {
                            let a = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                            let seg = (a / 36.0) as u32;
                            let edge = (a - seg as f32 * 36.0).min((seg + 1) as f32 * 36.0 - a);
                            // spaak: ~1 px breed, ongeacht de straal
                            if edge.to_radians() * d < 0.6 && d > 1.0 {
                                SPOKE
                            } else if seg < filled {
                                fill.map(|v| v as f32)
                            } else {
                                EMPTY
                            }
                        };
                        for i in 0..3 {
                            acc[i] += c[i] / 16.0;
                        }
                    }
                }
                let o = img.get_pixel(x, y);
                // alleen het gebied van de taart (met rand) overschrijven
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                if (dx * dx + dy * dy).sqrt() <= r + 1.5 {
                    img.put_pixel(x, y, Rgba([acc[0].round() as u8, acc[1].round() as u8, acc[2].round() as u8, 255]));
                } else {
                    img.put_pixel(x, y, *o);
                }
            }
        }
    }

    fn fm_color(filled: u32) -> [u8; 3] {
        match filled {
            0..=4 => [255, 183, 0], // oranje (~43°)
            5..=8 => [255, 255, 0], // geel
            _ => [0, 235, 40],      // groen
        }
    }

    /// Elke vulling van 0 tot 10 segmenten, klein (1366×768: ~9 px straal) en groot (2560×1440:
    /// ~14 px straal), op de achtergronden die een paneel in FM27 kan hebben: donker en half
    /// doorzichtig over de clubkleur (gemeten: (70, 49, 53) bij Ajax en Sunderland), de lichtere
    /// gemarkeerde rij, en donkere tinten van andere clubkleuren.
    #[test]
    fn pies_on_any_background() {
        let backgrounds: [[u8; 3]; 7] = [[70, 49, 53], [92, 80, 84], [95, 25, 40], [30, 40, 85], [28, 75, 40], [85, 78, 25], [40, 40, 40]];
        let mut wrong = Vec::new();
        for bg in backgrounds {
            for r in [9.0f32, 14.0] {
                for filled in 0..=10u32 {
                    let mut img = RgbaImage::from_pixel(80, 48, Rgba([bg[0], bg[1], bg[2], 255]));
                    draw_pie(&mut img, 40.0, 24.0, r, filled, fm_color(filled), bg);
                    let th = r * 1.4; // labelhoogte op die schaal
                    let got = find_pie(&img, 0.0, 80.0, 24.0, th).map(|g| {
                        let (score, _) = gauge_score(g.fill, g.hue, g.fill > 0.0);
                        score.round() as u32
                    });
                    if got != Some(filled * 10) {
                        wrong.push(format!("bg {bg:?} r {r}: {filled} segmenten → {got:?}"));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "verkeerd gelezen:\n{}", wrong.join("\n"));
    }

    /// Grotere tekst in FM (of een kleinere rijafstand): de zoekband rond een label reikt dan tot in
    /// de meters van de rij erboven en eronder. Elke meter moet zijn eigen waarde houden.
    #[test]
    fn pies_in_tight_rows() {
        let bg = [70, 49, 53];
        let mut wrong = Vec::new();
        for r in [9.0f32, 14.0] {
            // rijafstand in FM: ~2,7 × de straal van een meter (38 px bij 14 px straal)
            let pitch = (r * 2.7).round();
            // labelhoogte / rijafstand: 0,53 op 2560×1440 met standaardtekst, groter met grotere tekst
            for ratio in [0.53f32, 0.62, 0.7, 0.8, 0.9, 1.0, 1.1] {
                let th = pitch * ratio;
                let rows = [7u32, 3, 9, 2, 5];
                let h = (pitch * (rows.len() as f32 + 1.0)) as u32;
                let mut img = RgbaImage::from_pixel(80, h, Rgba([bg[0], bg[1], bg[2], 255]));
                for (i, k) in rows.iter().enumerate() {
                    draw_pie(&mut img, 40.0, pitch * (i as f32 + 1.0), r, *k, fm_color(*k), bg);
                }
                for (i, k) in rows.iter().enumerate() {
                    let yc = pitch * (i as f32 + 1.0);
                    let got = find_pie(&img, 0.0, 80.0, yc, th).map(|g| gauge_score(g.fill, g.hue, g.fill > 0.0).0.round() as u32);
                    if got != Some(k * 10) {
                        wrong.push(format!("r {r} tekst/rij {ratio}: rij {i} met {k} segmenten → {got:?}"));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "verkeerd gelezen:
{}", wrong.join("
"));
    }

    #[test]
    fn colour_disagrees_with_fill() {
        // een oranje meter (laag) die als 70 % gemeten wordt: naar de rand van oranje, en nakijken
        assert_eq!(gauge_score(0.7, 43.0, true), (50.0, true));
        assert_eq!(gauge_score(0.4, 43.0, true), (40.0, false));
        assert_eq!(gauge_score(0.4, 59.0, true), (40.0, false), "40 % kan geel zijn (Ajax-save)");
        assert_eq!(gauge_score(0.9, 120.0, true), (90.0, false));
        assert_eq!(gauge_score(0.3, 120.0, true), (70.0, true));
        assert_eq!(gauge_score(0.0, 0.0, false), (0.0, false));
    }
}
