use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// Display-only Ratatui group-chat composer.
///
/// The host TUI owns input events and publish actions. This component renders
/// draft state and exposes the same trim/send decision a host can call before
/// routing into a Rust-owned action.
pub struct NostrGroupComposer<'a> {
    draft: &'a str,
    placeholder: &'a str,
    is_enabled: bool,
    is_focused: bool,
}

impl<'a> NostrGroupComposer<'a> {
    pub fn new(draft: &'a str) -> Self {
        Self {
            draft,
            placeholder: "Message",
            is_enabled: true,
            is_focused: false,
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.is_enabled = enabled;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.is_focused = focused;
        self
    }

    pub fn trimmed_message(&self) -> Option<&str> {
        let trimmed = self.draft.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    pub fn can_send(&self) -> bool {
        self.is_enabled && self.trimmed_message().is_some()
    }

    fn line(&self) -> Line<'static> {
        let style = if self.is_enabled {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let text = if self.draft.is_empty() {
            Span::styled(
                self.placeholder.to_string(),
                Style::default().fg(Color::DarkGray),
            )
        } else {
            Span::styled(self.draft.to_string(), style)
        };
        let send = if self.can_send() { " send " } else { "      " };
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            text,
            Span::raw(" "),
            Span::styled(
                send,
                Style::default()
                    .fg(Color::Black)
                    .bg(if self.can_send() {
                        Color::Cyan
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    }
}

impl Widget for NostrGroupComposer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default().borders(Borders::ALL).border_style(border);
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.line()).render(inner, buf);
    }
}
