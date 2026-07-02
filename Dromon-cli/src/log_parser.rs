use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub enum ParsedMessage {
    Log(Line<'static>),
    Fps(f32),
    State(String),
    Config { mode: String, profiling: bool },
    /// Arbre d'initialisation complet, reçu en un message, déjà en ordre d'affichage.
    ProfileInit(Vec<ProfileNode>),
    /// Arbre d'une frame prélevée (même format), renvoyé périodiquement.
    ProfileFrame(Vec<ProfileNode>),
    /// Arbre des temps GPU (timestamps), renvoyé régulièrement.
    ProfileGpu(Vec<ProfileNode>),
}

/// Un nœud d'arbre de profiling tel que décodé d'un message `[PROFILE_*]`.
pub struct ProfileNode {
    pub depth: usize,
    pub dur_us: u128,
    pub count: u32,
    pub name: String,
}

pub fn parse(raw: String) -> ParsedMessage {
    if let Some(rest) = raw.strip_prefix("[PROFILE_INIT] ") {
        return ParsedMessage::ProfileInit(parse_profile_nodes(rest));
    }
    if let Some(rest) = raw.strip_prefix("[PROFILE_FRAME] ") {
        return ParsedMessage::ProfileFrame(parse_profile_nodes(rest));
    }
    if let Some(rest) = raw.strip_prefix("[PROFILE_GPU] ") {
        return ParsedMessage::ProfileGpu(parse_profile_nodes(rest));
    }
    if let Some(rest) = raw.strip_prefix("[FPS] ") {
        if let Ok(fps) = rest.trim().parse::<f32>() {
            return ParsedMessage::Fps(fps);
        }
    }
    if let Some(rest) = raw.strip_prefix("[STATE] ") {
        return ParsedMessage::State(rest.trim().to_string());
    }
    if let Some(rest) = raw.strip_prefix("[CONFIG] ") {
        let mut mode = String::new();
        let mut profiling = false;
        for tok in rest.split_whitespace() {
            if let Some(v) = tok.strip_prefix("mode=") {
                mode = v.to_string();
            } else if let Some(v) = tok.strip_prefix("profiling=") {
                profiling = v == "enabled";
            }
        }
        return ParsedMessage::Config { mode, profiling };
    }
    ParsedMessage::Log(parse_log(raw))
}

/// Décode le corps d'un message `[PROFILE_*]` : l'arbre entier en une passe.
/// Les nœuds sont séparés par `\x1e` (record) et leurs 4 champs
/// `depth␟dur_us␟count␟name` par `\x1f` — des caractères de contrôle qui
/// n'apparaissent jamais dans un nom, donc pas d'ambiguïté même si le nom
/// contient des espaces ou des `::`. Les nœuds arrivent déjà en ordre préfixe.
fn parse_profile_nodes(rest: &str) -> Vec<ProfileNode> {
    rest.split('\u{1e}')
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let mut fields = record.splitn(4, '\u{1f}');
            let depth = fields.next()?.parse().ok()?;
            let dur_us = fields.next()?.parse().ok()?;
            let count = fields.next()?.parse().ok()?;
            let name = fields.next()?.to_string();
            Some(ProfileNode {
                depth,
                dur_us,
                count,
                name,
            })
        })
        .collect()
}

fn parse_log(raw: String) -> Line<'static> {
    if let Some(rest) = raw.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let tag = &rest[..end];
            let color = tag_color(tag);
            let after = rest[end + 1..].to_string();
            return Line::from(vec![
                Span::styled(
                    format!("[{tag}]"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(after),
            ]);
        }
    }
    Line::raw(raw)
}

fn tag_color(tag: &str) -> Color {
    match tag.to_ascii_uppercase().as_str() {
        "INFO" => Color::Rgb(0x56, 0x9C, 0xD6),
        "WARN" | "WARNING" => Color::Rgb(0xFF, 0xC6, 0x6D),
        "ERROR" => Color::Rgb(0xFF, 0x45, 0x00),
        "VULKAN" => Color::Rgb(0xBD, 0x93, 0xF9),
        _ => Color::Rgb(0xBF, 0xBD, 0xB6),
    }
}
