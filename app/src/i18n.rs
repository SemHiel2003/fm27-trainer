//! Nederlands / Engels / Português. Teksten staan naast elkaar in de code: `t!("Nederlands", "English", "Português")`.
//! FM-termen (sessienamen, factors) blijven Engels, zoals in het spel.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Nl,
    En,
    Pt,
}

static LANG: AtomicU8 = AtomicU8::new(2);

pub fn set(l: Lang) {
    LANG.store(
        match l {
            Lang::Nl => 0,
            Lang::En => 1,
            Lang::Pt => 2,
        },
        Ordering::Relaxed,
    );
}

pub fn get() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        0 => Lang::Nl,
        1 => Lang::En,
        _ => Lang::Pt,
    }
}

pub fn is_nl() -> bool {
    get() == Lang::Nl
}

pub fn is_pt() -> bool {
    get() == Lang::Pt
}

/// Taal van het systeem: Nederlands of Engels als de voorkeurstaal dat is, anders Português.
pub fn detect() -> Lang {
    match sys_locale::get_locale() {
        Some(l) if l.to_lowercase().starts_with("nl") => Lang::Nl,
        Some(l) if l.to_lowercase().starts_with("en") => Lang::En,
        _ => Lang::Pt,
    }
}

/// Kiest de variant van de huidige taal; werkt voor `&str` en `String`.
/// Zonder Portugese tekst valt het terug op Engels.
#[macro_export]
macro_rules! t {
    ($nl:expr, $en:expr, $pt:expr $(,)?) => {
        match $crate::i18n::get() {
            $crate::i18n::Lang::Nl => $nl,
            $crate::i18n::Lang::En => $en,
            $crate::i18n::Lang::Pt => $pt,
        }
    };
    ($nl:expr, $en:expr $(,)?) => {
        if $crate::i18n::is_nl() { $nl } else { $en }
    };
}

/// Maandnamen voor datums in de UI.
pub fn month_short(m: u32) -> &'static str {
    const NL: [&str; 12] = ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"];
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const PT: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
    let i = (m.clamp(1, 12) - 1) as usize;
    match get() {
        Lang::Nl => NL[i],
        Lang::En => EN[i],
        Lang::Pt => PT[i],
    }
}

pub fn weekday_short(i: usize) -> &'static str {
    const NL: [&str; 7] = ["ma", "di", "wo", "do", "vr", "za", "zo"];
    const EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const PT: [&str; 7] = ["Seg", "Ter", "Qua", "Qui", "Sex", "Sáb", "Dom"];
    match get() {
        Lang::Nl => NL[i % 7],
        Lang::En => EN[i % 7],
        Lang::Pt => PT[i % 7],
    }
}

pub fn date(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    match get() {
        Lang::En => format!("{} {} {}", month_short(d.month()), d.day(), d.year()),
        _ => format!("{} {} {}", d.day(), month_short(d.month()), d.year()),
    }
}

pub fn date_short(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    match get() {
        Lang::En => format!("{} {}", month_short(d.month()), d.day()),
        _ => format!("{} {}", d.day(), month_short(d.month())),
    }
}
