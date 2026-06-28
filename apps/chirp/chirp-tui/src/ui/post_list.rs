//! Approach-b Home: left-pane post list.
//!
//! Three-row card layout per root post:
//!   row 1: gutter + author (colored) + " · " + relative timestamp
//!   row 2: gutter + body text
//!   row 3: gutter + ♥ N · ↺ N · 💬 N

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, Pane};
use crate::timeline::{RowRelationCount, RowRelationCounts, TimelineRow};
use crate::ui::colors::{
    author_color, fmt_count, format_age, ACCENT_CYAN, BODY_TEXT, DIMMER_TEXT, DIM_TEXT, HEART,
    LIST_BG, REPLY_COLOR, REPOST, SELECTED_BG,
};
use crate::ui::nostr_content::nostr_minimal_content::NostrMinimalContent;
use crate::ui::nostr_user::profile_name_span;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focused == Pane::Feed;
    let border_color = if focused { ACCENT_CYAN } else { DIMMER_TEXT };
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(LIST_BG));

    // Account for the 1-col right border when computing pane width.
    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = build_lines(state, pane_width);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

fn build_lines(state: &AppState, pane_width: usize) -> Vec<Line<'static>> {
    if state.rows.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Waiting for timeline events...",
                Style::default().fg(DIM_TEXT),
            )),
        ];
    }

    // Collect indices of depth-0 rows (root posts) in the timeline.
    let root_indices: Vec<usize> = state
        .rows
        .iter()
        .enumerate()
        .filter_map(|(idx, row)| (row.depth == 0).then_some(idx))
        .collect();

    // Determine the selected root: either the row matching state.selected
    // exactly, or the most recent root before it (the parent of the selected
    // reply).
    let selected_root_idx = nearest_root(&root_indices, state.selected);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for &row_idx in &root_indices {
        let row = &state.rows[row_idx];
        let is_selected = Some(row_idx) == selected_root_idx;
        append_card(&mut lines, row, is_selected, pane_width);
    }
    lines
}

fn nearest_root(root_indices: &[usize], selected: usize) -> Option<usize> {
    if root_indices.is_empty() {
        return None;
    }
    // Largest root index that is <= selected, else first root.
    root_indices
        .iter()
        .rev()
        .find(|&&idx| idx <= selected)
        .copied()
        .or_else(|| root_indices.first().copied())
}

fn append_card(
    lines: &mut Vec<Line<'static>>,
    row: &TimelineRow,
    selected: bool,
    pane_width: usize,
) {
    let row_bg = if selected { SELECTED_BG } else { LIST_BG };
    let gutter_span = if selected {
        Span::styled(
            "\u{2503} ", // ┃
            Style::default().fg(ACCENT_CYAN).bg(row_bg),
        )
    } else {
        Span::styled("  ", Style::default().bg(row_bg))
    };

    let gutter_width = 2usize;
    let content_width = pane_width.saturating_sub(gutter_width);

    // Row 0 (reposts only): "↻ <reposter> reposted <age>"
    if let Some(repost) = row.repost.as_ref() {
        let prefix = "\u{21BA} ";
        let prefix_len = prefix.chars().count();
        let suffix = " reposted ";
        let suffix_len = suffix.chars().count();
        let repost_age = format_age(repost.repost_created_at);
        let repost_age_len = repost_age.chars().count();
        let name_budget = content_width
            .saturating_sub(prefix_len)
            .saturating_sub(suffix_len)
            .saturating_sub(repost_age_len);
        let (reposter_span, reposter_len) = profile_name_span(
            &repost.author_profile,
            Style::default().fg(DIM_TEXT).bg(row_bg),
            name_budget,
        );
        let used = prefix_len + reposter_len + suffix_len + repost_age_len;
        let pad = pad_for(content_width, used);
        let line0 = Line::from(vec![
            gutter_span.clone(),
            Span::styled(prefix.to_string(), Style::default().fg(REPOST).bg(row_bg)),
            reposter_span,
            Span::styled(suffix.to_string(), Style::default().fg(DIM_TEXT).bg(row_bg)),
            Span::styled(repost_age, Style::default().fg(DIM_TEXT).bg(row_bg)),
            Span::styled(pad, Style::default().bg(row_bg)),
        ]);
        lines.push(line0);
    }

    // Row 0b (V-80 OP-centric feed): "↳ <name> replied in thread" — surfaces
    // the most-recent follow whose reply caused this root to appear (or who
    // replied to it). The feed is thread-roots-only: replies never get their
    // own row, they attribute back here. Q1 display decision: the TUI shows
    // ONLY the most-recent 1 replier even though the projection carries them
    // all raw. Co-occurs with the repost banner above (L-4): banner first,
    // then attribution, then the author line.
    if let Some(attribution) = row.thread_attribution.last() {
        let prefix = "\u{21B3} ";
        let prefix_len = prefix.chars().count();
        let suffix = " replied in thread";
        let suffix_len = suffix.chars().count();
        let name_budget = content_width
            .saturating_sub(prefix_len)
            .saturating_sub(suffix_len);
        let (name_span, name_len) = profile_name_span(
            &attribution.author_profile,
            Style::default().fg(REPLY_COLOR).bg(row_bg),
            name_budget,
        );
        let used = prefix_len + name_len + suffix_len;
        let pad = pad_for(content_width, used);
        lines.push(Line::from(vec![
            gutter_span.clone(),
            Span::styled(prefix.to_string(), Style::default().fg(REPLY_COLOR).bg(row_bg)),
            name_span,
            Span::styled(
                suffix.to_string(),
                Style::default().fg(DIM_TEXT).bg(row_bg),
            ),
            Span::styled(pad, Style::default().bg(row_bg)),
        ]));
    }

    // Row 1: author · timestamp
    let author_style = Style::default()
        .fg(author_color(&row.author_pubkey))
        .bg(row_bg)
        .add_modifier(Modifier::BOLD);
    let (author_span, author_len) = profile_name_span(
        &row.author_profile,
        author_style,
        content_width.saturating_sub(8),
    );
    let sep_span = Span::styled(" \u{00b7} ", Style::default().fg(DIM_TEXT).bg(row_bg));
    let age = format_age(row.created_at);
    let age_span = Span::styled(age.clone(), Style::default().fg(DIM_TEXT).bg(row_bg));
    let line1_text_len = author_len + 3 + age.chars().count();
    let line1_pad = pad_for(content_width, line1_text_len);
    let line1 = Line::from(vec![
        gutter_span.clone(),
        author_span,
        sep_span,
        age_span,
        Span::styled(line1_pad, Style::default().bg(row_bg)),
    ]);
    lines.push(line1);

    // Row 2: body
    let body = content_preview(row, content_width);
    let body_len = body.chars().count();
    let body_pad = pad_for(content_width, body_len);
    let line2 = Line::from(vec![
        gutter_span.clone(),
        Span::styled(body, Style::default().fg(BODY_TEXT).bg(row_bg)),
        Span::styled(body_pad, Style::default().bg(row_bg)),
    ]);
    lines.push(line2);

    // Row 3: reaction bar
    let (reaction_spans, reaction_len) = reaction_spans(&row.relation_counts, row_bg);
    let reaction_pad = pad_for(content_width, reaction_len);
    let mut line3_spans = vec![gutter_span];
    line3_spans.extend(reaction_spans);
    line3_spans.push(Span::styled(reaction_pad, Style::default().bg(row_bg)));
    lines.push(Line::from(line3_spans));
}

fn reaction_spans(
    counts: &RowRelationCounts,
    bg: ratatui::style::Color,
) -> (Vec<Span<'static>>, usize) {
    let reactions = count_value(&counts.reactions);
    let reposts = count_value(&counts.reposts);
    let replies = count_value(&counts.replies);
    let zaps = count_value(&counts.zaps);
    let r1 = fmt_count(reactions);
    let r2 = fmt_count(reposts);
    let r3 = fmt_count(replies);
    let r4 = fmt_count(zaps);
    let dot = " \u{00b7} ";

    let segs = vec![
        Span::styled("\u{2665} ", Style::default().fg(HEART).bg(bg)),
        Span::styled(r1.clone(), Style::default().fg(HEART).bg(bg)),
        Span::styled(dot, Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled("\u{21ba} ", Style::default().fg(REPOST).bg(bg)),
        Span::styled(r2.clone(), Style::default().fg(REPOST).bg(bg)),
        Span::styled(dot, Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled("\u{1f4ac} ", Style::default().fg(REPLY_COLOR).bg(bg)),
        Span::styled(r3.clone(), Style::default().fg(REPLY_COLOR).bg(bg)),
        Span::styled(dot, Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled("\u{26a1} ", Style::default().fg(ACCENT_CYAN).bg(bg)),
        Span::styled(r4.clone(), Style::default().fg(ACCENT_CYAN).bg(bg)),
    ];
    // Approximate character width for padding.
    let len = 2
        + r1.chars().count()
        + dot.chars().count()
        + 2
        + r2.chars().count()
        + dot.chars().count()
        + 2
        + r3.chars().count()
        + dot.chars().count()
        + 2
        + r4.chars().count();
    (segs, len)
}

fn count_value(count: &RowRelationCount) -> u64 {
    match count {
        RowRelationCount::Known(n) => *n,
        RowRelationCount::Loading => 0,
    }
}

fn pad_for(width: usize, used: usize) -> String {
    if width > used {
        " ".repeat(width - used)
    } else {
        String::new()
    }
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

fn content_preview(row: &TimelineRow, width: usize) -> String {
    if let Some(tree) = &row.content_tree {
        let line = NostrMinimalContent::new(tree)
            .render_data(Some(&row.content_render))
            .lines(width)
            .into_iter()
            .next()
            .unwrap_or_else(|| Line::from(""));
        let text = line
            .spans
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        return truncate(&text, width);
    }
    truncate(&row.content.replace('\n', " "), width)
}
