use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::AppState;
use crate::feature_snapshot::MessageLine;
use crate::features::FeatureTab;
use crate::ui::shared_snapshot_lines::{action_summary, relay_lines};

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    match state.tab {
        FeatureTab::Chats => render_chats(frame, area, state),
        FeatureTab::Groups => render_groups(frame, area, state),
        FeatureTab::Wallet => render_wallet(frame, area, state),
        FeatureTab::Settings => render_settings(frame, area, state),
        FeatureTab::Home => {}
    }
}

fn render_chats(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let panes = two_columns(area);
    let conv = state
        .features
        .dm_conversations
        .iter()
        .map(|c| Line::from(format!("{}  {}", c.peer_display, c.latest)))
        .chain(empty_hint(
            state.features.dm_conversations.is_empty(),
            "No NIP-17 conversations yet.",
        ))
        .collect::<Vec<_>>();
    frame.render_widget(panel("Chats", conv), panes[0]);

    let messages = state
        .features
        .dm_conversations
        .first()
        .map_or_else(
            || vec![Line::from(":dm <pubkey> <message> sends through nmp.nip17.send")],
            |c| message_lines(&c.messages),
        );
    frame.render_widget(panel("Conversation", messages), panes[1]);
}

fn render_groups(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let panes = if area.width < 110 {
        two_columns(area)
    } else {
        three_columns(area)
    };
    let discovered = state
        .features
        .discovered_groups
        .iter()
        .map(|g| {
            Line::from(format!(
                "{}  {} members  {}",
                g.name,
                g.member_count,
                if g.open { "open" } else { "closed" }
            ))
        })
        .chain(empty_hint(
            state.features.discovered_groups.is_empty(),
            ":group discover <relay> lists NIP-29 groups.",
        ))
        .collect::<Vec<_>>();
    frame.render_widget(panel("Discover", discovered), panes[0]);

    let messages = message_lines(&state.features.group_messages);
    frame.render_widget(panel("NIP-29 Chat", messages), panes[1]);

    if panes.len() > 2 {
        frame.render_widget(
            panel(
                "Marmot MLS",
                vec![
                    Line::from(":mls init registers active identity"),
                    Line::from(":mls snapshot shows encrypted group state"),
                    Line::from(":mls dispatch <json> routes Marmot actions"),
                ],
            ),
            panes[2],
        );
    }
}

fn render_wallet(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let wallet = &state.features.wallet;
    let balance = wallet
        .balance_msats.map_or_else(|| "balance unavailable".to_string(), |m| format!("{} sats", m / 1000));
    frame.render_widget(
        panel(
            "Wallet",
            vec![
                Line::from(format!(
                    "status: {}",
                    fallback(&wallet.status, "disconnected")
                )),
                Line::from(format!("relay: {}", fallback(&wallet.relay_url, "-"))),
                Line::from(format!("wallet: {}", fallback(&wallet.wallet_npub, "-"))),
                Line::from(format!("balance: {balance}")),
                Line::from(""),
                Line::from(":wallet connect <nostr+walletconnect-uri>"),
                Line::from(":wallet pay <bolt11> [amount_msats]"),
                Line::from(":wallet disconnect"),
            ],
        ),
        area,
    );
}

fn render_settings(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let panes = three_columns(area);
    let accounts = state
        .features
        .accounts
        .iter()
        .map(|a| {
            Line::from(format!(
                "{}{}  {}",
                if a.active { "*" } else { " " },
                a.display,
                a.signer
            ))
        })
        .chain(empty_hint(
            state.features.accounts.is_empty(),
            ":account create/import/bunker signs in.",
        ))
        .collect::<Vec<_>>();
    frame.render_widget(panel("Accounts", accounts), panes[0]);

    let mut relays = state
        .features
        .relay_edit_rows
        .iter()
        .map(|r| Line::from(format!("{}  {}", r.url, r.role_label)))
        .collect::<Vec<_>>();
    if relays.is_empty() {
        relays = relay_lines(state);
    }
    frame.render_widget(panel("Relays", relays), panes[1]);

    let mut ops = vec![
        Line::from(action_summary(state)),
        Line::from(format!("outbox: {}", state.features.outbox_summary.title)),
        Line::from(format!("follows: {}", state.features.follow_count)),
    ];
    ops.extend(
        state
            .features
            .outbox
            .iter()
            .map(|o| Line::from(format!("{}  {}  {}", o.handle, o.status_label, o.title))),
    );
    frame.render_widget(panel("Outbox & Diagnostics", ops), panes[2]);
}

fn message_lines(messages: &[MessageLine]) -> Vec<Line<'static>> {
    if messages.is_empty() {
        return vec![Line::from("No messages in this projection yet.")];
    }
    messages
        .iter()
        .take(12)
        .map(|m| {
            let side = if m.outgoing {
                "me".to_string()
            } else {
                short(&m.author)
            };
            Line::from(format!("{side}: {}", m.content.replace('\n', " ")))
        })
        .collect()
}

fn two_columns(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area)
        .to_vec()
}

fn three_columns(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(35),
            Constraint::Percentage(35),
        ])
        .split(area)
        .to_vec()
}

fn panel(title: &'static str, lines: Vec<Line<'static>>) -> Paragraph<'static> {
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true })
}

fn empty_hint(show: bool, text: &'static str) -> impl Iterator<Item = Line<'static>> {
    show.then_some(Line::from(text)).into_iter()
}

fn fallback<'a>(value: &'a str, empty: &'a str) -> &'a str {
    if value.is_empty() {
        empty
    } else {
        value
    }
}

fn short(value: &str) -> String {
    if value.len() <= 14 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 4..])
    }
}
