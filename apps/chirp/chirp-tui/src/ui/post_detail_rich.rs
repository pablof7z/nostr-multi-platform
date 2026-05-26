use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::{
    app::{AppState, Pane},
    timeline::TimelineRow,
    ui::{
        colors::{
            author_color, format_age, ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, SELECTED_BG,
        },
        layout::RenderContext,
        nostr_content::nostr_content_view::NostrContentView,
        post_detail::{
            collect_replies, pad_for, prefix_line, reaction_spans, root_for_selection, wrap_body,
        },
    },
};

pub fn render_rich(f: &mut Frame, area: Rect, state: &AppState, context: &RenderContext<'_>) {
    let Some(root) = root_for_selection(state) else {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Select a note to view the thread.",
                Style::default().fg(DIM_TEXT),
            )),
        ])
        .style(Style::default().bg(DETAIL_BG));
        f.render_widget(empty, area);
        return;
    };

    let mut painter = DetailPainter::new(f, area, state.detail_scroll);
    let focused = state.focused == Pane::Detail;
    append_main_post(
        &mut painter,
        root.row,
        focused && state.detail_cursor == 0,
        context,
    );

    let replies = collect_replies(state, root.row_idx);
    if !replies.is_empty() {
        painter.line(Line::from(""));
        painter.line(Line::from(Span::styled(
            "\u{2500}\u{2500}\u{2500} Replies \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            Style::default().fg(DIM_TEXT),
        )));
        painter.line(Line::from(""));
    }

    for (reply_index, (_, reply)) in replies.iter().enumerate() {
        append_reply(
            &mut painter,
            reply,
            focused && state.detail_cursor == reply_index + 1,
            context,
        );
    }
}

struct DetailPainter<'a, 'b> {
    frame: &'a mut Frame<'b>,
    area: Rect,
    cursor: u16,
    scroll: u16,
}

impl<'a, 'b> DetailPainter<'a, 'b> {
    fn new(frame: &'a mut Frame<'b>, area: Rect, scroll: u16) -> Self {
        Self {
            frame,
            area,
            cursor: 0,
            scroll,
        }
    }

    fn line(&mut self, line: Line<'static>) {
        if let Some(rect) = self.take(1) {
            self.frame.render_widget(
                Paragraph::new(line).style(Style::default().bg(DETAIL_BG)),
                rect,
            );
        }
    }

    fn content(
        &mut self,
        row: &TimelineRow,
        prefix_width: u16,
        bg: ratatui::style::Color,
        context: &RenderContext<'_>,
    ) {
        let width = self.area.width.saturating_sub(prefix_width);
        if width == 0 {
            return;
        }
        let Some(tree) = row.content_tree.as_ref() else {
            self.text_content(row, prefix_width, bg, width as usize);
            return;
        };
        let view = NostrContentView::new(tree)
            .render_data(Some(&row.content_render))
            .media_images(context.media_images);
        let Some(rect) = self.take(view.preferred_height(width as usize)) else {
            return;
        };
        if prefix_width > 0 {
            let prefix = Rect {
                width: prefix_width,
                ..rect
            };
            self.frame
                .render_widget(Paragraph::new("").style(Style::default().bg(bg)), prefix);
        }
        self.frame.render_widget(
            view,
            Rect {
                x: rect.x.saturating_add(prefix_width),
                width,
                ..rect
            },
        );
    }

    fn text_content(
        &mut self,
        row: &TimelineRow,
        prefix_width: u16,
        bg: ratatui::style::Color,
        width: usize,
    ) {
        let prefix = Span::styled(" ".repeat(prefix_width as usize), Style::default().bg(bg));
        for body in wrap_body(&row.content, width) {
            let line = Line::from(Span::styled(body, Style::default().fg(BODY_TEXT)));
            self.line(prefix_line(line, prefix.clone(), bg, width));
        }
    }

    fn take(&mut self, height: u16) -> Option<Rect> {
        if height == 0 || self.area.is_empty() {
            return None;
        }
        let start = self.cursor;
        let end = start.saturating_add(height);
        self.cursor = end;
        if end <= self.scroll {
            return None;
        }
        let visible_start = start.saturating_sub(self.scroll);
        if visible_start >= self.area.height {
            return None;
        }
        let bottom = end.saturating_sub(self.scroll).min(self.area.height);
        Some(Rect {
            x: self.area.x,
            y: self.area.y.saturating_add(visible_start),
            width: self.area.width,
            height: bottom - visible_start,
        })
    }
}

fn append_main_post(
    painter: &mut DetailPainter<'_, '_>,
    row: &TimelineRow,
    selected: bool,
    context: &RenderContext<'_>,
) {
    let bg = if selected { SELECTED_BG } else { DETAIL_BG };
    let prefix = if selected {
        Span::styled("\u{25b6} ", Style::default().fg(ACCENT_CYAN).bg(bg))
    } else {
        Span::styled("  ", Style::default().bg(bg))
    };
    let content_width = painter.area.width.saturating_sub(2) as usize;
    painter.line(header_line(row, prefix.clone(), bg, content_width));
    painter.content(row, 2, bg, context);
    painter.line(reaction_line(row, prefix, bg, content_width));
}

fn append_reply(
    painter: &mut DetailPainter<'_, '_>,
    row: &TimelineRow,
    selected: bool,
    context: &RenderContext<'_>,
) {
    let bg = if selected { SELECTED_BG } else { DETAIL_BG };
    let indent = row.depth.min(4).saturating_sub(1) * 2;
    let prefix_text = format!("{}\u{2502} ", " ".repeat(indent));
    let prefix_width = prefix_text.chars().count() as u16;
    let prefix = Span::styled(prefix_text, Style::default().fg(DIM_TEXT).bg(bg));
    let content_width = painter.area.width.saturating_sub(prefix_width) as usize;
    painter.line(header_line(row, prefix.clone(), bg, content_width));
    painter.content(row, prefix_width, bg, context);
}

fn header_line(
    row: &TimelineRow,
    prefix: Span<'static>,
    bg: ratatui::style::Color,
    content_width: usize,
) -> Line<'static> {
    let author_label = row.author_label().to_string();
    let author = Span::styled(
        author_label.clone(),
        Style::default()
            .fg(author_color(&row.author_pubkey))
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    );
    let age = format_age(row.created_at);
    let used = author_label.chars().count() + 3 + age.chars().count();
    Line::from(vec![
        prefix,
        author,
        Span::styled(" \u{00b7} ", Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled(age, Style::default().fg(DIM_TEXT).bg(bg)),
        Span::styled(pad_for(content_width, used), Style::default().bg(bg)),
    ])
}

fn reaction_line(
    row: &TimelineRow,
    prefix: Span<'static>,
    bg: ratatui::style::Color,
    content_width: usize,
) -> Line<'static> {
    let (spans, used) = reaction_spans(&row.relation_counts, bg);
    let mut bar = vec![prefix];
    bar.extend(spans);
    bar.push(Span::styled(
        pad_for(content_width, used),
        Style::default().bg(bg),
    ));
    Line::from(bar)
}
