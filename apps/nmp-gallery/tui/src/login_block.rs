use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};

pub fn plain_lines() -> Vec<String> {
    vec![
        "NostrLoginBlock".to_string(),
        "Signer detection and manual key-entry login UI.".to_string(),
    ]
}

pub fn render(area: Rect, buf: &mut Buffer) {
    Paragraph::new(vec![
        Line::from("NostrLoginBlock"),
        Line::from("Signer detection and manual key-entry login UI."),
    ])
    .render(area, buf);
}
