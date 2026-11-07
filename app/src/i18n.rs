//! Nederlands / Engels. Teksten staan naast elkaar in de code: `t!("Nederlands", "English")`.
//! FM-termen (sessienamen, factors) blijven Engels, zoals in het spel.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Nl,
    En,
}

static LANG: AtomicU8 = AtomicU8::new(1);

pub fn set(l: Lang) {
    LANG.store(if l == Lang::Nl { 0 } else { 1 }, Ordering::Relaxed);
}

pub fn get() -> Lang {
    if LANG.load(Ordering::Relaxed) == 0 { Lang::Nl } else { Lang::En }
}

pub fn is_nl() -> bool {
    get() == Lang::Nl
}

/// Taal van het systeem: Nederlands als de voorkeurstaal Nederlands is, anders Engels.
pub fn detect() -> Lang {
    match sys_locale::get_locale() {
        Some(l) if l.to_lowercase().starts_with("nl") => Lang::Nl,
        _ => Lang::En,
    }
}

/// Kiest de Nederlandse of Engelse variant; werkt voor `&str` en `String`.
#[macro_export]
macro_rules! t {
    ($nl:expr, $en:expr $(,)?) => {
        if $crate::i18n::is_nl() { $nl } else { $en }
    };
}

/// Maandnamen voor datums in de UI.
pub fn month_short(m: u32) -> &'static str {
    const NL: [&str; 12] = ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"];
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let i = (m.clamp(1, 12) - 1) as usize;
    if is_nl() { NL[i] } else { EN[i] }
}

pub fn weekday_short(i: usize) -> &'static str {
    const NL: [&str; 7] = ["ma", "di", "wo", "do", "vr", "za", "zo"];
    const EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    if is_nl() { NL[i % 7] } else { EN[i % 7] }
}

pub fn date(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    if is_nl() {
        format!("{} {} {}", d.day(), month_short(d.month()), d.year())
    } else {
        format!("{} {} {}", month_short(d.month()), d.day(), d.year())
    }
}

pub fn date_short(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    if is_nl() { format!("{} {}", d.day(), month_short(d.month())) } else { format!("{} {}", month_short(d.month()), d.day()) }
}
