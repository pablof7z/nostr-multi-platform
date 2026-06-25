use std::collections::VecDeque;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::cache::CachedEvent;
use crate::relay::{id_prefix, Config, NegReport, PlainReport, PublishReport, RunReport};

#[derive(Default)]
pub struct AppState {
    pub running: bool,
    pub status: String,
    pub plain: Option<PlainReport>,
    pub neg: Option<NegReport>,
    pub neg_error: Option<String>,
    pub publish: Option<PublishReport>,
    pub surface: String,
    pub newest: Vec<CachedEvent>,
    pub logs: VecDeque<String>,
}

impl AppState {
    pub fn log(&mut self, line: impl Into<String>) {
        if self.logs.len() >= 200 {
            self.logs.pop_front();
        }
        self.logs.push_back(line.into());
    }

    pub fn apply_run(&mut self, report: RunReport) {
        self.running = false;
        self.status = "idle".to_string();
        self.surface = report.surface;
        self.plain = report.plain;
        self.neg = report.neg;
        self.neg_error = report.neg_error;
        self.newest = report.newest;
        self.log(format!("cache: {}", report.cache_path.display()));
        if let Some(error) = &self.neg_error {
            self.log(format!("NIP-77 error: {error}"));
        }
    }
}

pub fn render(frame: &mut Frame<'_>, state: &AppState, config: &Config) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
        .split(frame.area());

    render_header(frame, root[0], state, config);
    render_metrics(frame, root[1], state);
    render_events(frame, root[2], state);
    render_logs(frame, root[3], state);
}

fn render_header(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    state: &AppState,
    config: &Config,
) {
    let running = if state.running {
        "running"
    } else {
        &state.status
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Relay ", Style::default().fg(Color::Gray)),
            Span::raw(&config.relay),
        ]),
        Line::from(vec![
            Span::styled("Filter ", Style::default().fg(Color::Gray)),
            Span::raw(&config.filter_json),
        ]),
        Line::from(vec![
            Span::styled("Surface ", Style::default().fg(Color::Gray)),
            Span::raw(if state.surface.is_empty() {
                "not measured yet"
            } else {
                &state.surface
            }),
        ]),
        Line::from(vec![
            Span::styled("Cache ", Style::default().fg(Color::Gray)),
            Span::raw(config.cache_path.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Group ", Style::default().fg(Color::Gray)),
            Span::raw(config.group.as_deref().unwrap_or("none")),
        ]),
        Line::from(vec![
            Span::styled("Keys ", Style::default().fg(Color::Gray)),
            Span::raw("r run plain+NIP-77 | n NIP-77 only | p publish demo event | c clear cache | q quit"),
        ]),
        Line::from(vec![
            Span::styled("State ", Style::default().fg(Color::Gray)),
            Span::styled(running, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("NIP-77 real relay diagnostic"),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_metrics(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let plain = state.plain.as_ref().map_or_else(
        || vec![Line::from("No plain REQ run yet.")],
        |p| {
            vec![
                Line::from(format!("events returned: {}", p.events)),
                Line::from(format!("auths sent: {}", p.auths_sent)),
                Line::from(format!(
                    "wire bytes: sent {} / received {}",
                    p.bytes_sent, p.bytes_received
                )),
                Line::from(format!("elapsed: {} ms", p.elapsed_ms)),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(plain).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Plain REQ baseline"),
        ),
        cols[0],
    );
    let neg = state.neg.as_ref().map_or_else(
        || {
            if let Some(error) = &state.neg_error {
                vec![
                    Line::from("No NIP-77 delta sync completed."),
                    Line::from(format!("error: {error}")),
                ]
            } else {
                vec![Line::from("No NIP-77 run yet.")]
            }
        },
        |n| {
            vec![
                Line::from(format!("cache: {} -> {}", n.local_before, n.local_after)),
                Line::from(format!(
                    "relay-only ids synced: {} (fetched {})",
                    n.need, n.fetched
                )),
                Line::from(format!("local-only ids: {} | rounds: {}", n.have, n.rounds)),
                Line::from(format!("auths sent: {}", n.auths_sent)),
                Line::from(format!(
                    "wire bytes: sent {} / received {}",
                    n.bytes_sent, n.bytes_received
                )),
                Line::from(format!("elapsed: {} ms", n.elapsed_ms)),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(neg).block(
            Block::default()
                .borders(Borders::ALL)
                .title("NIP-77 delta sync"),
        ),
        cols[1],
    );
}

fn render_events(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &AppState) {
    let items: Vec<_> = state
        .newest
        .iter()
        .map(|event| {
            ListItem::new(Line::from(format!(
                "{} kind:{} at:{} {}",
                id_prefix(&event.id),
                event.kind,
                event.created_at,
                event.content.replace('\n', " ")
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Newest cached matching events"),
        ),
        area,
    );
}

fn render_logs(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &AppState) {
    let mut lines: Vec<_> = state.logs.iter().rev().take(5).cloned().collect();
    lines.reverse();
    if let Some(publish) = &state.publish {
        lines.push(format!(
            "publish {} signer {} {}: {}",
            id_prefix(&publish.id),
            id_prefix(&publish.pubkey),
            if publish.accepted {
                "accepted"
            } else {
                "rejected"
            },
            publish.relay_message
        ));
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .block(Block::default().borders(Borders::ALL).title("Log"))
            .wrap(Wrap { trim: false }),
        area,
    );
}
