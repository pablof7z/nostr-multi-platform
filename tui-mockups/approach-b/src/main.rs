//! Chirp TUI Mockup — Approach B: Master-Detail Split.
//!
//! Standalone ratatui demo. 100% hardcoded fake data, no network. Three
//! sections vertically: header (tab bar), body (master/detail), footer
//! (relay health + key hints). On the `Home` / `Notifications` / `Search`
//! tabs the left pane is a post list and the right pane is a single
//! selected post with threaded replies. On the `DMs` tab the left pane
//! becomes a conversation list and the right pane becomes the selected
//! conversation transcript.
//!
//! Key bindings:
//!   j / Down         — move selection down in left pane
//!   k / Up           — move selection up in left pane
//!   Shift+J / Shift+K— scroll detail pane down / up
//!   Tab / 1..4       — switch tabs (Home / Notifications / DMs / Search)
//!   q / Esc          — quit

use std::io::{self, Stdout};
use std::panic;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Truecolor palette. Chosen for a dark-navy "Chirp" feel with high-contrast
/// accents on reactions and relay-health dots.
mod palette {
    use ratatui::style::Color;

    pub const HEADER_BG: Color = Color::Rgb(0x1a, 0x1a, 0x2e);
    pub const FOOTER_BG: Color = Color::Rgb(0x12, 0x12, 0x22);
    pub const DETAIL_BG: Color = Color::Rgb(0x10, 0x10, 0x1c);
    pub const LIST_BG: Color = Color::Rgb(0x14, 0x14, 0x24);
    pub const SELECTED_BG: Color = Color::Rgb(0x22, 0x2a, 0x55);

    pub const ACCENT_CYAN: Color = Color::Rgb(0x55, 0xd0, 0xe0);
    pub const DIM_TEXT: Color = Color::Rgb(0x88, 0x88, 0x99);
    pub const DIMMER_TEXT: Color = Color::Rgb(0x55, 0x55, 0x66);
    pub const BODY_TEXT: Color = Color::Rgb(0xee, 0xee, 0xee);

    pub const HEART: Color = Color::Rgb(0xff, 0x5d, 0x5d);
    pub const ZAP: Color = Color::Rgb(0xff, 0xd1, 0x3a);
    pub const REPOST: Color = Color::Rgb(0x66, 0xe0, 0x86);
    pub const REPLY: Color = Color::Rgb(0x6a, 0xc8, 0xff);

    pub const RELAY_OK: Color = Color::Rgb(0x55, 0xe0, 0x88);
    pub const RELAY_DOWN: Color = Color::Rgb(0xff, 0x5d, 0x5d);

    /// Author-color cycle used for both the left-pane author names and the
    /// detail-pane large author heading. Bright, distinguishable hues.
    pub const AUTHOR_CYCLE: [Color; 6] = [
        Color::Rgb(0xff, 0x8e, 0xc8), // pink — alice
        Color::Rgb(0x8e, 0xc8, 0xff), // sky — bob
        Color::Rgb(0xc8, 0xff, 0x8e), // lime — carol
        Color::Rgb(0xff, 0xc8, 0x8e), // peach — dave
        Color::Rgb(0xc8, 0x8e, 0xff), // violet — eve
        Color::Rgb(0x8e, 0xff, 0xe0), // teal — frank
    ];
}

// ---------------------------------------------------------------------------
// Fake domain model
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Post {
    author: &'static str,
    npub: &'static str,
    timestamp: &'static str,
    body: &'static str,
    likes: u32,
    sats: u32,
    reposts: u32,
    reply_count: u32,
    replies: Vec<Reply>,
}

#[derive(Clone)]
struct Reply {
    author: &'static str,
    timestamp: &'static str,
    body: &'static str,
    depth: u8,
}

#[derive(Clone)]
struct Conversation {
    peer: &'static str,
    unread: u32,
    last_message: &'static str,
    messages: Vec<DmMessage>,
}

#[derive(Clone)]
struct DmMessage {
    from_me: bool,
    sender: &'static str,
    body: &'static str,
}

#[derive(Clone)]
struct Relay {
    url: &'static str,
    connected: bool,
}

fn fake_posts() -> Vec<Post> {
    vec![
        Post {
            author: "@alice",
            npub: "npub1abc…def",
            timestamp: "2 minutes ago",
            body: "just shipped something really cool after months of work \
                   on the nostr protocol stack",
            likes: 12,
            sats: 4_200,
            reposts: 3,
            reply_count: 7,
            replies: vec![
                Reply {
                    author: "@carol",
                    timestamp: "1m ago",
                    body: "congrats!! what's the tldr?",
                    depth: 0,
                },
                Reply {
                    author: "@alice",
                    timestamp: "30s ago",
                    body: "basically a new way to handle key delegation \
                           without custodial risk",
                    depth: 1,
                },
                Reply {
                    author: "@bob",
                    timestamp: "2m ago",
                    body: "🔥🔥🔥",
                    depth: 0,
                },
            ],
        },
        Post {
            author: "@bob",
            npub: "npub1xyz…uvw",
            timestamp: "5 minutes ago",
            body: "gm everyone 🌅",
            likes: 31,
            sats: 500,
            reposts: 8,
            reply_count: 2,
            replies: vec![
                Reply {
                    author: "@dave",
                    timestamp: "3m ago",
                    body: "gm! good day to build",
                    depth: 0,
                },
                Reply {
                    author: "@alice",
                    timestamp: "4m ago",
                    body: "gm 🌞",
                    depth: 0,
                },
            ],
        },
        Post {
            author: "@carol",
            npub: "npub1def…ghi",
            timestamp: "12 minutes ago",
            body: "reminder that you own your nostr identity forever, no \
                   platform can take it away",
            likes: 89,
            sats: 21_000,
            reposts: 44,
            reply_count: 15,
            replies: vec![],
        },
        Post {
            author: "@dave",
            npub: "npub1ghi…jkl",
            timestamp: "18 minutes ago",
            body: "built a lightning ⚡ integration that took 3 hours, \
                   would've taken weeks on legacy platforms",
            likes: 22,
            sats: 8_800,
            reposts: 11,
            reply_count: 6,
            replies: vec![],
        },
        Post {
            author: "@eve",
            npub: "npub1jkl…mno",
            timestamp: "25 minutes ago",
            body: "the nostr ecosystem is growing faster than i expected \
                   honestly",
            likes: 67,
            sats: 3_300,
            reposts: 19,
            reply_count: 9,
            replies: vec![],
        },
        Post {
            author: "@alice",
            npub: "npub1abc…def",
            timestamp: "28 minutes ago",
            body: "reading through the latest NIPs, some really clever \
                   stuff being proposed",
            likes: 14,
            sats: 1_100,
            reposts: 5,
            reply_count: 3,
            replies: vec![],
        },
        Post {
            author: "@frank",
            npub: "npub1mno…pqr",
            timestamp: "32 minutes ago",
            body: "spent the morning setting up my own relay, surprisingly \
                   easy with the new tooling",
            likes: 18,
            sats: 750,
            reposts: 7,
            reply_count: 4,
            replies: vec![],
        },
        Post {
            author: "@carol",
            npub: "npub1def…ghi",
            timestamp: "41 minutes ago",
            body: "NIP-46 bunker signing is underrated, everyone should use \
                   it for key security",
            likes: 45,
            sats: 9_900,
            reposts: 28,
            reply_count: 12,
            replies: vec![],
        },
    ]
}

fn fake_conversations() -> Vec<Conversation> {
    vec![
        Conversation {
            peer: "@bob",
            unread: 2,
            last_message: "hey are you going to the nostr summit?",
            messages: vec![
                DmMessage { from_me: false, sender: "@bob", body: "hey are you going to the nostr summit?" },
                DmMessage { from_me: true,  sender: "You",  body: "probably! when is it?" },
                DmMessage { from_me: false, sender: "@bob", body: "next month in Prague" },
                DmMessage { from_me: true,  sender: "You",  body: "oh nice, I might make it" },
                DmMessage { from_me: false, sender: "@bob", body: "would be great to meet IRL" },
                DmMessage { from_me: false, sender: "@bob", body: "there's also a hackathon the day before" },
                DmMessage { from_me: true,  sender: "You",  body: "oh that sounds fun, I'll try to come for that" },
            ],
        },
        Conversation {
            peer: "@carol",
            unread: 0,
            last_message: "let me know what you think!",
            messages: vec![
                DmMessage { from_me: false, sender: "@carol", body: "i pushed the relay tuning patch" },
                DmMessage { from_me: false, sender: "@carol", body: "let me know what you think!" },
            ],
        },
        Conversation {
            peer: "@dave",
            unread: 1,
            last_message: "check this relay out, it's super fast",
            messages: vec![
                DmMessage { from_me: false, sender: "@dave", body: "check this relay out, it's super fast" },
            ],
        },
    ]
}

fn fake_relays() -> Vec<Relay> {
    vec![
        Relay { url: "wss://relay.damus.io",   connected: true  },
        Relay { url: "wss://nos.lol",          connected: true  },
        Relay { url: "wss://nostr.wine",       connected: true  },
        Relay { url: "wss://relay.nostr.band", connected: false },
    ]
}

/// Deterministic author -> color mapping. We hash by the first non-`@`
/// character so the same handle always renders with the same hue across the
/// list and the detail pane.
fn author_color(author: &str) -> Color {
    let key = author.trim_start_matches('@').as_bytes();
    let idx = key.first().copied().unwrap_or(0) as usize % palette::AUTHOR_CYCLE.len();
    palette::AUTHOR_CYCLE[idx]
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Notifications,
    Dms,
    Search,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Home, Tab::Notifications, Tab::Dms, Tab::Search];

    fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Notifications => "Notifications",
            Tab::Dms => "DMs",
            Tab::Search => "Search",
        }
    }

    fn from_digit(d: char) -> Option<Tab> {
        match d {
            '1' => Some(Tab::Home),
            '2' => Some(Tab::Notifications),
            '3' => Some(Tab::Dms),
            '4' => Some(Tab::Search),
            _ => None,
        }
    }
}

struct App {
    active_tab: Tab,
    posts: Vec<Post>,
    conversations: Vec<Conversation>,
    relays: Vec<Relay>,
    /// Selected index in the *currently visible* left pane (posts list on
    /// non-DM tabs, conversation list on DM tab). We keep one cursor per
    /// pane so tab-switching doesn't lose position.
    post_cursor: usize,
    convo_cursor: usize,
    /// Detail-pane vertical scroll offset, clamped on render. Kept separate
    /// per content-mode for the same reason.
    post_detail_scroll: u16,
    dm_detail_scroll: u16,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            active_tab: Tab::Home,
            posts: fake_posts(),
            conversations: fake_conversations(),
            relays: fake_relays(),
            post_cursor: 0,
            convo_cursor: 0,
            post_detail_scroll: 0,
            dm_detail_scroll: 0,
            should_quit: false,
        }
    }

    fn in_dm_mode(&self) -> bool {
        self.active_tab == Tab::Dms
    }

    fn list_len(&self) -> usize {
        if self.in_dm_mode() {
            self.conversations.len()
        } else {
            self.posts.len()
        }
    }

    fn cursor(&self) -> usize {
        if self.in_dm_mode() {
            self.convo_cursor
        } else {
            self.post_cursor
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = self.list_len();
        if len == 0 {
            return;
        }
        let cur = self.cursor() as i32;
        let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
        if self.in_dm_mode() {
            self.convo_cursor = next;
            self.dm_detail_scroll = 0;
        } else {
            self.post_cursor = next;
            self.post_detail_scroll = 0;
        }
    }

    fn scroll_detail(&mut self, delta: i32) {
        // Clamp at zero; ratatui's Paragraph::scroll silently caps high
        // values against the wrapped output, so we don't need an upper
        // bound for the mockup.
        let slot = if self.in_dm_mode() {
            &mut self.dm_detail_scroll
        } else {
            &mut self.post_detail_scroll
        };
        let next = (*slot as i32 + delta).max(0) as u16;
        *slot = next;
    }

    fn set_tab(&mut self, tab: Tab) {
        if tab != self.active_tab {
            self.active_tab = tab;
        }
    }

    fn next_tab(&mut self) {
        let i = Tab::ALL.iter().position(|t| *t == self.active_tab).unwrap_or(0);
        self.active_tab = Tab::ALL[(i + 1) % Tab::ALL.len()];
    }
}

// ---------------------------------------------------------------------------
// Terminal lifecycle
// ---------------------------------------------------------------------------

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// Install a panic hook that restores the terminal before the default hook
/// runs. Without this, a panic mid-render leaves the user's terminal in
/// raw mode with the alternate screen active, which usually means
/// `reset`/closing the tab. Cheap, and worth it even for a mockup we'll
/// run many times during the build/fix loop.
fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    restore_terminal()?;
    result
}

fn run(terminal: &mut Tui) -> io::Result<()> {
    let mut app = App::new();
    while !app.should_quit {
        terminal.draw(|f| draw(f, &app))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                // Filter to Press only — without this, some terminals
                // (notably Windows + macOS iTerm in certain modes) deliver
                // Press+Release pairs and selections jump two rows per tap.
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key(&mut app, key.code, key.modifiers);
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    let shift = mods.contains(KeyModifiers::SHIFT);
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') if shift => app.scroll_detail(1),
        KeyCode::Char('k') if shift => app.scroll_detail(-1),
        KeyCode::Char('J') => app.scroll_detail(1),
        KeyCode::Char('K') => app.scroll_detail(-1),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(-1),
        KeyCode::Tab => app.next_tab(),
        KeyCode::Char(c @ '1'..='4') => {
            if let Some(t) = Tab::from_digit(c) {
                app.set_tab(t);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn draw(f: &mut ratatui::Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header / tab bar
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
        ])
        .split(f.area());

    draw_header(f, root[0], app);
    draw_body(f, root[1], app);
    draw_footer(f, root[2], app);
}

// --- Header ---------------------------------------------------------------

fn draw_header(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::with_capacity(2 + Tab::ALL.len() * 2);
    spans.push(Span::styled(
        " ◆ CHIRP    ",
        Style::default()
            .fg(palette::ACCENT_CYAN)
            .bg(palette::HEADER_BG)
            .add_modifier(Modifier::BOLD),
    ));
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let active = *tab == app.active_tab;
        let bullet = if active { "● " } else { "○ " };
        let bullet_style = if active {
            Style::default().fg(palette::ACCENT_CYAN).bg(palette::HEADER_BG)
        } else {
            Style::default().fg(palette::DIMMER_TEXT).bg(palette::HEADER_BG)
        };
        let label_style = if active {
            Style::default()
                .fg(Color::White)
                .bg(palette::HEADER_BG)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(palette::DIM_TEXT).bg(palette::HEADER_BG)
        };
        spans.push(Span::styled(bullet, bullet_style));
        spans.push(Span::styled(tab.label(), label_style));
        if i + 1 != Tab::ALL.len() {
            spans.push(Span::styled("   ", Style::default().bg(palette::HEADER_BG)));
        }
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(Style::default().bg(palette::HEADER_BG));
    f.render_widget(para, area);
}

// --- Body -----------------------------------------------------------------

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    if app.in_dm_mode() {
        draw_conversation_list(f, cols[0], app);
        draw_conversation_detail(f, cols[1], app);
    } else {
        draw_post_list(f, cols[0], app);
        draw_post_detail(f, cols[1], app);
    }
}

// --- Left pane: post list -------------------------------------------------

fn draw_post_list(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(palette::DIMMER_TEXT))
        .style(Style::default().bg(palette::LIST_BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Each post takes a 3-line "card": header row (▶ @author · time),
    // body preview row, blank spacer. We render manually rather than via
    // List so we can control per-author colors and the cyan selection
    // gutter on the leftmost column.
    let mut lines: Vec<Line> = Vec::new();
    let width = inner.width.saturating_sub(2) as usize;

    for (i, post) in app.posts.iter().enumerate() {
        let selected = i == app.post_cursor;
        let gutter_glyph = if selected { "▶ " } else { "  " };
        let gutter_style = if selected {
            Style::default()
                .fg(palette::ACCENT_CYAN)
                .bg(palette::SELECTED_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette::DIMMER_TEXT).bg(palette::LIST_BG)
        };
        let row_bg = if selected { palette::SELECTED_BG } else { palette::LIST_BG };

        // Header row: gutter + author (colored) + " · " + timestamp (dim).
        let author_style = Style::default()
            .fg(author_color(post.author))
            .bg(row_bg)
            .add_modifier(Modifier::BOLD);
        let ts_style = Style::default()
            .fg(palette::DIM_TEXT)
            .bg(row_bg)
            .add_modifier(Modifier::ITALIC);
        let short_ts = short_timestamp(post.timestamp);
        let short_ts_len = short_ts.chars().count();
        let header_used = 2 + post.author.chars().count() + 3 + short_ts_len;
        lines.push(Line::from(vec![
            Span::styled(gutter_glyph, gutter_style),
            Span::styled(post.author, author_style),
            Span::styled(" · ", Style::default().fg(palette::DIMMER_TEXT).bg(row_bg)),
            Span::styled(short_ts, ts_style),
            // Pad the rest of the row with the selection background so the
            // highlight spans the full pane width.
            Span::styled(
                pad_right("", width.saturating_sub(header_used)),
                Style::default().bg(row_bg),
            ),
        ]));

        // Body preview row — single line, truncated to pane width.
        let preview = truncate_line(post.body, width.saturating_sub(2));
        let body_style = if selected {
            Style::default().fg(Color::White).bg(row_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette::DIM_TEXT).bg(row_bg)
        };
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(row_bg)),
            Span::styled(preview.clone(), body_style),
            Span::styled(
                pad_right("", width.saturating_sub(2 + preview.chars().count())),
                Style::default().bg(row_bg),
            ),
        ]));

        // Spacer row.
        lines.push(Line::from(Span::styled(
            pad_right("", width),
            Style::default().bg(palette::LIST_BG),
        )));
    }

    let para = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(palette::LIST_BG));
    f.render_widget(para, inner);
}

// --- Left pane: conversation list (DM mode) -------------------------------

fn draw_conversation_list(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(palette::DIMMER_TEXT))
        .style(Style::default().bg(palette::LIST_BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let width = inner.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (i, c) in app.conversations.iter().enumerate() {
        let selected = i == app.convo_cursor;
        let row_bg = if selected { palette::SELECTED_BG } else { palette::LIST_BG };
        let gutter_glyph = if selected { "▶ " } else { "  " };
        let gutter_style = if selected {
            Style::default()
                .fg(palette::ACCENT_CYAN)
                .bg(row_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette::DIMMER_TEXT).bg(row_bg)
        };

        // Header row: gutter + peer (colored) + unread badge.
        let peer_style = Style::default()
            .fg(author_color(c.peer))
            .bg(row_bg)
            .add_modifier(Modifier::BOLD);
        let mut header: Vec<Span> = vec![
            Span::styled(gutter_glyph, gutter_style),
            Span::styled(c.peer, peer_style),
        ];
        if c.unread > 0 {
            header.push(Span::styled(
                format!("  [{} unread]", c.unread),
                Style::default()
                    .fg(palette::ZAP)
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            header.push(Span::styled(
                "  [read]",
                Style::default().fg(palette::DIMMER_TEXT).bg(row_bg),
            ));
        }
        let header_len = visible_width(&header);
        header.push(Span::styled(
            pad_right("", width.saturating_sub(header_len)),
            Style::default().bg(row_bg),
        ));
        lines.push(Line::from(header));

        // Preview row: most recent message body, truncated, dim.
        let preview = truncate_line(c.last_message, width.saturating_sub(2));
        let body_style = if selected {
            Style::default().fg(Color::White).bg(row_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette::DIM_TEXT).bg(row_bg)
        };
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(row_bg)),
            Span::styled(preview.clone(), body_style),
            Span::styled(
                pad_right("", width.saturating_sub(2 + preview.chars().count())),
                Style::default().bg(row_bg),
            ),
        ]));

        // Spacer.
        lines.push(Line::from(Span::styled(
            pad_right("", width),
            Style::default().bg(palette::LIST_BG),
        )));
    }

    let para = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(palette::LIST_BG));
    f.render_widget(para, inner);
}

// --- Right pane: post detail with replies ---------------------------------

fn draw_post_detail(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let outer = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(palette::DETAIL_BG));
    f.render_widget(outer, area);

    let pad = area.inner(Margin { horizontal: 2, vertical: 1 });
    let post = match app.posts.get(app.post_cursor) {
        Some(p) => p,
        None => return,
    };

    let mut lines: Vec<Line> = Vec::new();

    // Author heading (large + colored), npub (dim).
    lines.push(Line::from(vec![
        Span::styled(
            post.author,
            Style::default()
                .fg(author_color(post.author))
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  ",
            Style::default().bg(palette::DETAIL_BG),
        ),
        Span::styled(
            post.npub,
            Style::default()
                .fg(palette::DIM_TEXT)
                .bg(palette::DETAIL_BG),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        post.timestamp,
        Style::default()
            .fg(palette::DIM_TEXT)
            .bg(palette::DETAIL_BG)
            .add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    // Body — relies on Paragraph wrap.
    lines.push(Line::from(Span::styled(
        post.body,
        Style::default().fg(palette::BODY_TEXT).bg(palette::DETAIL_BG),
    )));
    lines.push(Line::from(""));

    // Reaction bar.
    lines.push(Line::from(vec![
        Span::styled(
            format!("♥ {}", post.likes),
            Style::default()
                .fg(palette::HEART)
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::default().bg(palette::DETAIL_BG)),
        Span::styled(
            format!("⚡ {} sats", format_thousands(post.sats)),
            Style::default()
                .fg(palette::ZAP)
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::default().bg(palette::DETAIL_BG)),
        Span::styled(
            format!("↺ {}", post.reposts),
            Style::default().fg(palette::REPOST).bg(palette::DETAIL_BG),
        ),
        Span::styled("   ", Style::default().bg(palette::DETAIL_BG)),
        Span::styled(
            format!("💬 {}", post.reply_count),
            Style::default().fg(palette::REPLY).bg(palette::DETAIL_BG),
        ),
    ]));
    lines.push(Line::from(""));

    // Replies header.
    lines.push(Line::from(vec![Span::styled(
        "─── Replies ──────────────────────────────",
        Style::default()
            .fg(palette::DIMMER_TEXT)
            .bg(palette::DETAIL_BG),
    )]));
    lines.push(Line::from(""));

    // Thread.
    for reply in &post.replies {
        lines.extend(render_reply(reply));
        lines.push(Line::from(""));
    }
    if post.replies.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no replies yet)",
            Style::default()
                .fg(palette::DIMMER_TEXT)
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let content_lines = lines.len() as u16;
    let scroll = app.post_detail_scroll.min(content_lines.saturating_sub(1));

    let para = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .style(Style::default().bg(palette::DETAIL_BG));
    f.render_widget(para, pad);

    // Scrollbar — only meaningful if content exceeds visible rows.
    if content_lines > pad.height {
        let mut state = ScrollbarState::new(content_lines as usize)
            .position(scroll as usize)
            .viewport_content_length(pad.height as usize);
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .style(Style::default().fg(palette::DIMMER_TEXT));
        f.render_stateful_widget(bar, area, &mut state);
    }
}

/// Render one reply (plus the gutter for any nested replies underneath it).
/// Depth 0 uses a cyan gutter; deeper nesting uses dim gray, matching the
/// spec ("vertical line gutter (│) in dim cyan ... Nested replies: further
/// indented, gutter in dim gray").
fn render_reply(reply: &Reply) -> Vec<Line<'static>> {
    let indent = "  ".repeat(reply.depth as usize + 1);
    let gutter_color = if reply.depth == 0 {
        palette::ACCENT_CYAN
    } else {
        palette::DIMMER_TEXT
    };
    let gutter = Span::styled(
        "│ ",
        Style::default().fg(gutter_color).bg(palette::DETAIL_BG),
    );

    let header = Line::from(vec![
        Span::styled(indent.clone(), Style::default().bg(palette::DETAIL_BG)),
        gutter.clone(),
        Span::styled(
            reply.author.to_string(),
            Style::default()
                .fg(author_color(reply.author))
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " · ",
            Style::default().fg(palette::DIMMER_TEXT).bg(palette::DETAIL_BG),
        ),
        Span::styled(
            reply.timestamp.to_string(),
            Style::default()
                .fg(palette::DIM_TEXT)
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::ITALIC),
        ),
    ]);

    let body = Line::from(vec![
        Span::styled(indent, Style::default().bg(palette::DETAIL_BG)),
        gutter,
        Span::styled(
            reply.body.to_string(),
            Style::default().fg(palette::BODY_TEXT).bg(palette::DETAIL_BG),
        ),
    ]);

    vec![header, body]
}

// --- Right pane: DM conversation detail -----------------------------------

fn draw_conversation_detail(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let outer = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(palette::DETAIL_BG));
    f.render_widget(outer, area);

    let pad = area.inner(Margin { horizontal: 2, vertical: 1 });

    let convo = match app.conversations.get(app.convo_cursor) {
        Some(c) => c,
        None => return,
    };

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            "Direct messages with ",
            Style::default().fg(palette::DIM_TEXT).bg(palette::DETAIL_BG),
        ),
        Span::styled(
            convo.peer,
            Style::default()
                .fg(author_color(convo.peer))
                .bg(palette::DETAIL_BG)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "end-to-end encrypted · NIP-17",
        Style::default()
            .fg(palette::DIM_TEXT)
            .bg(palette::DETAIL_BG)
            .add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    for msg in &convo.messages {
        let (label_color, body_color, prefix) = if msg.from_me {
            (palette::ACCENT_CYAN, palette::BODY_TEXT, "  ▶ ")
        } else {
            (author_color(msg.sender), palette::BODY_TEXT, "  ◀ ")
        };
        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                Style::default()
                    .fg(palette::DIMMER_TEXT)
                    .bg(palette::DETAIL_BG),
            ),
            Span::styled(
                msg.sender.to_string(),
                Style::default()
                    .fg(label_color)
                    .bg(palette::DETAIL_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                "      ",
                Style::default().bg(palette::DETAIL_BG),
            ),
            Span::styled(
                msg.body.to_string(),
                Style::default().fg(body_color).bg(palette::DETAIL_BG),
            ),
        ]));
        lines.push(Line::from(""));
    }

    let content_lines = lines.len() as u16;
    let scroll = app.dm_detail_scroll.min(content_lines.saturating_sub(1));

    let para = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .style(Style::default().bg(palette::DETAIL_BG));
    f.render_widget(para, pad);

    if content_lines > pad.height {
        let mut state = ScrollbarState::new(content_lines as usize)
            .position(scroll as usize)
            .viewport_content_length(pad.height as usize);
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .style(Style::default().fg(palette::DIMMER_TEXT));
        f.render_stateful_widget(bar, area, &mut state);
    }
}

// --- Footer ---------------------------------------------------------------

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" ", Style::default().bg(palette::FOOTER_BG)));
    for r in &app.relays {
        let (dot, dot_color) = if r.connected {
            ("● ", palette::RELAY_OK)
        } else {
            ("○ ", palette::RELAY_DOWN)
        };
        spans.push(Span::styled(
            dot,
            Style::default()
                .fg(dot_color)
                .bg(palette::FOOTER_BG)
                .add_modifier(Modifier::BOLD),
        ));
        // Strip "wss://" prefix to save horizontal real estate.
        let pretty = r.url.trim_start_matches("wss://");
        spans.push(Span::styled(
            pretty,
            Style::default().fg(palette::DIM_TEXT).bg(palette::FOOTER_BG),
        ));
        spans.push(Span::styled(
            "  ",
            Style::default().bg(palette::FOOTER_BG),
        ));
    }
    spans.push(Span::styled(
        "  │  ",
        Style::default()
            .fg(palette::DIMMER_TEXT)
            .bg(palette::FOOTER_BG),
    ));
    let hint_style = Style::default().fg(palette::DIM_TEXT).bg(palette::FOOTER_BG);
    let hint_key = Style::default()
        .fg(palette::BODY_TEXT)
        .bg(palette::FOOTER_BG)
        .add_modifier(Modifier::BOLD);
    spans.extend([
        Span::styled("j/k", hint_key),
        Span::styled(":nav  ", hint_style),
        Span::styled("J/K", hint_key),
        Span::styled(":scroll  ", hint_style),
        Span::styled("Tab/1-4", hint_key),
        Span::styled(":tabs  ", hint_style),
        Span::styled("q", hint_key),
        Span::styled(":quit", hint_style),
    ]);

    let para = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Left)
        .style(Style::default().bg(palette::FOOTER_BG));
    f.render_widget(para, area);
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Truncate a long timestamp like "12 minutes ago" -> "12m" for the list.
fn short_timestamp(ts: &str) -> String {
    // Heuristic, fine for the fixed fake corpus.
    if let Some(rest) = ts.strip_suffix(" minutes ago") {
        return format!("{rest}m");
    }
    if let Some(rest) = ts.strip_suffix(" minute ago") {
        return format!("{rest}m");
    }
    if let Some(rest) = ts.strip_suffix(" seconds ago") {
        return format!("{rest}s");
    }
    ts.to_string()
}

fn truncate_line(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let end = max.saturating_sub(1);
    let mut out: String = chars[..end].iter().collect();
    out.push('…');
    out
}

fn pad_right(s: &str, total: usize) -> String {
    let len = s.chars().count();
    if len >= total {
        return s.to_string();
    }
    let mut out = String::from(s);
    out.extend(std::iter::repeat(' ').take(total - len));
    out
}

fn visible_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

fn format_thousands(n: u32) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}
