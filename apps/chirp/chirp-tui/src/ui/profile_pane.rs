//! Rich profile pane — renders in the left column when Pane::Profile is focused.
//!
//! Layout (vertical split):
//!   - Top 9 rows: profile header (avatar block + name/npub + nip05 + bio + stats)
//!   - Remaining rows: author's opened feed rows

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{
    author_color, format_age, ACCENT_CYAN, BODY_TEXT, DIMMER_TEXT, DIM_TEXT, LIST_BG,
};
use crate::ui::nostr_user::nostr_avatar::NostrAvatar;
use crate::ui::nostr_user::nostr_nip05_badge::NostrNip05Badge;
use crate::ui::nostr_user::profile_name_span;
use crate::ui::nostr_user::profile_wire::ProfileWire;

const HEADER_HEIGHT: u16 = 9;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let sections =
        Layout::vertical([Constraint::Length(HEADER_HEIGHT), Constraint::Min(0)]).split(area);

    render_header(f, sections[0], state);
    render_post_list(f, sections[1], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let pubkey = &state.profile_pubkey;

    let profile = profile_for_header(state);
    let display_name = profile
        .display_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| profile.npub_short.clone());
    let about = profile.about.as_deref().unwrap_or("").to_string();
    let note_count_n = state
        .profile_rows
        .iter()
        .filter(|r| r.depth == 0 && r.author_pubkey == *pubkey)
        .count();
    let note_count = if note_count_n > 0 {
        note_count_n.to_string()
    } else {
        String::new()
    };

    let sections = Layout::vertical([
        Constraint::Length(3), // avatar (NostrAvatar widget)
        Constraint::Length(1), // name + npub
        Constraint::Length(1), // NIP-05 badge (or empty)
        Constraint::Length(2), // bio (up to 2 lines)
        Constraint::Length(2), // stats
    ])
    .split(area);

    // Avatar — registry NostrAvatar widget: initials in a pubkey-keyed colored
    // bordered tile. Pass image=None (TUI has no profile-image cache yet).
    f.render_widget(NostrAvatar::new(&profile).image(None), sections[0]);

    // Name + short pubkey line.
    let npub_short = profile.npub_short.clone();
    let name_line = Line::from(vec![
        Span::styled(
            truncate_to_width(
                &display_name,
                (area.width as usize).saturating_sub(npub_short.len() + 2),
            ),
            Style::default()
                .fg(author_color(pubkey))
                .bg(LIST_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default().bg(LIST_BG)),
        Span::styled(npub_short, Style::default().fg(DIMMER_TEXT).bg(LIST_BG)),
    ]);
    f.render_widget(
        Paragraph::new(name_line).style(Style::default().bg(LIST_BG)),
        sections[1],
    );

    // NIP-05 badge — registry NostrNip05Badge; empty row when absent.
    let nip05_line = NostrNip05Badge::from_profile(&profile)
        .map(|badge| badge.line())
        .unwrap_or_else(|| Line::from(""));
    f.render_widget(
        Paragraph::new(nip05_line).style(Style::default().bg(LIST_BG)),
        sections[2],
    );

    // Bio — up to 2 lines.
    let bio_text = if about.is_empty() {
        vec![
            Line::from(Span::styled(
                "no bio",
                Style::default().fg(DIMMER_TEXT).bg(LIST_BG),
            )),
            Line::from(""),
        ]
    } else {
        let bio_truncated = truncate_to_width(&about, area.width as usize);
        vec![
            Line::from(Span::styled(
                bio_truncated,
                Style::default().fg(DIM_TEXT).bg(LIST_BG),
            )),
            Line::from(""),
        ]
    };
    f.render_widget(
        Paragraph::new(bio_text).style(Style::default().bg(LIST_BG)),
        sections[3],
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
            Style::default()
                .fg(BODY_TEXT)
                .bg(LIST_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  Notes ", Style::default().fg(DIM_TEXT).bg(LIST_BG)),
        Span::styled(
            notes_label,
            Style::default()
                .fg(BODY_TEXT)
                .bg(LIST_BG)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let border_color = ACCENT_CYAN;
    let stats_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(LIST_BG));
    f.render_widget(
        Paragraph::new(vec![stats_line1]).block(stats_block),
        sections[4],
    );
}

fn profile_for_header(state: &AppState) -> ProfileWire {
    let pubkey = &state.profile_pubkey;

    // ADR-0063 (#1671 Lane F): profile display data comes from the kernel-owned
    // `refs.profile` row (the resolve_ref output), read via the RefRowCache
    // mirror. The open profile pane resolve_ref's this pubkey at profile.card /
    // Live (see runtime.rs), so the full card lands here. Opened author-feed rows
    // remain a fallback while a fresh profile resolution is still absent.
    if let Some(profile) = state.profile(pubkey) {
        return profile;
    }
    let row_wire = state
        .profile_rows
        .iter()
        .find(|r| r.author_pubkey == *pubkey)
        .map(|r| r.author_profile.clone());
    if let Some(profile) = row_wire {
        return profile;
    }

    // Build a minimal ProfileWire for the avatar when no rows are available yet.
    ProfileWire {
        pubkey: pubkey.clone(),
        display_name: None,
        about: None,
        picture_url: None,
        nip05: None,
        npub: pubkey.clone(),
        npub_short: short_pubkey(pubkey),
    }
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
        .profile_rows
        .iter()
        .filter(|r| r.depth == 0 && r.author_pubkey == pubkey)
        .collect();

    if author_rows.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No posts in opened author feed",
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
    use crate::timeline::TimelineRow;

    fn row(id: &str, author: &str, content: &str) -> TimelineRow {
        let snapshot = serde_json::json!({
            "cards": [{
                "card": {
                    "id": id,
                    "author_pubkey": author,
                    "kind": 1,
                    "created_at": 1_700_000_000_u64,
                    "content": content,
                    "relation_counts": {}
                },
                "attribution": []
            }]
        });
        TimelineRow::from_snapshot(&snapshot)
            .into_iter()
            .next()
            .expect("row decodes")
    }

    fn lines_text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    }

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

    #[test]
    fn author_post_lines_use_opened_author_feed_not_home_rows() {
        let author = "aa".repeat(32);
        let mut state = AppState {
            profile_pubkey: author.clone(),
            rows: vec![row("home-row", &author, "home row body")],
            profile_rows: vec![row("author-row", &author, "author feed body")],
            ..Default::default()
        };

        let text = lines_text(&build_author_post_lines(&state, &author, 80));

        assert!(text.contains("author feed body"));
        assert!(!text.contains("home row body"));
        state.profile_rows.clear();
        let empty_text = lines_text(&build_author_post_lines(&state, &author, 80));
        assert!(empty_text.contains("No posts in opened author feed"));
    }

    /// ADR-0063 (#1671 Lane F): a `kind:0` ingest arrives as a `refs.profile`
    /// row-delta, is merged into the shell's `RefProfileStore`, and the profile
    /// header renders from it (the resolve_ref output) — preferred over the
    /// stale feed-row author metadata. This is the rendered-profile-updates-via-
    /// refs.profile gate for the open profile pane.
    #[test]
    fn header_prefers_refs_profile_card_over_feed_row_metadata() {
        use nmp_core::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch};
        use nmp_core::typed_projections::{encode_profile, ProfileCardModel};

        let author = "aa".repeat(32);
        let mut state = AppState {
            profile_pubkey: author.clone(),
            profile_rows: vec![row("author-row", &author, "author feed body")],
            ..Default::default()
        };
        state.profile_rows[0].author_profile.display_name = Some("Stale Row Name".to_string());

        // Build the refs.profile sidecar payload the kernel emits for this pubkey
        // after a kind:0 ingest (a KPRF card wrapped in a baseline NRRD batch).
        let card = encode_profile(&ProfileCardModel {
            pubkey: author.clone(),
            display_name: Some("Kernel Name".to_string()),
            about: "kernel bio".to_string(),
            nip05: "kernel@example.test".to_string(),
            picture_url: Some("https://example.test/avatar.png".to_string()),
            ..Default::default()
        });
        let batch = encode_ref_row_delta_batch(&RefRowDeltaBatch {
            namespace: "profile".to_string(),
            baseline: true,
            rows: vec![RefRow::changed(author.clone(), 1, card)],
        });
        state.ref_profiles.apply_sidecar(&batch, 1, 0);

        let profile = profile_for_header(&state);

        assert_eq!(profile.display_name.as_deref(), Some("Kernel Name"));
        assert_eq!(profile.about.as_deref(), Some("kernel bio"));
        assert_eq!(profile.nip05.as_deref(), Some("kernel@example.test"));
        assert_ne!(profile.display_name.as_deref(), Some("Stale Row Name"));
    }
}
