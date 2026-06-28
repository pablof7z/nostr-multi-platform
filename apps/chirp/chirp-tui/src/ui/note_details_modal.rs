use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT};

pub fn render(f: &mut Frame, area: Rect, state: &AppState, scroll: u16) {
    let width = (area.width * 80 / 100)
        .max(60)
        .min(area.width.saturating_sub(2));
    let height = (area.height * 85 / 100)
        .max(10)
        .min(area.height.saturating_sub(2));
    let popup = centered(area, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Note Details - j/k scroll  Esc close ")
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let text = if state.note_details_content.is_empty() {
        "no note details".to_string()
    } else {
        state.note_details_content.clone()
    };

    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(BODY_TEXT).bg(DETAIL_BG))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default().style(Style::default().fg(DIM_TEXT).add_modifier(Modifier::DIM)),
            ),
        inner,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
