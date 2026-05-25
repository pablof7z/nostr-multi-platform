//! Full-screen onboarding overlay.
//!
//! Rendered when no account is configured. `render` selects the correct phase
//! from the helper `onboarding_phase` which derives phase from existing
//! `AppState` fields today; once the wiring agent adds `onboarding_phase: u8`
//! this file gets a one-line update inside the helper.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DIM_TEXT, RELAY_OK, SELECTED_BG};

/// Derive onboarding phase from state.
/// Phase 0 = welcome, 1-3 = account input in progress, 4 = relay picker, 5 = done.
/// Today (pre-wiring): if accounts is empty we show phase 0, else done (5).
fn onboarding_phase(state: &AppState) -> u8 {
    if state.features.accounts.is_empty() {
        0
    } else {
        5
    }
}

/// Preset relay list used when state.relays is empty during phase 4.
const PRESET_RELAYS: [&str; 5] = [
    "wss://relay.damus.io",
    "wss://relay.nostr.band",
    "wss://nos.lol",
    "wss://relay.snort.social",
    "wss://purplepag.es",
];

/// Render the onboarding overlay for the given `area`.
/// No-ops when `onboarding_phase` returns 5 (done).
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let phase = onboarding_phase(state);
    if phase >= 5 {
        return;
    }
    f.render_widget(Clear, area);
    match phase {
        4 => render_relay_picker(f, area, state),
        _ => render_welcome(f, area),
    }
}

// ---------------------------------------------------------------------------
// Phase 0 — welcome
// ---------------------------------------------------------------------------

fn render_welcome(f: &mut Frame, area: Rect) {
    // Vertical layout: push title+subtitle to upper-third, then the menu box.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Length(3),  // title + subtitle
            Constraint::Length(1),  // spacer
            Constraint::Length(14), // menu box (8 options × ~1.5 rows + borders)
            Constraint::Length(1),  // spacer
            Constraint::Length(1),  // footer hint
            Constraint::Min(0),
        ])
        .split(area);

    // Title
    let title = Paragraph::new(Line::from(Span::styled(
        "chirp",
        Style::default()
            .fg(ACCENT_CYAN)
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);
    f.render_widget(title, sections[1]);

    // Subtitle
    let subtitle = Paragraph::new(Line::from(Span::styled(
        "the nostr social client",
        Style::default().fg(DIM_TEXT),
    )))
    .alignment(Alignment::Center);
    f.render_widget(subtitle, shrink_y(sections[1], 1));

    // Menu box — centered horizontally
    let box_width = 41u16;
    let menu_area = horizontal_center(sections[3], box_width);
    let items: Vec<ListItem> = vec![
        menu_item("[ 1 ]", "Create a new account", "generates a fresh keypair"),
        menu_item("[ 2 ]", "Import an existing nsec", "paste your secret key"),
        menu_item("[ 3 ]", "Connect a NIP-46 bunker", "sign with a remote signer"),
        menu_item("[ 4 ]", "Browse without an account", "read-only mode"),
    ];
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(DIM_TEXT)),
    );
    f.render_widget(list, menu_area);

    // Footer hint
    let hint = Paragraph::new(Line::from(Span::styled(
        "Press 1-4  \u{00b7}  ? for help  \u{00b7}  q to quit",
        Style::default().fg(DIM_TEXT),
    )))
    .alignment(Alignment::Center);
    f.render_widget(hint, sections[5]);
}

fn menu_item(key: &'static str, label: &'static str, hint: &'static str) -> ListItem<'static> {
    ListItem::new(vec![
        Line::from(vec![
            Span::styled(
                format!("  {key}  "),
                Style::default()
                    .fg(ACCENT_CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(label, Style::default().fg(BODY_TEXT)),
        ]),
        Line::from(Span::styled(
            format!("         {hint}"),
            Style::default().fg(DIM_TEXT),
        )),
        Line::from(""),
    ])
}

// ---------------------------------------------------------------------------
// Phase 4 — relay picker
// ---------------------------------------------------------------------------

fn render_relay_picker(f: &mut Frame, area: Rect, state: &AppState) {
    let relay_urls: Vec<String> = if state.relays.is_empty() {
        PRESET_RELAYS.iter().map(|s| s.to_string()).collect()
    } else {
        state
            .relays
            .iter()
            .take(5)
            .map(|r| r.short_url.clone())
            .collect()
    };

    let box_height = (relay_urls.len() as u16) * 2 + 6;
    let box_width = 50u16;
    let popup = centered(area, box_width, box_height);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = relay_urls
        .iter()
        .enumerate()
        .map(|(i, url)| {
            // Toggle state via detail_cursor as a stand-in until wiring adds
            // proper relay selection state; just mark first two as selected.
            let checked = i < 2;
            let (checkbox, fg) = if checked {
                ("[x]", RELAY_OK)
            } else {
                ("[ ]", DIM_TEXT)
            };
            let style = if i == state.detail_cursor % relay_urls.len() {
                Style::default().bg(SELECTED_BG).fg(fg)
            } else {
                Style::default().fg(fg)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {checkbox} "), style),
                Span::styled(url.clone(), style),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Choose relays ")
            .style(Style::default().fg(ACCENT_CYAN)),
    );
    f.render_widget(list, popup);

    // Footer hint
    let hint_area = Rect {
        y: popup.y + popup.height,
        height: 1,
        ..popup
    };
    if hint_area.y < area.y + area.height {
        let hint = Paragraph::new(Line::from(Span::styled(
            "Space toggle  \u{00b7}  Enter confirm",
            Style::default().fg(DIM_TEXT),
        )))
        .alignment(Alignment::Center);
        f.render_widget(hint, hint_area);
    }
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(4));
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(h) / 2),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(area);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(w) / 2),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(vert[1]);
    horiz[1]
}

fn horizontal_center(area: Rect, width: u16) -> Rect {
    let w = width.min(area.width);
    let x = area.x + area.width.saturating_sub(w) / 2;
    Rect {
        x,
        width: w,
        ..area
    }
}

/// Shift a rect down by `n` rows (used to render subtitle below title).
fn shrink_y(area: Rect, n: u16) -> Rect {
    Rect {
        y: area.y + n,
        height: area.height.saturating_sub(n),
        ..area
    }
}
