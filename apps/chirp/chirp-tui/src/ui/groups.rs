//! Approach-b Groups tab: 2-pane split — unified group list + group chat.
//!
//! Left pane (38%): a **unified** list of NIP-29 public groups AND Marmot MLS
//! encrypted groups in a single scrollable view.  Type indicators distinguish them:
//!   - NIP-29 public:  `[#]`  in REPLY_COLOR (blue)
//!   - Marmot MLS:     `[E]`  in ZAP (yellow)
//!
//! Currently the snapshot only provides `discovered_groups` (NIP-29).  The
//! unified-list machinery is already in place so MLS groups can be added later
//! by extending `GroupKind`/`build_unified_list` without touching the renderer.
//!
//! Right pane (62%): group chat transcript + optional inline compose strip.
//!
//! Compose strip fields (`group_composing`, `group_compose_buf`) are plumbed
//! through file-local helpers returning stub values until the companion wiring
//! PR adds them to `AppState`.
//!
//! Per-message timestamps are omitted: `MessageLine` does not yet expose
//! `created_at`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::feature_snapshot::{GroupLine, MessageLine};
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, DIMMER_TEXT, LIST_BG, REPLY_COLOR, SELECTED_BG,
    ZAP, author_color,
};

/// Whether the inline group compose strip is open.
/// TODO(wiring): replace body with `state.group_composing`
fn is_composing(_state: &AppState) -> bool { false }

/// Current text in the group compose buffer.
/// TODO(wiring): replace body with `state.group_compose_buf.as_str()`
fn compose_buf(_state: &AppState) -> &str { "" }

/// Protocol type for a group entry in the unified list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    /// NIP-29 public relay group
    Nip29,
    /// Marmot MLS encrypted group (placeholder — no snapshot source yet).
    #[allow(dead_code)]
    Mls,
}

/// A group row in the unified list, borrowing from FeatureSnapshot.
struct UnifiedGroup<'a> {
    kind: GroupKind,
    line: &'a GroupLine,
}

/// Build the unified list from whatever sources are available.
/// Currently only NIP-29 groups exist in the snapshot.
/// MLS groups: extend here when FeatureSnapshot gains `mls_groups: Vec<GroupLine>`.
fn build_unified_list(state: &AppState) -> Vec<UnifiedGroup<'_>> {
    state.features.discovered_groups.iter()
        .map(|g| UnifiedGroup { kind: GroupKind::Nip29, line: g })
        .collect()
}

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    render_group_list(frame, cols[0], state);
    render_group_chat(frame, cols[1], state);
}

fn render_group_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let border_color = ACCENT_CYAN; // TODO(wiring): use DIMMER_TEXT when pane is unfocused
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Groups ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let pane_width = block.inner(area).width as usize;
    let groups = build_unified_list(state);
    let lines = if groups.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No groups  \u{00b7}  n to discover or join",
                Style::default().fg(DIM_TEXT),
            )),
        ]
    } else {
        let mut all: Vec<Line<'static>> = Vec::new();
        for (i, entry) in groups.iter().enumerate() {
            let selected = i == state.group_selected;
            append_group_card(&mut all, entry, selected, pane_width);
        }
        all
    };

    frame.render_widget(Paragraph::new(lines).block(block).style(Style::default().bg(LIST_BG).fg(BODY_TEXT)), area);
}

fn append_group_card(
    lines: &mut Vec<Line<'static>>,
    entry: &UnifiedGroup<'_>,
    selected: bool,
    pane_width: usize,
) {
    let group = entry.line;
    let row_bg = if selected { SELECTED_BG } else { LIST_BG };
    let gutter = if selected {
        Span::styled("\u{2503} ", Style::default().fg(ACCENT_CYAN).bg(row_bg))
    } else {
        Span::styled("  ", Style::default().bg(row_bg))
    };
    let content_width = pane_width.saturating_sub(2);
    // Type indicator: "[#]" for NIP-29, "[E]" for MLS
    let (indicator, indicator_color) = match entry.kind {
        GroupKind::Nip29 => ("[#] ", REPLY_COLOR),
        GroupKind::Mls => ("[E] ", ZAP),
    };
    let indicator_len = indicator.chars().count(); // 4
    // Row 1: indicator + name (bold) + padding + member count (dim) + open/closed badge
    let open_badge = if group.open { " open" } else { " closed" };
    let members_str = format!(" {} ", group.member_count);
    let right_len = members_str.chars().count() + open_badge.chars().count();
    let name = truncate(&group.name, content_width.saturating_sub(indicator_len).saturating_sub(right_len));
    let mid_pad = content_width.saturating_sub(indicator_len + name.chars().count() + right_len);
    lines.push(Line::from(vec![
        gutter.clone(),
        Span::styled(indicator, Style::default().fg(indicator_color).bg(row_bg)),
        Span::styled(name, Style::default().fg(BODY_TEXT).bg(row_bg).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(mid_pad), Style::default().bg(row_bg)),
        Span::styled(members_str, Style::default().fg(DIM_TEXT).bg(row_bg)),
        Span::styled(open_badge, Style::default().fg(if group.open { ACCENT_CYAN } else { DIMMER_TEXT }).bg(row_bg)),
    ]));
    // Row 2: relay URL (NIP-29) or "encrypted" (MLS)
    let row2_text = match entry.kind {
        GroupKind::Nip29 => truncate(&group.host_relay_url, content_width),
        GroupKind::Mls => "encrypted".to_string(),
    };
    let row2_len = row2_text.chars().count();
    lines.push(Line::from(vec![
        gutter,
        Span::styled(row2_text, Style::default().fg(DIM_TEXT).bg(row_bg)),
        Span::styled(" ".repeat(content_width.saturating_sub(row2_len)), Style::default().bg(row_bg)),
    ]));
}

fn render_group_chat(frame: &mut Frame, area: Rect, state: &AppState) {
    let (msg_area, compose_area_opt) = if is_composing(state) && area.height > 3 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let selected_group = state.features.discovered_groups.get(state.group_selected);
    let title_str = selected_group.map(|g| format!(" {} ", g.name)).unwrap_or_else(|| " Chat ".to_string());
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(title_str, Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)));
    let pane_width = block.inner(msg_area).width as usize;
    let lines = if state.features.group_messages.is_empty() {
        empty_state("  No messages yet")
    } else {
        build_message_lines(&state.features.group_messages, pane_width)
    };
    frame.render_widget(Paragraph::new(lines).block(block).style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT)), msg_area);
    if let Some(compose_area) = compose_area_opt {
        render_compose_strip(frame, compose_area, state, selected_group.map(|g| g.name.as_str()).unwrap_or("?"));
    }
}

fn build_message_lines(messages: &[MessageLine], pane_width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for msg in messages.iter().take(30) {
        // Format: "@short_name  ›  content"
        let label = format!("@{}  \u{203a}  ", short_author(&msg.author));
        let body = truncate(&msg.content.replace('\n', " "), pane_width.saturating_sub(label.chars().count()));
        out.push(Line::from(vec![
            Span::styled(label, Style::default().fg(author_color(&msg.author))),
            Span::styled(body, Style::default().fg(BODY_TEXT)),
        ]));
        out.push(Line::from(""));
    }
    out
}

fn render_compose_strip(frame: &mut Frame, area: Rect, state: &AppState, group_name: &str) {
    let buf = compose_buf(state);
    let w = area.width.saturating_sub(4) as usize;
    let lines = vec![
        Line::from(Span::styled(
            format!("\u{251c}\u{2500} {}", truncate(&format!(" Compose to {group_name} "), w)),
            Style::default().fg(ACCENT_CYAN),
        )),
        Line::from(vec![
            Span::styled("\u{2502} ", Style::default().fg(ACCENT_CYAN)),
            Span::styled(truncate(&format!("> {buf}\u{2588}"), w), Style::default().fg(BODY_TEXT)),
        ]),
        Line::from(Span::styled(
            format!("\u{2514}\u{2500} {}", truncate(" Ctrl+Enter send  Esc cancel ", w)),
            Style::default().fg(ACCENT_CYAN),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(DETAIL_BG)), area);
}

fn empty_state(msg: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(msg, Style::default().fg(DIM_TEXT))),
    ]
}

fn short_author(value: &str) -> String {
    const MAX: usize = 12;
    if value.chars().count() <= MAX { return value.to_string(); }
    let prefix: String = value.chars().take(6).collect();
    let suffix: String = value.chars().rev().take(4).collect::<String>().chars().rev().collect();
    format!("{prefix}..{suffix}")
}

fn truncate(value: &str, max: usize) -> String {
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
