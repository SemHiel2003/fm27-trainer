//! Datamodel: de vier onderdelen van FM27 Tactical Effectiveness, de 17 key factors en de
//! trainingssessies. Namen zijn letterlijk zoals FM27 ze toont (uit de game-binary gehaald),
//! zodat ze in het spel één-op-één terug te vinden zijn.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Component {
    TacticalUnderstanding,
    PlayerAbility,
    PlayerBuyIn,
    NextMatchInfo,
}

impl Component {
    pub const ALL: [Component; 4] = [
        Component::TacticalUnderstanding,
        Component::PlayerAbility,
        Component::PlayerBuyIn,
        Component::NextMatchInfo,
    ];

    /// Titel zoals in FM27.
    pub fn label(self) -> &'static str {
        match self {
            Component::TacticalUnderstanding => "Tactical Understanding",
            Component::PlayerAbility => "Player Ability",
            Component::PlayerBuyIn => "Player Buy-In",
            Component::NextMatchInfo => "Next Match Info",
        }
    }

    /// Alternatieve schrijfwijzen die OCR kan opleveren.
    pub fn ocr_aliases(self) -> &'static [&'static str] {
        match self {
            Component::TacticalUnderstanding => &["tactical understanding"],
            Component::PlayerAbility => &["player ability"],
            Component::PlayerBuyIn => &["player buy-in", "player buy in", "player buyin"],
            Component::NextMatchInfo => &["next match info", "next match information"],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Factor {
    // Tactical Understanding → Tactical Areas (in balbezit)
    InPossessionShape,
    PassingStyle,
    BuildUp,
    ChanceCreation,
    // Tactical Understanding → Tactical Areas (zonder bal)
    OutOfPossessionShape,
    LineOfEngagement,
    PressingIntensity,
    PressingTrap,
    // Tactical Understanding → Tactical Comprehension
    PlayerTacticalAbility,
    TacticalComplexity,
    // Player Ability
    PlayerCapability,
    PlayerRoleSuitability,
    // Player Buy-In
    ManagerialSupport,
    TeammatesAndStaffRelationships,
    ManagerTendencies,
    // Next Match Info
    QualityOfNextOppositionReport,
    OppositionPlayerKnowledge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    InPossession,
    OutOfPossession,
}

impl Factor {
    pub const ALL: [Factor; 17] = [
        Factor::InPossessionShape,
        Factor::PassingStyle,
        Factor::BuildUp,
        Factor::ChanceCreation,
        Factor::OutOfPossessionShape,
        Factor::LineOfEngagement,
        Factor::PressingIntensity,
        Factor::PressingTrap,
        Factor::PlayerTacticalAbility,
        Factor::TacticalComplexity,
        Factor::PlayerCapability,
        Factor::PlayerRoleSuitability,
        Factor::ManagerialSupport,
        Factor::TeammatesAndStaffRelationships,
        Factor::ManagerTendencies,
        Factor::QualityOfNextOppositionReport,
        Factor::OppositionPlayerKnowledge,
    ];

    pub const TACTICAL_AREAS: [Factor; 8] = [
        Factor::InPossessionShape,
        Factor::PassingStyle,
        Factor::BuildUp,
        Factor::ChanceCreation,
        Factor::OutOfPossessionShape,
        Factor::LineOfEngagement,
        Factor::PressingIntensity,
        Factor::PressingTrap,
    ];

    /// Label zoals in het Breakdown-scherm van FM27.
    pub fn label(self) -> &'static str {
        match self {
            Factor::InPossessionShape => "In Possession Shape",
            Factor::PassingStyle => "Passing Style",
            Factor::BuildUp => "Build Up",
            Factor::ChanceCreation => "Chance Creation",
            Factor::OutOfPossessionShape => "Out Of Possession Shape",
            Factor::LineOfEngagement => "Line of Engagement",
            Factor::PressingIntensity => "Pressing Intensity",
            Factor::PressingTrap => "Pressing Trap",
            Factor::PlayerTacticalAbility => "Player Tactical Ability",
            Factor::TacticalComplexity => "Tactical Complexity",
            Factor::PlayerCapability => "Player Capability",
            Factor::PlayerRoleSuitability => "Player Role Suitability",
            Factor::ManagerialSupport => "Managerial Support",
            Factor::TeammatesAndStaffRelationships => "Teammates and Staff Relationships",
            Factor::ManagerTendencies => "Manager Tendencies",
            Factor::QualityOfNextOppositionReport => "Quality of Next Opposition Report",
            Factor::OppositionPlayerKnowledge => "Opposition Player Knowledge",
        }
    }

    pub fn component(self) -> Component {
        use Factor::*;
        match self {
            InPossessionShape | PassingStyle | BuildUp | ChanceCreation | OutOfPossessionShape
            | LineOfEngagement | PressingIntensity | PressingTrap | PlayerTacticalAbility
            | TacticalComplexity => Component::TacticalUnderstanding,
            PlayerCapability | PlayerRoleSuitability => Component::PlayerAbility,
            ManagerialSupport | TeammatesAndStaffRelationships | ManagerTendencies => {
                Component::PlayerBuyIn
            }
            QualityOfNextOppositionReport | OppositionPlayerKnowledge => Component::NextMatchInfo,
        }
    }

    pub fn is_tactical_area(self) -> bool {
        Factor::TACTICAL_AREAS.contains(&self)
    }

    pub fn phase(self) -> Option<Phase> {
        use Factor::*;
        match self {
            InPossessionShape | PassingStyle | BuildUp | ChanceCreation => Some(Phase::InPossession),
            OutOfPossessionShape | LineOfEngagement | PressingIntensity | PressingTrap => {
                Some(Phase::OutOfPossession)
            }
            _ => None,
        }
    }

    /// Wat je eraan kunt doen (kort).
    pub fn advice(self) -> &'static str {
        use Factor::*;
        match self {
            InPossessionShape | PassingStyle | BuildUp | ChanceCreation | OutOfPossessionShape
            | LineOfEngagement | PressingIntensity | PressingTrap => crate::t!(
                "Train de sessie met dezelfde naam (groep Tactical Understanding). Zet je Primary Trained Tactic op de tactiek die je speelt.",
                "Train the session with the same name (Tactical Understanding group). Set your Primary Trained Tactic to the tactic you play."
            ),
            PlayerTacticalAbility => crate::t!(
                "Hangt af van de spelers zelf (beslissingen, anticipatie, teamwork). Speel spelers met een hogere Tactic Ability of houd de instructies eenvoudiger.",
                "Depends on the players themselves (decisions, anticipation, teamwork). Pick players with higher Tactic Ability or keep instructions simpler."
            ),
            TacticalComplexity => crate::t!(
                "Te veel team-, speler- en tegenstanderinstructies. Schrap instructies die weinig opleveren en voeg ze later weer toe.",
                "Too many team, player and opposition instructions. Drop the ones that add little and bring them back later."
            ),
            PlayerCapability => crate::t!(
                "Spelers kunnen de rollen niet goed genoeg uitvoeren. Kies rollen die bij hun attributen passen of versterk de selectie.",
                "Players struggle to carry out their roles. Pick roles that suit their attributes or strengthen the squad."
            ),
            PlayerRoleSuitability => crate::t!(
                "Speel spelers op hun natuurlijke positie en rol. Check in Player View wie het cijfer omlaag trekt.",
                "Play people in their natural position and role. Use Player View to see who drags the rating down."
            ),
            ManagerialSupport => crate::t!(
                "Resultaten, goede teamtalks en positieve gesprekken met spelers verhogen de steun.",
                "Results, good team talks and positive player interactions raise support."
            ),
            TeammatesAndStaffRelationships => crate::t!(
                "Plan Team Bonding (Extra-Curricular). Houd de kern van de selectie bij elkaar en vermijd onrust.",
                "Schedule Team Bonding (Extra-Curricular). Keep the core of the squad together and avoid unrest."
            ),
            ManagerTendencies => crate::t!(
                "Spelers verwachten de speelstijl van je managerprofiel. Hoe meer je tactiek afwijkt, hoe lager dit.",
                "Players expect the style of your manager profile. The further your tactic strays from it, the lower this is."
            ),
            QualityOfNextOppositionReport => crate::t!(
                "Laat je analist/scouts de volgende tegenstander rapporteren en neem goede analisten aan.",
                "Have your analysts/scouts report on the next opponent, and hire good analysts."
            ),
            OppositionPlayerKnowledge => crate::t!(
                "Groeit door scouting van de tegenstander en hun spelers. Laat onbekende (Europese) tegenstanders vooraf scouten.",
                "Grows by scouting the opponent and their players. Scout unknown (European) opponents in advance."
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    TacticalUnderstanding,
    MatchPreparation,
    FootballingDevelopment,
    PhysicalRecovery,
    ExtraCurricular,
    Fixed,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::TacticalUnderstanding => "Tactical Understanding",
            Category::MatchPreparation => "Match Preparation",
            Category::FootballingDevelopment => "Footballing Development",
            Category::PhysicalRecovery => "Physical and Recovery",
            Category::ExtraCurricular => "Extra-Curricular",
            Category::Fixed => "Match / Travel",
        }
    }

    /// Kleur voor de GUI en de HTML-export (RGB).
    pub fn color(self) -> [u8; 3] {
        match self {
            Category::TacticalUnderstanding => [139, 124, 255],
            Category::MatchPreparation => [76, 178, 255],
            Category::FootballingDevelopment => [61, 214, 140],
            Category::PhysicalRecovery => [245, 181, 68],
            Category::ExtraCurricular => [244, 114, 182],
            Category::Fixed => [255, 92, 108],
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Session {
    pub name: &'static str,
    pub category: Category,
    /// Maximaal aantal keer per week (uit de game-data).
    pub max_per_week: i32,
    /// Effect op [conditie, vermoeidheid, geluk, blessurerisico, scherpte], −3…+3, zoals
    /// FM27 het in de sessie-info toont (positief = gunstig; uit de game-data).
    pub effects: [i8; 5],
}

const fn s(name: &'static str, category: Category, max_per_week: i32, effects: [i8; 5]) -> Session {
    Session { name, category, max_per_week, effects }
}

/// Alle 59 sessies van FM27, letterlijk uit `training/training_sessions` in de game-data
/// (game_plugin.bundle/…/game_simulation/training.fmf), in dezelfde volgorde.
pub const SESSIONS: &[Session] = &[
    s("Passing Style", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Build Up", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Chance Creation", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("In Possession Shape", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Line of Engagement", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Pressing Intensity", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Pressing Trap", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Out Of Possession Shape", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Press Avoidance", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Ball Progression", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Final Third", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Defensive Block", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Defensive Positioning", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Defensive Engagement", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Corners", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Free Kicks", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Penalties", Category::MatchPreparation, 1, [-1, 0, 0, -1, 2]),
    s("Individual Development", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Attacking Ball Work", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Defensive Ball Work", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 1]),
    s("Possession Ball Work", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Attacking - Balanced", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Attacking - Patient", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Attacking - Direct", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Attacking - Wings", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Defending - Balanced", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Defending - Engaged", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Defending - Disengaged", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Defending - Wide", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Theory - Attacking Shadow Play", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Theory - Defensive Shadow Play", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Technical - Chance Creation", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Technical - Chance Conversion", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Technical - Ball Retention", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Technical - Ground Defence", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Technical - Aerial Defence", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Transition - Restrict", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Transition - Press", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Transition - Progress", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("GK - Balanced", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("GK - Shot Stopping", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("GK - Handling", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("GK - One On Ones", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("GK - Distribution", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Set Pieces - Delivery", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Set Pieces - Penalties", Category::FootballingDevelopment, 7, [-1, -1, 1, -1, 0]),
    s("Rest", Category::PhysicalRecovery, 21, [3, 1, 2, 0, -3]),
    s("Recovery", Category::PhysicalRecovery, 7, [1, 2, 1, 1, -2]),
    s("Light Conditioning", Category::PhysicalRecovery, 7, [-1, -1, -1, -2, -1]),
    s("Conditioning", Category::PhysicalRecovery, 7, [-3, -2, -2, -2, -1]),
    s("Heavy Conditioning", Category::PhysicalRecovery, 7, [-3, -3, -3, -2, -1]),
    s("Rest (IM)", Category::PhysicalRecovery, 21, [0, 0, 0, 0, 0]),
    s("Recovery (IM)", Category::PhysicalRecovery, 7, [0, 0, 0, 0, 0]),
    s("Light Conditioning (IM)", Category::PhysicalRecovery, 7, [0, 0, 0, 0, 0]),
    s("Team Bonding", Category::ExtraCurricular, 2, [1, 0, 3, 0, -3]),
    s("Community Outreach", Category::ExtraCurricular, 2, [1, 0, 3, 0, -3]),
    s("Travel", Category::ExtraCurricular, 21, [1, 0, 0, 0, -3]),
    s("Match", Category::Fixed, 7, [0, 0, 0, 0, 0]),
    s("Not Available", Category::PhysicalRecovery, 21, [0, 0, 0, 0, 0]),
    // Algemene keuzes die de SI-presets gebruiken: FM vult zelf de specifieke sessie in
    s("Match Preparation", Category::MatchPreparation, 9, [-1, 0, 0, -1, 2]),
    s("Tactical Understanding IP", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
    s("Tactical Understanding OOP", Category::TacticalUnderstanding, 8, [-1, 0, 0, -1, 1]),
];

impl Session {
    /// Belasting 0 (rust) … 6 (zwaar): hoe negatief het effect op conditie en vermoeidheid is.
    pub fn load(&self) -> i32 {
        (-(self.effects[0] as i32) - self.effects[1] as i32).max(0)
    }
}

pub fn session(name: &str) -> Option<&'static Session> {
    SESSIONS.iter().find(|s| s.name == name)
}

/// Interne sessiecode van FM27 (zoals in de save en het geheugen): Tactical 0–7, Match
/// Preparation 100–108, Footballing Development 200–228, Physical 300–308, Extra 400–402,
/// Match 500. Afgeleid van de volgorde in de game-data en gecontroleerd tegen de kalender in
/// een echte save (bv. dag [200, 3, 7] = Individual Development / In Possession Shape / Out Of
/// Possession Shape). De algemene keuzes ("Match Preparation", "Tactical Understanding IP")
/// hebben geen eigen code.
pub fn session_code(name: &str) -> Option<u16> {
    let idx = SESSIONS.iter().position(|s| s.name == name)?;
    match idx {
        0..=7 => Some(idx as u16),
        8..=16 => Some(100 + (idx - 8) as u16),
        17..=45 => Some(200 + (idx - 17) as u16),
        46..=53 => Some(300 + (idx - 46) as u16),
        54..=56 => Some(400 + (idx - 54) as u16),
        57 => Some(500),
        _ => None,
    }
}

pub fn session_by_code(code: u16) -> Option<&'static str> {
    SESSIONS.iter().map(|s| s.name).find(|n| session_code(n) == Some(code))
}

pub fn category_of(name: &str) -> Category {
    session(name).map(|s| s.category).unwrap_or(Category::Fixed)
}

/// Uitgelezen of handmatig ingevulde stand van alle factors (0–100) en onderdelen.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effectiveness {
    pub factors: Vec<(Factor, f32)>,
    pub components: Vec<(Component, f32)>,
    pub source: String,
    /// Spel-datum waarop de Breakdown is uitgelezen (als FM met de module draaide).
    #[serde(default)]
    pub game_date: Option<chrono::NaiveDate>,
}

impl Default for Effectiveness {
    fn default() -> Self {
        Effectiveness {
            factors: Factor::ALL.iter().map(|f| (*f, 50.0)).collect(),
            components: Component::ALL.iter().map(|c| (*c, 50.0)).collect(),
            source: String::new(),
            game_date: None,
        }
    }
}

impl Effectiveness {
    pub fn get(&self, f: Factor) -> f32 {
        self.factors.iter().find(|(x, _)| *x == f).map(|(_, v)| *v).unwrap_or(50.0)
    }

    pub fn set(&mut self, f: Factor, v: f32) {
        if let Some(e) = self.factors.iter_mut().find(|(x, _)| *x == f) {
            e.1 = v.clamp(0.0, 100.0);
        } else {
            self.factors.push((f, v.clamp(0.0, 100.0)));
        }
    }

    pub fn component(&self, c: Component) -> f32 {
        if let Some((_, v)) = self.components.iter().find(|(x, _)| *x == c) {
            return *v;
        }
        let vals: Vec<f32> =
            Factor::ALL.iter().filter(|f| f.component() == c).map(|f| self.get(*f)).collect();
        vals.iter().sum::<f32>() / vals.len().max(1) as f32
    }

    /// Factors gesorteerd van zwak naar sterk.
    pub fn weakest(&self) -> Vec<(Factor, f32)> {
        let mut v: Vec<(Factor, f32)> = Factor::ALL.iter().map(|f| (*f, self.get(*f))).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    pub fn phase_score(&self, p: Phase) -> f32 {
        let v: Vec<f32> = Factor::TACTICAL_AREAS
            .iter()
            .filter(|f| f.phase() == Some(p))
            .map(|f| self.get(*f))
            .collect();
        v.iter().sum::<f32>() / v.len().max(1) as f32
    }

    pub fn buy_in(&self) -> f32 {
        self.get(Factor::TeammatesAndStaffRelationships)
            .min(self.get(Factor::ManagerialSupport))
    }
}

/// Kleurklasse van een meter: rood → groen, zoals FM27 ze toont.
pub fn level_name(score: f32) -> &'static str {
    match score {
        s if s < 25.0 => crate::t!("zeer laag", "very low"),
        s if s < 45.0 => crate::t!("laag", "low"),
        s if s < 60.0 => crate::t!("matig", "fair"),
        s if s < 80.0 => crate::t!("goed", "good"),
        _ => crate::t!("uitstekend", "excellent"),
    }
}

pub fn level_color(score: f32) -> [u8; 3] {
    // rood (0) → oranje → geel → limoen → groen (100), zoals de FM27-meters
    let stops: [(f32, [f32; 3]); 5] = [
        (0.0, [230.0, 60.0, 50.0]),
        (35.0, [240.0, 130.0, 30.0]),
        (55.0, [235.0, 215.0, 40.0]),
        (75.0, [170.0, 220.0, 50.0]),
        (100.0, [60.0, 200.0, 90.0]),
    ];
    let s = score.clamp(0.0, 100.0);
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if s <= b {
            let t = (s - a) / (b - a);
            return [
                (ca[0] + (cb[0] - ca[0]) * t) as u8,
                (ca[1] + (cb[1] - ca[1]) * t) as u8,
                (ca[2] + (cb[2] - ca[2]) * t) as u8,
            ];
        }
    }
    [60, 200, 90]
}
