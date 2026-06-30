use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub enum ParsedMessage {
    Log(Line<'static>),
    Fps(f32),
    State(String),
    Config { mode: String, profiling: bool },
}

pub fn parse(raw: String) -> ParsedMessage {
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
