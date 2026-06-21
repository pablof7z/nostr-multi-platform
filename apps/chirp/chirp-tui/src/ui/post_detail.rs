//! Approach-b Home: right-pane thread detail view.
//!
//! Shows the opened thread feed when one is active. Without an opened thread it
//! falls back to the selected home row plus its consecutive reply rows.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{AppState, Pane};
use crate::timeline::{RowRelationCount, RowRelationCounts, TimelineRow};
use crate::ui::colors::{
    author_color, fmt_count, format_age, ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIMMER_TEXT, DIM_TEXT,
    HEART, REPLY_COLOR, REPOST, SELECTED_BG,
};
use crate::ui::layout::RenderContext;
use crate::ui::nostr_content::nostr_content_view::NostrContentView;
use crate::ui::nostr_user::profile_name_span;
use crate::ui::post_detail_rich::render_rich;

pub fn render(f: &mut Frame, area: Rect, state: &AppState, context: &RenderContext<'_>) {
    let focused = state.focused == Pane::Detail;
    let border_color = if focused { ACCENT_CYAN } else { DIMMER_TEXT };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(DETAIL_BG));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    if state.thread_event_id.is_empty() && !context.media_images.is_empty() {
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(DETAIL_BG)),
            inner,
        );
        render_rich(f, inner, state, context);
        return;
    }

    let lines = build_lines(state, pane_width);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT))
        .wrap(Wrap { trim: false })
        .scroll((state.detail_scroll, 0));
    f.render_widget(paragraph, area);
}

fn build_lines(state: &AppState, pane_width: usize) -> Vec<Line<'static>> {
    if !state.thread_event_id.is_empty() {
        return build_open_thread_lines(state, pane_width);
    }

    let Some(root) = root_for_selection(state) else {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Select a note to view the thread.",
                Style::default().fg(DIM_TEXT),
            )),
        ];
    };

    let focused = state.focused == Pane::Detail;
    let mut lines = Vec::new();

    // Main post: detail_cursor == 0 means it is highlighted (when focused).
    let main_selected = focused && state.detail_cursor == 0;
    append_main_post(&mut lines, root.row, main_selected, pane_width);

    // Replies: consecutive depth > 0 rows after root.row index.
    let replies = collect_replies(state, root.row_idx);
    if !replies.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "\u{2500}\u{2500}\u{2500} Replies \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            Style::default().fg(DIM_TEXT),
        )));
        lines.push(Line::from(""));
    }

    for (reply_index, (_, reply)) in replies.iter().enumerate() {
        let selected = focused && state.detail_cursor == reply_index + 1;
        append_reply(&mut lines, reply, selected, pane_width);
    }

    lines
}

fn build_open_thread_lines(state: &AppState, pane_width: usize) -> Vec<Line<'static>> {
    if state.thread_rows.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Loading opened thread feed.",
                Style::default().fg(DIM_TEXT),
            )),
        ];
    }

    let focused = state.focused == Pane::Detail;
    let mut lines = Vec::new();
    for (index, row) in state.thread_rows.iter().enumerate() {
        append_main_post(
            &mut lines,
            row,
            focused && state.detail_cursor == index,
            pane_width,
        );
        if index + 1 < state.thread_rows.len() {
            lines.push(Line::from(""));
        }
    }
    lines
}

pub(super) struct SelectedRoot<'a> {
    pub(super) row: &'a TimelineRow,
    pub(super) row_idx: usize,
}

pub(super) fn root_for_selection(state: &AppState) -> Option<SelectedRoot<'_>> {
    if state.rows.is_empty() {
        return None;
    }
    let selected = state.selected.min(state.rows.len().saturating_sub(1));
    // Walk backwards to find the nearest depth-0 row.
    for idx in (0..=selected).rev() {
        let row = &state.rows[idx];
        if row.depth == 0 {
            return Some(SelectedRoot { row, row_idx: idx });
        }
    }
    // No root found — use the first row.
    state
        .rows
        .first()
        .map(|row| SelectedRoot { row, row_idx: 0 })
}

pub(super) fn collect_replies(state: &AppState, root_idx: usize) -> Vec<(usize, &TimelineRow)> {
    let start = root_idx.saturating_add(1);
    if start >= state.rows.len() {
        return Vec::new();
    }
    state.rows[start..]
        .iter()
        .enumerate()
        .take_while(|(_, r)| r.depth > 0)
        .map(|(i, r)| (start + i, r))
        .collect()
}

fn append_main_post(
    lines: &mut Vec<Line<'static>>,
    row: &TimelineRow,
    selected: bool,
    pane_width: usize,
) {
    let bg = if selected { SELECTED_BG } else { DETAIL_BG };
    let prefix = if selected {
        Span::styled("\u{25b6} ", Style::default().fg(ACCENT_CYAN).bg(bg))
    } else {
        Span::styled("  ", Style::default().bg(bg))
    };
    let prefix_width = 2usize;
    let content_width = pane_width.saturating_sub(prefix_width);

    // Header
    let author_style = Style::default()
        .fg(author_color(&row.author_pubkey))
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let (author_span, author_len) = profile_name_span(
        &row.author_profile,
        author_style,
        content_width.saturating_sub(8),
    );
    let sep = Span::styled(" \u{00b7} ", Style::default().fg(DIM_TEXT).bg(bg));
    let age = format_age(row.created_at);
    let used = author_len + 3 + age.chars().count();
    let pad = pad_for(content_width, used);
    lines.push(Line::from(vec![
        prefix.clone(),
        author_span,
        sep,
        Span::styled(age, Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled(pad, Style::default().bg(bg)),
    ]));

    append_content_lines(lines, row, prefix.clone(), bg, content_width);

    // Reaction bar
    let (spans, used) = reaction_spans(&row.relation_counts, bg);
    let pad = pad_for(content_width, used);
    let mut bar = vec![prefix];
    bar.extend(spans);
    bar.push(Span::styled(pad, Style::default().bg(bg)));
    lines.push(Line::from(bar));
}

fn append_reply(
    lines: &mut Vec<Line<'static>>,
    row: &TimelineRow,
    selected: bool,
    pane_width: usize,
) {
    let bg = if selected { SELECTED_BG } else { DETAIL_BG };
    let indent = row.depth.min(4).saturating_sub(1) * 2;
    let prefix_text = format!("{}\u{2502} ", " ".repeat(indent));
    let prefix_width = prefix_text.chars().count();
    let prefix = Span::styled(prefix_text, Style::default().fg(DIM_TEXT).bg(bg));
    let content_width = pane_width.saturating_sub(prefix_width);

    // Author line
    let author_style = Style::default()
        .fg(author_color(&row.author_pubkey))
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let (author_span, author_len) = profile_name_span(
        &row.author_profile,
        author_style,
        content_width.saturating_sub(8),
    );
    let sep = Span::styled(" \u{00b7} ", Style::default().fg(DIM_TEXT).bg(bg));
    let age = format_age(row.created_at);
    let used = author_len + 3 + age.chars().count();
    let pad = pad_for(content_width, used);
    lines.push(Line::from(vec![
        prefix.clone(),
        author_span,
        sep,
        Span::styled(age, Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled(pad, Style::default().bg(bg)),
    ]));

    append_content_lines(lines, row, prefix, bg, content_width);
}

pub(super) fn reaction_spans(
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

fn append_content_lines(
    lines: &mut Vec<Line<'static>>,
    row: &TimelineRow,
    prefix: Span<'static>,
    bg: ratatui::style::Color,
    width: usize,
) {
    let rendered = row
        .content_tree
        .as_ref()
        .map(|tree| {
            NostrContentView::new(tree)
                .render_data(Some(&row.content_render))
                .lines(width)
        })
        .unwrap_or_else(|| {
            wrap_body(&row.content, width)
                .into_iter()
                .map(|body| Line::from(Span::styled(body, Style::default().fg(BODY_TEXT))))
                .collect()
        });
    for line in rendered {
        lines.push(prefix_line(line, prefix.clone(), bg, width));
    }
}

pub(super) fn prefix_line(
    line: Line<'static>,
    prefix: Span<'static>,
    bg: ratatui::style::Color,
    width: usize,
) -> Line<'static> {
    let mut used = 0usize;
    let mut spans = vec![prefix];
    for span in line.spans {
        used += span.content.chars().count();
        spans.push(Span::styled(span.content.to_string(), span.style.bg(bg)));
    }
    spans.push(Span::styled(pad_for(width, used), Style::default().bg(bg)));
    Line::from(spans)
}

pub(super) fn wrap_body(content: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let single = content.replace('\n', " ");
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut buf_len = 0usize;
    for word in single.split_whitespace() {
        let wlen = word.chars().count();
        if buf_len == 0 {
            if wlen > width {
                // hard split long word
                let mut remaining = word.to_string();
                while remaining.chars().count() > width {
                    let chunk: String = remaining.chars().take(width).collect();
                    out.push(chunk);
                    remaining = remaining.chars().skip(width).collect();
                }
                buf.push_str(&remaining);
                buf_len = remaining.chars().count();
            } else {
                buf.push_str(word);
                buf_len = wlen;
            }
        } else if buf_len + 1 + wlen <= width {
            buf.push(' ');
            buf.push_str(word);
            buf_len += 1 + wlen;
        } else {
            out.push(std::mem::take(&mut buf));
            if wlen > width {
                let mut remaining = word.to_string();
                while remaining.chars().count() > width {
                    let chunk: String = remaining.chars().take(width).collect();
                    out.push(chunk);
                    remaining = remaining.chars().skip(width).collect();
                }
                buf.push_str(&remaining);
                buf_len = remaining.chars().count();
            } else {
                buf.push_str(word);
                buf_len = wlen;
            }
        }
    }
    if !buf.is_empty() || out.is_empty() {
        out.push(buf);
    }
    out
}

pub(super) fn pad_for(width: usize, used: usize) -> String {
    if width > used {
        " ".repeat(width - used)
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn detail_lines_use_opened_thread_feed_not_home_selection() {
        let mut state = AppState {
            focused: Pane::Detail,
            rows: vec![row("home-row", &"aa".repeat(32), "home selected body")],
            thread_event_id: "cc".repeat(32),
            thread_rows: vec![row("thread-row", &"bb".repeat(32), "thread feed body")],
            ..Default::default()
        };

        let text = lines_text(&build_lines(&state, 80));

        assert!(text.contains("thread feed body"));
        assert!(!text.contains("home selected body"));
        state.thread_rows.clear();
        let empty_text = lines_text(&build_lines(&state, 80));
        assert!(empty_text.contains("Loading opened thread feed"));
    }
}
