//! Portugese weergave van FM-termen (componenten, key factors, sessies, rollen, posities, moraal).
//! De Engelse namen blijven de sleutels in de code; dit wordt alleen bij het tonen gebruikt.
//! De termen volgen FM27 in het Portugees (zoals het spel ze toont).

use crate::i18n::{self, Lang};

/// Portugese naam van een FM-term, of de Engelse naam als er geen vertaling is of de taal niet PT is.
pub fn fm(en: &str) -> &str {
    if i18n::get() != Lang::Pt {
        return en;
    }
    pt(en).unwrap_or(en)
}

fn pt(en: &str) -> Option<&'static str> {
    Some(match en {
        // componenten
        "Tactical Understanding" => "Entendimento táctico",
        "Player Ability" => "Capacidade do jogador",
        "Player Buy-In" => "Adesão do jogador",
        "Next Match Info" => "Informações sobre o próximo jogo",
        // key factors
        "In Possession Shape" => "Formação com posse de bola",
        "Passing Style" => "Estilo de passe",
        "Build Up" => "Construção",
        "Chance Creation" => "Criação de oportunidades",
        "Out Of Possession Shape" => "Formação sem posse de bola",
        "Line of Engagement" => "Linha de pressão",
        "Pressing Intensity" => "Intensidade da pressão",
        "Pressing Trap" => "Armadilha de pressão",
        "Player Tactical Ability" => "Capacidade táctica do jogador",
        "Tactical Complexity" => "Complexidade táctica",
        "Player Capability" => "Capacidade do jogador",
        "Player Role Suitability" => "Aptidão para a função do jogador",
        "Managerial Support" => "Apoio ao treinador",
        "Teammates and Staff Relationships" => "Relações com colegas e equipa técnica",
        "Manager Tendencies" => "Tendências do treinador",
        "Quality of Next Opposition Report" => "Qualidade do relatório sobre o próximo adversário",
        "Opposition Player Knowledge" => "Conhecimento dos jogadores adversários",
        // categorieën
        "Match Preparation" => "Preparação para o jogo",
        "Footballing Development" => "Desenvolvimento futebolístico",
        "Physical and Recovery" => "Físico e recuperação",
        "Extra-Curricular" => "Extracurricular",
        "Match / Travel" => "Jogo / Viagem",
        // weekfocus
        "Balanced" => "Equilibrado",
        "In Possession" => "Com posse de bola",
        "Out Of Possession" => "Sem posse de bola",
        "Full Week of Rest" => "Semana completa de descanso",
        // sessies
        "Press Avoidance" => "Evitar fazer pressão",
        "Ball Progression" => "Progressão com bola",
        "Final Third" => "Terço final",
        "Defensive Block" => "Bloco defensivo",
        "Defensive Positioning" => "Posicionamento defensivo",
        "Defensive Engagement" => "Abordagem defensiva",
        "Corners" => "Cantos",
        "Free Kicks" => "Livres",
        "Penalties" => "Penáltis",
        "Individual Development" => "Desenvolvimento individual",
        "Attacking Ball Work" => "Trabalho de bola ofensivo",
        "Defensive Ball Work" => "Trabalho de bola defensivo",
        "Possession Ball Work" => "Trabalho de posse de bola",
        "Attacking - Balanced" => "Ataque - Equilibrado",
        "Attacking - Patient" => "Ataque - Paciente",
        "Attacking - Direct" => "Ataque - Directo",
        "Attacking - Wings" => "Ataque - Alas",
        "Defending - Balanced" => "Defesa - Equilibrada",
        "Defending - Engaged" => "Defesa - Pressionante",
        "Defending - Disengaged" => "Defesa - Recuada",
        "Defending - Wide" => "Defesa - Larga",
        "Theory - Attacking Shadow Play" => "Teoria - Jogo de sombra ofensivo",
        "Theory - Defensive Shadow Play" => "Teoria - Jogo de sombra defensivo",
        "Technical - Chance Creation" => "Técnico - Criação de oportunidades",
        "Technical - Chance Conversion" => "Técnico - Finalização",
        "Technical - Ball Retention" => "Técnico - Retenção de bola",
        "Technical - Ground Defence" => "Técnico - Defesa no chão",
        "Technical - Aerial Defence" => "Técnico - Defesa aérea",
        "Transition - Restrict" => "Transição - Conter",
        "Transition - Press" => "Transição - Pressionar",
        "Transition - Progress" => "Transição - Progredir",
        "GK - Balanced" => "GR - Equilibrado",
        "GK - Shot Stopping" => "GR - Defesa de remates",
        "GK - Handling" => "GR - Manuseamento",
        "GK - One On Ones" => "GR - Um para um",
        "GK - Distribution" => "GR - Distribuição",
        "Set Pieces - Delivery" => "Bolas paradas - Marcação",
        "Set Pieces - Penalties" => "Bolas paradas - Penáltis",
        "Rest" => "Descanso",
        "Recovery" => "Recuperação",
        "Light Conditioning" => "Condicionamento leve",
        "Conditioning" => "Condicionamento",
        "Heavy Conditioning" => "Condicionamento intenso",
        "Rest (IM)" => "Descanso (GI)",
        "Recovery (IM)" => "Recuperação (GI)",
        "Light Conditioning (IM)" => "Condicionamento leve (GI)",
        "Team Bonding" => "Coesão da equipa",
        "Community Outreach" => "Acção na comunidade",
        "Travel" => "Viagem",
        "Match" => "Jogo",
        "Not Available" => "Indisponível",
        "Tactical Understanding IP" => "Entendimento táctico (com posse)",
        "Tactical Understanding OOP" => "Entendimento táctico (sem posse)",
        // moraal
        "Abysmal" => "Péssima",
        "Very Poor" => "Muito fraca",
        "Poor" => "Fraca",
        "Okay" => "Razoável",
        "Good" => "Boa",
        "Very Good" => "Muito boa",
        "Superb" => "Excelente",
        // posities (plekken op de tactiekpagina)
        "GK" => "GR",
        "D (C)" => "D (C)",
        "D/WB (L)" => "D/DA (E)",
        "D/WB (R)" => "D/DA (D)",
        "D/WB" => "D/DA",
        "DM" => "MD",
        "DM/M (C)" => "MD/M (C)",
        "M (C)" => "M (C)",
        "M/AM (C)" => "M/MO (C)",
        "AM (C)" => "MO (C)",
        "AM/M (L)" => "MO/M (E)",
        "AM/M (R)" => "MO/M (D)",
        "AM/M (RL)" => "MO/M (DE)",
        "ST" => "PL",
        _ => return None,
    })
}

/// Korte sessienaam voor in de chips van de kalender (PT).
pub fn chip_pt(en: &str) -> Option<&'static str> {
    Some(match en {
        "In Possession Shape" => "Formação c/ posse",
        "Out Of Possession Shape" => "Formação s/ posse",
        "Chance Creation" => "Criação de oport.",
        "Pressing Intensity" => "Intens. da pressão",
        "Pressing Trap" => "Armadilha de pressão",
        "Press Avoidance" => "Evitar pressão",
        "Ball Progression" => "Progressão c/ bola",
        "Defensive Positioning" => "Posic. defensivo",
        "Defensive Engagement" => "Abord. defensiva",
        "Individual Development" => "Desenv. individual",
        "Attacking Ball Work" => "Bola ofensivo",
        "Defensive Ball Work" => "Bola defensivo",
        "Possession Ball Work" => "Posse de bola",
        "Light Conditioning" => "Cond. leve",
        "Heavy Conditioning" => "Cond. intenso",
        "Community Outreach" => "Comunidade",
        "Tactical Understanding IP" => "Ent. táctico (c/ posse)",
        "Tactical Understanding OOP" => "Ent. táctico (s/ posse)",
        _ => return None,
    })
}

/// Portugese afkorting en naam van een rol (met of zonder bal), op basis van de Engelse code.
pub fn role_pt(code: &str, name: &str) -> Option<(&'static str, &'static str)> {
    Some(match (code, name) {
        // met bal
        ("CB", _) => ("DC", "Defesa central"),
        ("FB", _) => ("DL", "Defesa lateral"),
        ("WB", _) => ("DA", "Defesa ala"),
        ("DM", _) => ("MD", "Médio defensivo"),
        ("W", _) => ("EX", "Extremo"),
        ("AM", _) => ("MO", "Médio ofensivo"),
        ("DLF", _) => ("AR", "Avançado recuado"),
        ("CFD", _) | ("CF", _) => ("PL", "Ponta de lança"),
        ("BGK", _) => ("GRJ", "Guarda-redes de construção"),
        ("ACB", _) => ("DCA", "Defesa central avançado"),
        ("DLP", _) => ("OR", "Organizador recuado"),
        ("BBM", _) => ("MBB", "Médio box-to-box"),
        ("AP", _) => ("OA", "Organizador avançado"),
        ("P", _) => ("AV", "Avançado de área"),
        ("BCB", _) => ("DCC", "Defesa central construtor"),
        ("IF", _) => ("AI", "Avançado interior"),
        ("NCB", _) => ("DCS", "Defesa central sem rodeios"),
        ("HB", _) => ("TR", "Trinco recuado"),
        ("AWB", _) => ("DAA", "Defesa ala avançado"),
        ("F9", _) => ("F9", "Falso nove"),
        ("PW", _) => ("EXO", "Extremo organizador"),
        ("IWB", _) => ("DAI", "Defesa ala interior"),
        ("WFD", _) => ("AL", "Avançado lateral"),
        ("BBP", _) => ("OBB", "Organizador box-to-box"),
        ("WCM", _) => ("MCA", "Médio centro aberto"),
        ("IFB", _) => ("DLI", "Defesa lateral interior"),
        ("NGK", _) => ("GRS", "Guarda-redes sem rodeios"),
        ("OCB", _) => ("DCP", "Defesa central de projecção"),
        ("PWB", _) => ("DAO", "Defesa ala organizador"),
        ("CHM", _) => ("MCO", "Médio de corredor"),
        ("MPM", _) => ("OM", "Organizador do meio-campo"),
        // zonder bal
        ("SK", _) => ("GRL", "Guarda-redes líbero"),
        ("LHK", _) => ("GRF", "Guarda-redes fixo na linha"),
        ("SCB", _) => ("DCB", "Defesa central de bloqueio"),
        ("CCB", _) => ("DCO", "Defesa central de cobertura"),
        ("WCB", _) => ("DCL", "Defesa central aberto"),
        ("PFB", _) => ("DLP", "Defesa lateral pressionante"),
        ("HFB", _) => ("DLF", "Defesa lateral fixo"),
        ("HWB", _) => ("DAF", "Defesa ala fixo"),
        ("DDM", _) => ("MDR", "Médio defensivo que recua"),
        ("PDM", _) => ("MDP", "Médio defensivo pressionante"),
        ("CM", _) => ("MC", "Médio centro"),
        ("PCM", _) => ("MCP", "Médio centro pressionante"),
        ("SCM", _) => ("MCB", "Médio centro de protecção"),
        ("OAM", _) => ("MOS", "Médio ofensivo de saída"),
        ("TW", _) => ("EXA", "Extremo que acompanha"),
        ("IOW", _) => ("EXS", "Extremo interior de saída"),
        ("TCF", _) => ("PLA", "Ponta de lança que acompanha"),
        ("OCF", _) => ("PLS", "Ponta de lança de saída"),
        _ => return None,
    })
}

/// Rol voor weergave: (afkorting, naam) in de huidige taal.
pub fn role(code: &str, name: &str) -> (String, String) {
    if i18n::get() == Lang::Pt {
        if let Some((_, n)) = role_pt(code, name) {
            return (n.to_string(), format!("{code} · {name}"));
        }
    }
    (code.to_string(), name.to_string())
}
