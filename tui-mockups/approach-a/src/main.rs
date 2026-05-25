//! Chirp TUI mockup — Approach A: "Wide Feed + Right Panel".
//!
//! Standalone visual demo. All data is hardcoded; nothing touches the network
//! or any `nmp-*` crate. The goal is to evaluate the layout, the color story,
//! and the keyboard ergonomics — not to ship a real client.
//!
//! ## Layout
//! - Header: app name + tab strip (Home / Notifications / DMs / Search)
//! - Body (split 70/30):
//!     - Left: feed of posts, or inline thread, or DM chat (depending on
//!       active tab and selected post)
//!     - Right: vertically split — top half is the relay list, bottom half
//!       is a preview of recent DMs
//! - Footer: keybinding hint line
//!
//! ## Keybindings
//! - `j` / `Down`           — scroll down
//! - `k` / `Up`             — scroll up
//! - `Enter`                — expand thread / open DM conversation
//! - `Esc` / `Backspace`    — collapse thread / leave conversation
//! - `1`..=`4`              — jump to tab
//! - `h` / `l`              — previous / next tab
//! - `d`                    — jump to DMs tab
//! - `r`                    — reply (no-op stub)
//! - `/`                    — search (no-op stub)
//! - `q` / `Ctrl-C`         — quit

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

// ---------------------------------------------------------------------------
// Data model — all fake / hardcoded.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone)]
struct Post {
    author: &'static str,
    npub: &'static str,
    ago: &'static str,
    body: &'static str,
    hearts: u32,
    sats: u32,
    reposts: u32,
    comments: u32,
    /// Inline thread replies (single-level nesting is enough for the mock).
    replies: Vec<Reply>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct Reply {
    author: &'static str,
    npub: &'static str,
    ago: &'static str,
    body: &'static str,
    /// One level deeper. Anything beyond two levels renders as the same indent
    /// as level 2 — the mock only needs to demonstrate the indent rhythm.
    nested: Vec<Reply>,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum RelayStatus {
    Connected,
    Disconnected,
    Connecting,
}

#[allow(dead_code)]
#[derive(Clone)]
struct Relay {
    url: &'static str,
    status: RelayStatus,
}

#[allow(dead_code)]
#[derive(Clone)]
struct DmMessage {
    /// `None` means "you" (the local user).
    from: Option<&'static str>,
    body: &'static str,
}

#[allow(dead_code)]
#[derive(Clone)]
struct DmThread {
    peer: &'static str,
    preview: &'static str,
    messages: Vec<DmMessage>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Notifications,
    Dms,
    Search,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Notifications => "Notifications",
            Tab::Dms => "DMs",
            Tab::Search => "Search",
        }
    }

    fn all() -> [Tab; 4] {
        [Tab::Home, Tab::Notifications, Tab::Dms, Tab::Search]
    }

    fn index(self) -> usize {
        match self {
            Tab::Home => 0,
            Tab::Notifications => 1,
            Tab::Dms => 2,
            Tab::Search => 3,
        }
    }

    fn from_index(i: usize) -> Tab {
        Self::all()[i.min(3)]
    }
}

/// What the left (wide) pane is currently showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FeedView {
    /// The scrolling list of posts (Home tab).
    Feed,
    /// A single post + its replies, expanded inline.
    Thread,
    /// DM conversation view (DMs tab with a thread selected).
    DmChat,
}

struct App {
    tab: Tab,
    feed_view: FeedView,
    selected_post: usize,
    selected_dm: usize,
    posts: Vec<Post>,
    relays: Vec<Relay>,
    dms: Vec<DmThread>,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            tab: Tab::Home,
            feed_view: FeedView::Feed,
            selected_post: 0,
            selected_dm: 0,
            posts: seed_posts(),
            relays: seed_relays(),
            dms: seed_dms(),
            should_quit: false,
        }
    }

    fn move_selection(&mut self, delta: i32) {
        // Order matters: DMs-tab-with-list-view must be checked *before* the
        // generic "Feed view scrolls the post list" arm, otherwise the wide
        // catch-all would swallow it.
        match (self.tab, self.feed_view) {
            // DM list scrolling owns its own cursor.
            (Tab::Dms, FeedView::Feed) => {
                let n = self.dms.len() as i32;
                if n == 0 {
                    return;
                }
                let next = (self.selected_dm as i32 + delta).rem_euclid(n);
                self.selected_dm = next as usize;
            }
            // Any other tab in the list view scrolls the feed.
            (_, FeedView::Feed) => {
                let n = self.posts.len() as i32;
                if n == 0 {
                    return;
                }
                let next = (self.selected_post as i32 + delta).rem_euclid(n);
                self.selected_post = next as usize;
            }
            // Thread / chat — j/k does nothing meaningful in this mock, but we
            // keep the binding alive so the user isn't confused by a dead key.
            (_, FeedView::Thread) | (_, FeedView::DmChat) => {}
        }
    }

    fn activate(&mut self) {
        match self.tab {
            Tab::Home => self.feed_view = FeedView::Thread,
            Tab::Dms => self.feed_view = FeedView::DmChat,
            _ => {}
        }
    }

    fn back(&mut self) {
        if matches!(self.feed_view, FeedView::Thread | FeedView::DmChat) {
            self.feed_view = FeedView::Feed;
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        // Switching tabs always returns to the list view of that tab.
        self.feed_view = FeedView::Feed;
        // The DM list owns its own selection so it survives tab switches.
        // The feed selection clamps so we never index out of range.
        if self.selected_post >= self.posts.len() {
            self.selected_post = 0;
        }
        if self.selected_dm >= self.dms.len() {
            self.selected_dm = 0;
        }
    }

    fn cycle_tab(&mut self, delta: i32) {
        let i = self.tab.index() as i32;
        let n = Tab::all().len() as i32;
        let next = (i + delta).rem_euclid(n) as usize;
        self.set_tab(Tab::from_index(next));
    }
}

// ---------------------------------------------------------------------------
// Color helpers.
// ---------------------------------------------------------------------------

/// Reaction-bar accent colors. These use truecolor RGB so the values are
/// stable across terminal themes (we want the heart to look the same red
/// on Solarized as on a default profile).
mod accent {
    use ratatui::style::Color;
    pub const HEART: Color = Color::Rgb(255, 85, 85);
    pub const ZAP: Color = Color::Rgb(255, 200, 60);
    pub const REPOST: Color = Color::Rgb(120, 220, 120);
    pub const COMMENT: Color = Color::Rgb(110, 220, 230);
    pub const DIM_BORDER: Color = Color::Rgb(80, 80, 92);
    pub const SELECTED_BORDER: Color = Color::Rgb(110, 220, 230);
    pub const FOOTER_BG: Color = Color::Rgb(24, 24, 32);
    pub const TIMESTAMP: Color = Color::Rgb(110, 110, 120);
    // Spec says "bold magenta" — use the literal magenta so the brand color
    // matches the design doc rather than a softer mauve.
    pub const APP_NAME: Color = Color::Magenta;
    pub const TAB_ACTIVE_BG: Color = Color::Rgb(40, 80, 180);
    pub const RELAY_CONNECTING: Color = Color::Rgb(230, 195, 80);
}

/// Stable per-author bright color. Hashing the &'static str pointer would be
/// non-deterministic across runs, so we round-robin by the first byte of the
/// author handle — good enough for the demo and trivially stable.
fn author_color(author: &str) -> Color {
    const PALETTE: [Color; 5] = [
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Blue,
    ];
    let key = author.as_bytes().iter().copied().next().unwrap_or(0) as usize;
    PALETTE[key % PALETTE.len()]
}

// ---------------------------------------------------------------------------
// Rendering.
// ---------------------------------------------------------------------------

fn ui(f: &mut Frame, app: &App) {
    let area = f.area();

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
        ])
        .split(area);

    draw_header(f, outer[0], app);
    draw_body(f, outer[1], app);
    draw_footer(f, outer[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    // Build the spans: app name, then each tab with active/inactive styling.
    let mut spans: Vec<Span> = Vec::with_capacity(2 + Tab::all().len() * 2);
    spans.push(Span::styled(
        " \u{25c6} chirp",
        Style::default().fg(accent::APP_NAME).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("   "));

    for (i, tab) in Tab::all().iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let label = format!(" {} ", tab.label());
        if *tab == app.tab {
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::White)
                    .bg(accent::TAB_ACTIVE_BG)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(label, Style::default().fg(Color::Gray)));
        }
    }

    let header = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(accent::DIM_BORDER)),
    );
    f.render_widget(header, area);
}

fn draw_body(f: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    // Left pane content depends on tab + feed_view.
    match (app.tab, app.feed_view) {
        (Tab::Home, FeedView::Feed) => draw_feed(f, cols[0], app),
        (Tab::Home, FeedView::Thread) => draw_thread(f, cols[0], app),
        (Tab::Dms, FeedView::Feed) => draw_dm_list_pane(f, cols[0], app),
        (Tab::Dms, FeedView::DmChat) => draw_dm_chat(f, cols[0], app),
        (Tab::Notifications, _) => draw_placeholder(f, cols[0], "Notifications", "no new notifications"),
        (Tab::Search, _) => draw_placeholder(f, cols[0], "Search", "type / to search — demo stub"),
        // Defensive fallback: any unexpected (tab, view) pair shows the feed.
        _ => draw_feed(f, cols[0], app),
    }

    // Right pane is the same in every tab.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[1]);
    draw_relays(f, right[0], app);
    draw_dm_previews(f, right[1], app);
}

fn draw_feed(f: &mut Frame, area: Rect, app: &App) {
    // Each post is rendered as its own bordered block. We stack them with a
    // vertical layout sized by post — using 6 lines per post which fits a
    // 2-line body comfortably with header + reactions + border.
    if app.posts.is_empty() {
        draw_placeholder(f, area, "Feed", "no posts");
        return;
    }

    // The feed scrolls by moving a "window start" so the selected post is
    // always visible. Each card needs ~6 rows.
    const CARD_HEIGHT: u16 = 6;
    let visible = (area.height / CARD_HEIGHT).max(1) as usize;
    let start = if app.selected_post >= visible {
        app.selected_post + 1 - visible
    } else {
        0
    };
    let end = (start + visible).min(app.posts.len());

    let constraints: Vec<Constraint> = (start..end)
        .map(|_| Constraint::Length(CARD_HEIGHT))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (slot, idx) in (start..end).enumerate() {
        let post = &app.posts[idx];
        let selected = idx == app.selected_post;
        draw_post_card(f, chunks[slot], post, selected);
    }
}

fn draw_post_card(f: &mut Frame, area: Rect, post: &Post, selected: bool) {
    let border_color = if selected {
        accent::SELECTED_BORDER
    } else {
        accent::DIM_BORDER
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if selected {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        post_header_line(post),
        Line::from(Span::raw(post.body)),
        reaction_bar_line(post.hearts, post.sats, post.reposts, post.comments),
    ];

    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(body, inner);
}

fn post_header_line(post: &Post) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("@{}", post.author),
            Style::default()
                .fg(author_color(post.author))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(accent::TIMESTAMP)),
        Span::styled(post.npub, Style::default().fg(accent::TIMESTAMP)),
        Span::styled("  ·  ", Style::default().fg(accent::TIMESTAMP)),
        Span::styled(
            format!("{} ago", post.ago),
            Style::default().fg(accent::TIMESTAMP),
        ),
    ])
}

fn reaction_bar_line(hearts: u32, sats: u32, reposts: u32, comments: u32) -> Line<'static> {
    Line::from(vec![
        Span::styled("\u{2665} ", Style::default().fg(accent::HEART)),
        Span::raw(format!("{hearts}  ")),
        Span::styled("\u{26a1} ", Style::default().fg(accent::ZAP)),
        Span::raw(format!("{} sats  ", format_sats(sats))),
        Span::styled("\u{21ba} ", Style::default().fg(accent::REPOST)),
        Span::raw(format!("{reposts}  ")),
        Span::styled("\u{1f4ac} ", Style::default().fg(accent::COMMENT)),
        Span::raw(format!("{comments}")),
    ])
}

fn format_sats(sats: u32) -> String {
    // Thousands separators with commas — easier to scan than raw integers.
    let s = sats.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

fn draw_thread(f: &mut Frame, area: Rect, app: &App) {
    let post = &app.posts[app.selected_post];

    // We render the thread as a single Paragraph so wrapping and scrolling
    // behave consistently. Indent replies with two spaces per level.
    let mut lines: Vec<Line> = Vec::new();
    lines.push(post_header_line(post));
    lines.push(Line::from(Span::raw(post.body)));
    lines.push(reaction_bar_line(
        post.hearts,
        post.sats,
        post.reposts,
        post.comments,
    ));
    lines.push(Line::from(""));

    for reply in &post.replies {
        push_reply(&mut lines, reply, 1);
    }

    if post.replies.is_empty() {
        lines.push(Line::styled(
            "  (no replies yet — Esc to go back)",
            Style::default().fg(accent::TIMESTAMP),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(accent::SELECTED_BORDER))
        .title(Span::styled(
            " Thread  (Esc to go back) ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn push_reply(lines: &mut Vec<Line<'static>>, reply: &Reply, depth: usize) {
    let indent = "  ".repeat(depth);
    let prefix = format!("{indent}\u{2937} ");
    lines.push(Line::from(vec![
        Span::styled(prefix, Style::default().fg(accent::DIM_BORDER)),
        Span::styled(
            format!("@{}", reply.author),
            Style::default()
                .fg(author_color(reply.author))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(accent::TIMESTAMP)),
        Span::styled(
            format!("{} ago", reply.ago),
            Style::default().fg(accent::TIMESTAMP),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw(format!("{indent}  ")),
        Span::raw(reply.body.to_string()),
    ]));
    lines.push(Line::from(""));
    for nested in &reply.nested {
        push_reply(lines, nested, depth + 1);
    }
}

fn draw_relays(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .relays
        .iter()
        .map(|r| {
            let (glyph, color, suffix) = match r.status {
                RelayStatus::Connected => ("\u{25cf}", Color::Green, ""),
                RelayStatus::Disconnected => ("\u{25cb}", Color::Red, ""),
                RelayStatus::Connecting => ("\u{25cf}", accent::RELAY_CONNECTING, " (connecting)"),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::raw(r.url),
                Span::styled(suffix, Style::default().fg(accent::TIMESTAMP)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent::DIM_BORDER))
            .title(Span::styled(
                " Relays ",
                Style::default().fg(Color::White).add_modifier(Modifier::DIM),
            )),
    );
    f.render_widget(list, area);
}

fn draw_dm_previews(f: &mut Frame, area: Rect, app: &App) {
    // This is the right-panel "DMs" preview — distinct from the DMs *tab*,
    // which shows the full conversation in the left pane. The right-panel
    // shows the first line of each thread regardless of which tab is active.
    let items: Vec<ListItem> = app
        .dms
        .iter()
        .map(|t| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("@{}", t.peer),
                    Style::default()
                        .fg(author_color(t.peer))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": "),
                Span::raw(t.preview),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent::DIM_BORDER))
            .title(Span::styled(
                " DMs ",
                Style::default().fg(Color::White).add_modifier(Modifier::DIM),
            )),
    );
    f.render_widget(list, area);
}

fn draw_dm_list_pane(f: &mut Frame, area: Rect, app: &App) {
    // The DMs tab in the left pane: list of conversations on the left
    // half, and the currently-selected conversation preview on the right
    // half. Selecting (Enter) zooms into the chat (FeedView::DmChat).
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let items: Vec<ListItem> = app
        .dms
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == app.selected_dm {
                Style::default()
                    .fg(Color::White)
                    .bg(accent::TAB_ACTIVE_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(author_color(t.peer))
            };
            ListItem::new(Line::styled(format!(" @{} ", t.peer), style))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent::DIM_BORDER))
            .title(Span::styled(
                " Conversations ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
    );
    f.render_widget(list, cols[0]);

    // Right column: preview of the selected thread (read-only, Enter to open).
    let lines = dm_chat_lines(&app.dms[app.selected_dm]);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent::DIM_BORDER))
                .title(Span::styled(
                    format!(" Preview — Enter to open @{} ", app.dms[app.selected_dm].peer),
                    Style::default().fg(Color::White).add_modifier(Modifier::DIM),
                )),
        );
    f.render_widget(para, cols[1]);
}

fn draw_dm_chat(f: &mut Frame, area: Rect, app: &App) {
    let thread = &app.dms[app.selected_dm];
    let lines = dm_chat_lines(thread);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(accent::SELECTED_BORDER))
        .title(Span::styled(
            format!(" Chat with @{}  (Esc to go back) ", thread.peer),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn dm_chat_lines(thread: &DmThread) -> Vec<Line<'static>> {
    thread
        .messages
        .iter()
        .map(|m| match m.from {
            Some(peer) => Line::from(vec![
                Span::styled(
                    format!("@{}: ", peer),
                    Style::default()
                        .fg(author_color(peer))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(m.body.to_string()),
            ]),
            None => Line::from(vec![
                Span::styled(
                    "you: ",
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw(m.body.to_string()),
            ]),
        })
        .collect()
}

fn draw_placeholder(f: &mut Frame, area: Rect, title: &str, msg: &str) {
    // We `to_string()` both inputs so the resulting widget owns its data and
    // doesn't borrow the (transient) `&str` arguments passed in from the
    // caller — which would otherwise force the function into a lifetime
    // gymnastics knot for no benefit.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent::DIM_BORDER))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(Color::White).add_modifier(Modifier::DIM),
        ));
    let para = Paragraph::new(Line::styled(
        msg.to_string(),
        Style::default().fg(accent::TIMESTAMP),
    ))
    .alignment(Alignment::Center)
    .block(block);
    f.render_widget(para, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    // Footer text adapts to the current view so the hints stay relevant.
    let hint = match (app.tab, app.feed_view) {
        (Tab::Home, FeedView::Feed) => {
            "j/k:scroll  enter:thread  d:DMs  r:reply  /:search  q:quit"
        }
        (Tab::Home, FeedView::Thread) => "esc:back  r:reply  q:quit",
        (Tab::Dms, FeedView::Feed) => "j/k:scroll  enter:open  h/l:tabs  q:quit",
        (Tab::Dms, FeedView::DmChat) => "esc:back  r:reply  q:quit",
        _ => "h/l:tabs  1-4:jump  q:quit",
    };
    let para = Paragraph::new(Line::styled(
        format!(" {hint} "),
        Style::default()
            .fg(Color::Gray)
            .bg(accent::FOOTER_BG)
            .add_modifier(Modifier::DIM),
    ));
    f.render_widget(para, area);
}

// ---------------------------------------------------------------------------
// Event loop.
// ---------------------------------------------------------------------------

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    while !app.should_quit {
        terminal.draw(|f| ui(f, &app))?;

        // Poll instead of block so we can paint cheaply on resize events and
        // remain responsive if we ever add a tick stream later.
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                handle_key(&mut app, key.code, key.modifiers);
            }
            Event::Resize(_, _) => { /* loop redraws on next iteration */ }
            _ => {}
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    // Ctrl-C quits regardless of context. We check this before the per-key
    // match so it can't be shadowed by a tab-switch binding.
    if mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        app.should_quit = true;
        return;
    }

    match code {
        KeyCode::Char('q') => app.should_quit = true,

        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),

        KeyCode::Enter => app.activate(),
        KeyCode::Esc | KeyCode::Backspace => app.back(),

        KeyCode::Char('1') => app.set_tab(Tab::Home),
        KeyCode::Char('2') => app.set_tab(Tab::Notifications),
        KeyCode::Char('3') => app.set_tab(Tab::Dms),
        KeyCode::Char('4') => app.set_tab(Tab::Search),

        KeyCode::Char('h') => app.cycle_tab(-1),
        KeyCode::Char('l') => app.cycle_tab(1),

        KeyCode::Char('d') => app.set_tab(Tab::Dms),

        // `r` / `/` are intentionally stub no-ops in this mock. Keeping them
        // bound (rather than letting them fall through to nothing) makes it
        // obvious to a reader what the real app would wire up.
        KeyCode::Char('r') => { /* TODO: reply composer */ }
        KeyCode::Char('/') => { /* TODO: search palette */ }

        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Terminal lifecycle.
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // We deliberately catch the run-loop result so cleanup runs even on
    // error. Without this the terminal is left in raw mode with the
    // alternate screen active, which makes the shell look "stuck".
    let result = run(&mut terminal);

    // Cleanup, in inverse order of setup.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

// ---------------------------------------------------------------------------
// Seed data.
// ---------------------------------------------------------------------------

fn seed_posts() -> Vec<Post> {
    vec![
        Post {
            author: "alice",
            npub: "npub1abc\u{2026}",
            ago: "2m",
            body: "just shipped something really cool after months of work on the nostr protocol stack",
            hearts: 12,
            sats: 4_200,
            reposts: 3,
            comments: 7,
            replies: vec![
                Reply {
                    author: "carol",
                    npub: "npub1def\u{2026}",
                    ago: "1m",
                    body: "congrats!! what's the tldr?",
                    nested: vec![Reply {
                        author: "alice",
                        npub: "npub1abc\u{2026}",
                        ago: "30s",
                        body: "basically a new way to handle key delegation without custodial risk",
                        nested: vec![],
                    }],
                },
                Reply {
                    author: "bob",
                    npub: "npub1xyz\u{2026}",
                    ago: "1m",
                    body: "\u{1f525}\u{1f525}\u{1f525}",
                    nested: vec![],
                },
            ],
        },
        Post {
            author: "bob",
            npub: "npub1xyz\u{2026}",
            ago: "5m",
            body: "gm everyone \u{1f305}",
            hearts: 31,
            sats: 500,
            reposts: 8,
            comments: 2,
            replies: vec![],
        },
        Post {
            author: "carol",
            npub: "npub1def\u{2026}",
            ago: "12m",
            body: "reminder that you own your nostr identity forever, no platform can take it away",
            hearts: 89,
            sats: 21_000,
            reposts: 44,
            comments: 15,
            replies: vec![],
        },
        Post {
            author: "dave",
            npub: "npub1ghi\u{2026}",
            ago: "23m",
            body: "built a lightning \u{26a1} integration that took 3 hours, would've taken weeks on legacy platforms",
            hearts: 22,
            sats: 8_800,
            reposts: 11,
            comments: 6,
            replies: vec![],
        },
        Post {
            author: "eve",
            npub: "npub1jkl\u{2026}",
            ago: "44m",
            body: "the nostr ecosystem is growing faster than i expected honestly",
            hearts: 67,
            sats: 3_300,
            reposts: 19,
            comments: 9,
            replies: vec![],
        },
        Post {
            author: "alice",
            npub: "npub1abc\u{2026}",
            ago: "1h",
            body: "reading through the latest NIPs, some really clever stuff being proposed",
            hearts: 14,
            sats: 1_100,
            reposts: 5,
            comments: 3,
            replies: vec![],
        },
        Post {
            author: "frank",
            npub: "npub1mno\u{2026}",
            ago: "2h",
            body: "spent the morning setting up my own relay, surprisingly easy",
            hearts: 18,
            sats: 750,
            reposts: 7,
            comments: 4,
            replies: vec![],
        },
        Post {
            author: "carol",
            npub: "npub1def\u{2026}",
            ago: "3h",
            body: "NIP-46 bunker signing is underrated, everyone should use it",
            hearts: 45,
            sats: 9_900,
            reposts: 28,
            comments: 12,
            replies: vec![],
        },
    ]
}

fn seed_relays() -> Vec<Relay> {
    vec![
        Relay {
            url: "wss://relay.damus.io",
            status: RelayStatus::Connected,
        },
        Relay {
            url: "wss://nos.lol",
            status: RelayStatus::Connected,
        },
        Relay {
            url: "wss://nostr.wine",
            status: RelayStatus::Connected,
        },
        Relay {
            url: "wss://relay.nostr.band",
            status: RelayStatus::Disconnected,
        },
        Relay {
            url: "wss://nostr-pub.wellorder.net",
            status: RelayStatus::Connecting,
        },
    ]
}

fn seed_dms() -> Vec<DmThread> {
    vec![
        DmThread {
            peer: "bob",
            preview: "hey are you going to the nostr summit?",
            messages: vec![
                DmMessage {
                    from: Some("bob"),
                    body: "hey are you going to the nostr summit?",
                },
                DmMessage {
                    from: None,
                    body: "probably! when is it?",
                },
                DmMessage {
                    from: Some("bob"),
                    body: "next month in Prague",
                },
                DmMessage {
                    from: None,
                    body: "oh nice, I might make it",
                },
                DmMessage {
                    from: Some("bob"),
                    body: "would be great to meet IRL",
                },
            ],
        },
        DmThread {
            peer: "carol",
            preview: "wen moon \u{1f319}",
            messages: vec![DmMessage {
                from: Some("carol"),
                body: "wen moon \u{1f319}",
            }],
        },
        DmThread {
            peer: "dave",
            preview: "check this relay out, it's super fast",
            messages: vec![DmMessage {
                from: Some("dave"),
                body: "check this relay out, it's super fast",
            }],
        },
    ]
}
