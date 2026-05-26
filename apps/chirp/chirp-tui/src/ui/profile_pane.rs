//! Rich profile pane — renders in the left column when Pane::Profile is focused.
//!
//! Layout (vertical split):
//!   - Top 8 rows: profile header (avatar block + name/npub + bio + stats)
//!   - Remaining rows: author's posts from the current timeline

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DIM_TEXT, DIMMER_TEXT, LIST_BG, author_color, format_age,
};
use crate::ui::nostr_user::profile_name_span;

const HEADER_HEIGHT: u16 = 8;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let sections = Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),
        Constraint::Min(0),
    ])
    .split(area);

    render_header(f, sections[0], state);
    render_post_list(f, sections[1], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let pubkey = &state.profile_pubkey;
    let avatar_color = author_color(pubkey);

    // Extract profile data from feature snapshot.
    let (display_name, about, note_count) =
        if let Some(profile) = &state.features.author_profile {
            let name = if profile.display.is_empty() {
                short_pubkey(pubkey)
            } else {
                profile.display.clone()
            };
            let bio = if profile.about.is_empty() {
                String::new()
            } else {
                profile.about.clone()
            };
            let count = profile.note_count.clone();
            (name, bio, count)
        } else {
            (short_pubkey(pubkey), String::new(), String::new())
        };

    let sections = Layout::vertical([
        Constraint::Length(3), // avatar block
        Constraint::Length(1), // name + npub
        Constraint::Length(2), // bio (up to 2 lines)
        Constraint::Length(2), // stats
    ])
    .split(area);

    // Avatar block — fill with colored "██" blocks, overlay name centered.
    let avatar_bg = avatar_color;
    let avatar_fill = "\u{2588}\u{2588}".repeat((area.width as usize / 2).max(1));
    let avatar_block = Block::default()
        .style(Style::default().bg(avatar_bg));
    let name_centered = Paragraph::new(display_name.clone())
        .style(
            Style::default()
                .fg(Color::White)
                .bg(avatar_bg)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(ratatui::layout::Alignment::Center)
        .block(avatar_block);
    // Render the colored fill as background first, then overlay the name.
    let fill_line = truncate_to_width(&avatar_fill, area.width as usize);
    let fill_para = Paragraph::new(vec![
        Line::from(Span::styled(fill_line.clone(), Style::default().fg(avatar_bg).bg(avatar_bg))),
        Line::from(Span::styled(fill_line.clone(), Style::default().fg(avatar_bg).bg(avatar_bg))),
        Line::from(Span::styled(fill_line, Style::default().fg(avatar_bg).bg(avatar_bg))),
    ]);
    f.render_widget(fill_para, sections[0]);
    f.render_widget(name_centered, sections[0]);

    // Name + short pubkey line.
    let npub_short = short_pubkey(pubkey);
    let name_line = Line::from(vec![
        Span::styled(
            truncate_to_width(&display_name, (area.width as usize).saturating_sub(npub_short.len() + 2)),
            Style::default()
                .fg(author_color(pubkey))
                .bg(LIST_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default().bg(LIST_BG)),
        Span::styled(
            npub_short,
            Style::default().fg(DIMMER_TEXT).bg(LIST_BG),
        ),
    ]);
    f.render_widget(
        Paragraph::new(name_line).style(Style::default().bg(LIST_BG)),
        sections[1],
    );

    // Bio — up to 2 lines.
    let bio_text = if about.is_empty() {
        vec![
            Line::from(Span::styled("no bio", Style::default().fg(DIMMER_TEXT).bg(LIST_BG))),
            Line::from(""),
        ]
    } else {
        let bio_truncated = truncate_to_width(&about, area.width as usize);
        vec![
            Line::from(Span::styled(bio_truncated, Style::default().fg(DIM_TEXT).bg(LIST_BG))),
            Line::from(""),
        ]
    };
    f.render_widget(
        Paragraph::new(bio_text).style(Style::default().bg(LIST_BG)),
        sections[2],
    );

    // Stats.
    let follow_count = state.features.follow_count;
    let notes_label = if note_count.is_empty() {
        "\u{2014}".to_string()
    } else {
        note_count
    };
    let stats_line1 = Line::from(vec![
        Span::styled("Following ", Style::default().fg(DIM_TEXT).bg(LIST_BG)),
        Span::styled(
            format!("{}", follow_count),
            Style::default().fg(BODY_TEXT).bg(LIST_BG).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  Notes ", Style::default().fg(DIM_TEXT).bg(LIST_BG)),
        Span::styled(
            notes_label,
            Style::default().fg(BODY_TEXT).bg(LIST_BG).add_modifier(Modifier::BOLD),
        ),
    ]);
    let border_color = ACCENT_CYAN;
    let stats_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(LIST_BG));
    f.render_widget(
        Paragraph::new(vec![stats_line1]).block(stats_block),
        sections[3],
    );
}

fn render_post_list(f: &mut Frame, area: Rect, state: &AppState) {
    let pubkey = &state.profile_pubkey;
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(DIMMER_TEXT))
        .style(Style::default().bg(LIST_BG));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = build_author_post_lines(state, pubkey, pane_width);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

fn build_author_post_lines(
    state: &AppState,
    pubkey: &str,
    pane_width: usize,
) -> Vec<Line<'static>> {
    if pubkey.is_empty() {
        return vec![Line::from(Span::styled(
            "  No profile loaded",
            Style::default().fg(DIM_TEXT),
        ))];
    }

    let author_rows: Vec<_> = state
        .rows
        .iter()
        .filter(|r| r.depth == 0 && r.author_pubkey == pubkey)
        .collect();

    if author_rows.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No posts in current timeline",
                Style::default().fg(DIM_TEXT),
            )),
        ];
    }

    let gutter_width = 2usize;
    let content_width = pane_width.saturating_sub(gutter_width);

    let mut lines = Vec::new();
    for row in author_rows {
        let gutter = Span::styled("  ", Style::default().bg(LIST_BG));

        // Row 1: author · timestamp
        let age = format_age(row.created_at);
        let author_style = Style::default()
            .fg(author_color(&row.author_pubkey))
            .bg(LIST_BG)
            .add_modifier(Modifier::BOLD);
        let (author_span, author_len) = profile_name_span(
            &row.author_profile,
            author_style,
            content_width.saturating_sub(8),
        );
        let sep = Span::styled(" \u{00b7} ", Style::default().fg(DIM_TEXT).bg(LIST_BG));
        let age_span = Span::styled(age.clone(), Style::default().fg(DIM_TEXT).bg(LIST_BG));
        let used = author_len + 3 + age.chars().count();
        let pad = pad_to(content_width, used);
        lines.push(Line::from(vec![
            gutter.clone(),
            author_span,
            sep,
            age_span,
            Span::styled(pad, Style::default().bg(LIST_BG)),
        ]));

        // Row 2: body
        let body = truncate_to_width(&row.content.replace('\n', " "), content_width);
        let body_len = body.chars().count();
        let body_pad = pad_to(content_width, body_len);
        lines.push(Line::from(vec![
            gutter.clone(),
            Span::styled(body, Style::default().fg(BODY_TEXT).bg(LIST_BG)),
            Span::styled(body_pad, Style::default().bg(LIST_BG)),
        ]));

        // Row 3: separator
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(pane_width),
            Style::default().fg(DIMMER_TEXT).bg(LIST_BG),
        )));
    }
    lines
}

/// Truncate a string to fit in `max` columns (appending ellipsis if truncated).
fn truncate_to_width(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= max {
        value.to_string()
    } else if max <= 1 {
        value.chars().take(max).collect()
    } else {
        let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

fn pad_to(width: usize, used: usize) -> String {
    if width > used {
        " ".repeat(width - used)
    } else {
        String::new()
    }
}

/// Short version of a pubkey for display: first 8 + "…" + last 4.
fn short_pubkey(pubkey: &str) -> String {
    if pubkey.len() < 12 {
        return pubkey.to_string();
    }
    format!("{}…{}", &pubkey[..8], &pubkey[pubkey.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_pubkey_formats_correctly() {
        let pk = "abcdefgh12345678abcdefgh12345678abcdefgh12345678abcdefgh12345678";
        let result = short_pubkey(pk);
        assert!(result.starts_with("abcdefgh"));
        assert!(result.ends_with("5678"));
        assert!(result.contains('\u{2026}'));
    }

    #[test]
    fn short_pubkey_handles_short_input() {
        assert_eq!(short_pubkey("short"), "short");
    }

    #[test]
    fn truncate_to_width_appends_ellipsis() {
        let s = "hello world";
        let result = truncate_to_width(s, 8);
        assert_eq!(result.chars().count(), 8);
        assert!(result.ends_with('\u{2026}'));
    }
}
