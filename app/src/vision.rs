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

/// Vindt de OCR-regel voor een label; geeft de regel en de x waar het label eindigt.
fn find_label<'a>(lines: &'a [OcrLine], label: &str) -> Option<(&'a OcrLine, f32)> {
    let want = norm(label);
    let mut best: Option<(i32, &OcrLine, f32)> = None;
    for l in lines {
        let t = norm(&l.text);
        let Some(pos) = t.find(&want) else { continue };
        // label moet op woordgrenzen staan
        let before_ok = pos == 0 || t.as_bytes()[pos - 1] == b' ';
        let end = pos + want.len();
        let after_ok = end == t.len() || t.as_bytes()[end] == b' ';
        if !before_ok || !after_ok {
            continue;
        }
        let extra = (t.len() - want.len()) as i32;
        let score = if extra == 0 { 1000 } else { 500 - extra };
        let n = t.chars().count().max(1) as f32;
        let end_frac = t[..end].chars().count() as f32 / n;
        let label_end = l.x0 + (l.x1 - l.x0) * end_frac;
        if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
            best = Some((score, l, label_end));
        }
    }
    best.map(|(_, l, e)| (l, e))
}

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
    s >= 0.42 && v >= 0.42 && (h <= 160.0 || h >= 345.0)
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
    let strict: Box<Pred> = Box::new(|p| is_colored(p) || is_light(p));
    let loose: Box<Pred> = Box::new(|p| is_colored(p) || is_gauge_grey(p));
    find_pie_with(img, x_start, x_limit, yc, th, &*strict, &is_light).or_else(|| find_pie_with(img, x_start, x_limit, yc, th, &*loose, &is_gauge_grey))
}

/// Kleine gaten (de donkere spaken tussen de taartpunten) breken een meter niet op.
fn find_pie_with(img: &RgbaImage, x_start: f32, x_limit: f32, yc: f32, th: f32, part: &Pred, empty: &Pred) -> Option<GaugeShape> {
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

    let mut run: Option<(u32, u32)> = None; // (begin, laatste treffer)
    for x in xs..xe {
        let hit = column_hits(img, x, y0, y1, part) >= need;
        if hit {
            run = Some(match run {
                Some((rs, last)) if x - last <= max_gap + 1 => (rs, x),
                Some((rs, last)) => {
                    let len = last + 1 - rs;
                    if len >= min_run && len <= max_run {
                        if let Some(g) = measure_pie(img, rs, last + 1, y0, y1, empty) {
                            return Some(g);
                        }
                    }
                    (x, x)
                }
                None => (x, x),
            });
        }
    }
    if let Some((rs, last)) = run {
        let len = last + 1 - rs;
        if len >= min_run && len <= max_run {
            return measure_pie(img, rs, last + 1, y0, y1, empty);
        }
    }
    None
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
fn measure_pie(img: &RgbaImage, xa: u32, xb: u32, y0: u32, y1: u32, empty: &Pred) -> Option<GaugeShape> {
    // verticale grenzen van de meter binnen de kolommen
    let (mut ty0, mut ty1) = (u32::MAX, 0u32);
    for y in y0..y1 {
        if (xa..xb).any(|x| {
            let p = img.get_pixel(x, y);
            is_colored(p) || empty(p)
        }) {
            ty0 = ty0.min(y);
            ty1 = ty1.max(y);
        }
    }
    if ty0 == u32::MAX {
        return None;
    }
    let cx = (xa as f32 + xb as f32 - 1.0) / 2.0;
    let cy = (ty0 as f32 + ty1 as f32) / 2.0;
    let r = ((xb - xa) as f32).max((ty1 - ty0 + 1) as f32) / 2.0;

    const BINS: usize = 36;
    let mut col = [0u32; BINS];
    let mut gry = [0u32; BINS];
    let (mut hx, mut hy, mut ncol) = (0f32, 0f32, 0u32);
    for y in ty0..=ty1 {
        for x in xa..xb {
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
            let p = img.get_pixel(x, y);
            if is_colored(p) {
                col[bin] += 1;
                let (h, _, _) = hsv(p);
                hx += h.to_radians().cos();
                hy += h.to_radians().sin();
                ncol += 1;
            } else if empty(p) {
                gry[bin] += 1;
            }
        }
    }
    let seen = (0..BINS).filter(|i| col[*i] + gry[*i] > 0).count();
    if seen < BINS / 2 {
        return None;
    }
    // De vulling loopt aaneengesloten met de klok mee vanaf 12 uur: de laatste sector met echt
    // gekleurde pixels bepaalt het gevulde deel. De lichte spaken tussen de punten (en het lichte
    // kruis in het midden) tellen zo niet als leeg.
    let mut fill = 0.0f32;
    for i in (0..BINS).rev() {
        let total = (col[i] + gry[i]).max(1);
        if col[i] >= 3 && col[i] * 4 >= total {
            let frac = (col[i] as f32 / total as f32).min(1.0);
            fill = (i as f32 + frac) / BINS as f32;
            break;
        }
    }
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

/// Meter-score: het gevulde deel van de taart (4 van de 10 segmenten = 40). De tint loopt daar
/// in FM mee op (oranje laag, geel midden, groen hoog) en wordt alleen nog ter informatie bewaard.
fn gauge_score(fill: f32, _hue: f32, has_color: bool) -> f32 {
    if !has_color {
        return 0.0;
    }
    (fill * 100.0).clamp(0.0, 100.0)
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
    let need = ((th * 0.45) as u32).max(3);
    let mut marker: Vec<u32> = Vec::new();
    let mut slots: Vec<u32> = Vec::new();
    for x in xs..xe {
        let (mut c, mut g, mut l) = (0, 0, 0);
        for y in y0..y1 {
            let p = img.get_pixel(x, y);
            if is_colored(p) {
                c += 1;
            } else if is_gauge_grey(p) {
                g += 1;
            } else if is_light(p) {
                l += 1;
            }
        }
        if c >= need {
            marker.push(x);
        } else if g >= need || l >= need {
            slots.push(x);
        }
    }
    if marker.is_empty() {
        return None;
    }
    let m = marker.iter().sum::<u32>() as f32 / marker.len() as f32;
    // Kolommen samenvoegen tot streepjes (middelpunten); brede grijze vlakken (de rand van een
    // paneel, een icoon) zijn geen streepjes.
    let max_width = (th * 0.35).max(5.0);
    let mut centers: Vec<f32> = Vec::new();
    let mut run: Option<(u32, u32)> = None;
    let mut flush = |run: Option<(u32, u32)>, centers: &mut Vec<f32>| {
        if let Some((a, b)) = run {
            if (b - a + 1) as f32 <= max_width {
                centers.push((a + b) as f32 / 2.0);
            }
        }
    };
    for x in slots {
        run = match run {
            Some((a, b)) if x == b + 1 => Some((a, x)),
            other => {
                flush(other, &mut centers);
                Some((x, x))
            }
        };
    }
    flush(run, &mut centers);
    centers.push(m);
    centers.sort_by(|a, b| a.total_cmp(b));
    centers.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    // Vanaf de marker naar buiten lopen zolang de afstand tussen streepjes regelmatig blijft:
    // de rand van de balk en losse vlakken verderop liggen verder uit elkaar en vallen af.
    let mi = centers.iter().position(|c| (*c - m).abs() < 1.0)?;
    let gaps: Vec<f32> = centers.windows(2).map(|w| w[1] - w[0]).collect();
    let mut sorted = gaps.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let typical = *sorted.get(sorted.len() / 2)?;
    let limit = typical * 1.6;
    let (mut lo_i, mut hi_i) = (mi, mi);
    while lo_i > 0 && centers[lo_i] - centers[lo_i - 1] <= limit {
        lo_i -= 1;
    }
    while hi_i + 1 < centers.len() && centers[hi_i + 1] - centers[hi_i] <= limit {
        hi_i += 1;
    }
    let (lo, hi) = (centers[lo_i], centers[hi_i]);
    if hi_i - lo_i < 3 || typical < 2.0 {
        return None;
    }
    // De stand is de plek van de gekleurde streep, geteld vanaf het eerste streepje. FM27 tekent
    // 11 plekken; staat een deel van de balk in ruis (de doorschijnende achtergrond van het paneel),
    // dan blijft dat aantal het uitgangspunt.
    let chain = ((hi - lo) / typical).round() as usize + 1;
    let n = if chain >= 10 { chain } else { 11 };
    let idx = ((m - lo) / typical).round().max(0.0);
    Some((idx / (n - 1) as f32 * 100.0).clamp(0.0, 100.0))
}

// ---------------------------------------------------------------- analyse

pub fn analyze_image(img: &RgbaImage) -> Result<(Analysis, Vec<OcrLine>)> {
    let lines = ocr(img)?;
    let analysis = analyze_with_lines(img, &lines);
    Ok((analysis, lines))
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
                let score = gauge_score(g.fill, g.hue, g.fill > 0.0);
                gauges.push(GaugeReading { factor: f, score, fill: g.fill, hue: g.hue, bbox: g.bbox });
            }
            None => missing.push(f),
        }
    }

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
