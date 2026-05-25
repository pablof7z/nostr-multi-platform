//! Chirp TUI mock — Approach C: "Full-Width Feed with Modal Overlays".
//!
//! This is a standalone demo binary with 100% fake/hardcoded data. The feed
//! takes the full terminal width; secondary views (thread detail, DMs) appear
//! as overlay panels rendered over a dimmed copy of the feed.
//!
//! Layout strategy:
//!   * `App::draw` always renders the base layout (tab bar, feed, footer) into
//!     the frame buffer first.
//!   * If an overlay is active, the cells underneath the overlay are mutated
//!     in-place with `Modifier::DIM`, then the overlay panel is rendered on
//!     top via the `Clear` widget + a styled `Block`.
//!
//! Controls:
//!   * `j` / `k` (or arrow keys) — scroll the feed selection
//!   * `Enter`                   — open the thread overlay for the selected post
//!   * `d`                       — open the DMs overlay
//!   * `1` / `2` / `3` / `4`     — switch tabs (Home / Notifications / DMs / Search)
//!   * `h` / `n` / `s`           — tab mnemonics (Home / Notifications / Search)
//!   * `Esc`                     — close any open overlay
//!   * `q`                       — quit (only when no overlay is open)

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs, Widget, Wrap};
use ratatui::{Frame, Terminal};

// ---------------------------------------------------------------------------
// Color palette — bright, vivid truecolor values as specified.
// ---------------------------------------------------------------------------

const COLOR_BG: Color = Color::Rgb(0x0d, 0x0d, 0x0d);
const COLOR_BG_TAB: Color = Color::Rgb(0x11, 0x11, 0x11);
const COLOR_BORDER_DIM: Color = Color::Rgb(0x33, 0x33, 0x33);
const COLOR_BORDER_SEL: Color = Color::Rgb(0x00, 0xd4, 0xff);
const COLOR_NPUB_DIM: Color = Color::Rgb(0x66, 0x66, 0x66);
const COLOR_TIMESTAMP_DIM: Color = Color::Rgb(0x88, 0x88, 0x88);
const COLOR_BODY: Color = Color::Rgb(0xee, 0xee, 0xee);
const COLOR_OVERLAY_BG: Color = Color::Rgb(0x1a, 0x1a, 0x2a);
const COLOR_OVERLAY_BORDER: Color = Color::Rgb(0x00, 0xd4, 0xff);
const COLOR_OVERLAY_TITLE: Color = Color::Rgb(0xff, 0xff, 0xff);
const COLOR_OVERLAY_REPLY: Color = Color::Rgb(0xc8, 0xa8, 0x33);
const COLOR_FOOTER_HINT: Color = Color::Rgb(0x99, 0x99, 0x99);
const COLOR_FOOTER_SEP: Color = Color::Rgb(0x55, 0x55, 0x55);

// Author palette — cycled in order of first appearance.
const AUTHOR_PALETTE: &[Color] = &[
    Color::Rgb(0x00, 0xd4, 0xff), // cyan
    Color::Rgb(0x00, 0xff, 0x88), // green
    Color::Rgb(0xff, 0xdd, 0x00), // gold
    Color::Rgb(0xff, 0x6b, 0x9d), // pink
    Color::Rgb(0xa7, 0x8b, 0xfa), // lavender
];

// Reaction colors.
const COLOR_HEART: Color = Color::Rgb(0xff, 0x44, 0x44);
const COLOR_ZAP: Color = Color::Rgb(0xff, 0xdd, 0x00);
const COLOR_REPOST: Color = Color::Rgb(0x00, 0xcc, 0x66);
const COLOR_REPLY: Color = Color::Rgb(0x00, 0xaa, 0xff);

// Relay status dot colors.
const COLOR_RELAY_OK: Color = Color::Rgb(0x00, 0xff, 0x88);
const COLOR_RELAY_BAD: Color = Color::Rgb(0xff, 0x44, 0x44);

// ---------------------------------------------------------------------------
// Domain types — all data is hardcoded; see `App::new`.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Post {
    author: &'static str,
    npub: &'static str,
    timestamp: &'static str,
    body: &'static str,
    hearts: u32,
    zap_sats: u32,
    reposts: u32,
    replies: u32,
    /// Thread entries: (indent_level, author, time, body).
    /// `indent_level == 0` is a top-level reply to this post.
    thread: &'static [(u8, &'static str, &'static str, &'static str)],
}

#[derive(Clone)]
struct Relay {
    url: &'static str,
    connected: bool,
}

#[derive(Clone)]
struct DmConversation {
    author: &'static str,
    unread: u32,
    /// Last-message preview, used in the conversation list snippet.
    preview: &'static str,
    /// (sender, body) — sender == "You" for outbound messages.
    messages: &'static [(&'static str, &'static str)],
}

#[derive(Copy, Clone, PartialEq, Eq)]
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
}

#[derive(Copy, Clone)]
enum Overlay {
    None,
    Thread(usize),
    Dms,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct App {
    posts: Vec<Post>,
    relays: Vec<Relay>,
    dms: Vec<DmConversation>,
    /// Index of the currently selected post (drives j/k focus highlight).
    selected: usize,
    /// First visible post index (drives vertical scroll).
    scroll_top: usize,
    active_tab: Tab,
    overlay: Overlay,
    /// Selected DM conversation index (when DMs overlay is open).
    dm_selected: usize,
    /// Set true on `q` (only honored when no overlay is open).
    should_quit: bool,
    /// Stable per-author color assignment — order of first appearance.
    author_color: std::collections::HashMap<&'static str, Color>,
}

impl App {
    fn new() -> Self {
        let posts = fake_posts();
        let relays = fake_relays();
        let dms = fake_dms();

        // Assign each distinct author a stable palette color in first-appearance order.
        let mut author_color = std::collections::HashMap::new();
        let mut palette_idx = 0;
        for post in &posts {
            if !author_color.contains_key(post.author) {
                author_color.insert(post.author, AUTHOR_PALETTE[palette_idx % AUTHOR_PALETTE.len()]);
                palette_idx += 1;
            }
        }
        // Make sure DM correspondents and thread replies also have colors.
        for dm in &dms {
            if !author_color.contains_key(dm.author) {
                author_color.insert(dm.author, AUTHOR_PALETTE[palette_idx % AUTHOR_PALETTE.len()]);
                palette_idx += 1;
            }
        }
        for post in &posts {
            for (_, who, _, _) in post.thread {
                if !author_color.contains_key(*who) {
                    author_color.insert(*who, AUTHOR_PALETTE[palette_idx % AUTHOR_PALETTE.len()]);
                    palette_idx += 1;
                }
            }
        }

        Self {
            posts,
            relays,
            dms,
            selected: 0,
            scroll_top: 0,
            active_tab: Tab::Home,
            overlay: Overlay::None,
            dm_selected: 0,
            should_quit: false,
            author_color,
        }
    }

    fn author_color(&self, author: &str) -> Color {
        self.author_color
            .get(author)
            .copied()
            .unwrap_or(AUTHOR_PALETTE[0])
    }

    fn on_key(&mut self, code: KeyCode) {
        // Esc always closes an overlay first; if none open, it's a no-op.
        if matches!(code, KeyCode::Esc) {
            if !matches!(self.overlay, Overlay::None) {
                self.overlay = Overlay::None;
            }
            return;
        }

        // When an overlay is open, only navigation within the overlay and Esc
        // are honored. `q` is intentionally blocked so users can't quit by
        // accident from inside a modal.
        if !matches!(self.overlay, Overlay::None) {
            match self.overlay {
                Overlay::Dms => match code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if self.dm_selected + 1 < self.dms.len() {
                            self.dm_selected += 1;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.dm_selected = self.dm_selected.saturating_sub(1);
                    }
                    _ => {}
                },
                Overlay::Thread(_) => {
                    // Thread overlay is read-only in this mock.
                }
                Overlay::None => unreachable!(),
            }
            return;
        }

        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.posts.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.overlay = Overlay::Thread(self.selected);
            }
            KeyCode::Char('d') => {
                self.overlay = Overlay::Dms;
                self.active_tab = Tab::Dms;
            }
            KeyCode::Char('1') | KeyCode::Char('h') => self.active_tab = Tab::Home,
            KeyCode::Char('2') | KeyCode::Char('n') => self.active_tab = Tab::Notifications,
            KeyCode::Char('3') => {
                self.active_tab = Tab::Dms;
                self.overlay = Overlay::Dms;
            }
            KeyCode::Char('4') | KeyCode::Char('s') => self.active_tab = Tab::Search,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Fake data
// ---------------------------------------------------------------------------

fn fake_posts() -> Vec<Post> {
    vec![
        Post {
            author: "@alice",
            npub: "npub1abc…def",
            timestamp: "2 minutes ago",
            body: "just shipped something really cool after months of work on the nostr protocol stack",
            hearts: 12,
            zap_sats: 4_200,
            reposts: 3,
            replies: 7,
            thread: &[
                (0, "@carol", "1m", "congrats!! what's the tldr?"),
                (
                    1,
                    "@alice",
                    "30s",
                    "basically a new way to handle key delegation without custodial risk",
                ),
                (0, "@bob", "2m", "🔥🔥🔥"),
            ],
        },
        Post {
            author: "@bob",
            npub: "npub1xyz…uvw",
            timestamp: "5 minutes ago",
            body: "gm everyone 🌅",
            hearts: 31,
            zap_sats: 500,
            reposts: 8,
            replies: 2,
            thread: &[
                (0, "@dave", "3m", "gm! good day to build"),
                (0, "@alice", "2m", "gm 🌞"),
            ],
        },
        Post {
            author: "@carol",
            npub: "npub1def…ghi",
            timestamp: "12 minutes ago",
            body: "reminder that you own your nostr identity forever, no platform can take it away",
            hearts: 89,
            zap_sats: 21_000,
            reposts: 44,
            replies: 15,
            thread: &[],
        },
        Post {
            author: "@dave",
            npub: "npub1ghi…jkl",
            timestamp: "23 minutes ago",
            body: "built a lightning ⚡ integration that took 3 hours, would've taken weeks on legacy platforms",
            hearts: 22,
            zap_sats: 8_800,
            reposts: 11,
            replies: 6,
            thread: &[],
        },
        Post {
            author: "@eve",
            npub: "npub1jkl…mno",
            timestamp: "41 minutes ago",
            body: "the nostr ecosystem is growing faster than i expected honestly",
            hearts: 67,
            zap_sats: 3_300,
            reposts: 19,
            replies: 9,
            thread: &[],
        },
        Post {
            author: "@alice",
            npub: "npub1abc…def",
            timestamp: "1 hour ago",
            body: "reading through the latest NIPs, some really clever stuff being proposed",
            hearts: 14,
            zap_sats: 1_100,
            reposts: 5,
            replies: 3,
            thread: &[],
        },
        Post {
            author: "@frank",
            npub: "npub1mno…pqr",
            timestamp: "2 hours ago",
            body: "spent the morning setting up my own relay, surprisingly easy with the new tooling",
            hearts: 18,
            zap_sats: 750,
            reposts: 7,
            replies: 4,
            thread: &[],
        },
        Post {
            author: "@carol",
            npub: "npub1def…ghi",
            timestamp: "3 hours ago",
            body: "NIP-46 bunker signing is underrated, everyone should use it for key security",
            hearts: 45,
            zap_sats: 9_900,
            reposts: 28,
            replies: 12,
            thread: &[],
        },
    ]
}

fn fake_relays() -> Vec<Relay> {
    vec![
        Relay { url: "wss://relay.damus.io", connected: true },
        Relay { url: "wss://nos.lol", connected: true },
        Relay { url: "wss://nostr.wine", connected: true },
        Relay { url: "wss://relay.nostr.band", connected: false },
        Relay { url: "wss://nostr-pub.wellorder.net", connected: true },
    ]
}

fn fake_dms() -> Vec<DmConversation> {
    vec![
        DmConversation {
            author: "@bob",
            unread: 2,
            preview: "hey are you going to the nostr summit?",
            messages: &[
                ("@bob", "hey are you going to the nostr summit?"),
                ("You", "probably! when is it?"),
                ("@bob", "next month in Prague"),
                ("You", "oh nice, I might make it"),
                ("@bob", "would be great to meet IRL"),
            ],
        },
        DmConversation {
            author: "@carol",
            unread: 0,
            preview: "let me know what you think!",
            messages: &[
                ("@carol", "did you see the new NIP draft?"),
                ("You", "skimmed it, looks promising"),
                ("@carol", "let me know what you think!"),
            ],
        },
        DmConversation {
            author: "@dave",
            unread: 1,
            preview: "check this relay out, it's super fast",
            messages: &[
                ("@dave", "check this relay out, it's super fast"),
            ],
        },
    ]
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// Format a sats value like "4,200" so reaction rows stay readable.
fn fmt_sats(sats: u32) -> String {
    let s = sats.to_string();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Lines required to render a post card (header + blank + body + blank + reactions + 2 borders).
fn post_card_height(post: &Post, inner_width: u16) -> u16 {
    // Body wraps; estimate with simple char-per-line division.
    let body_lines = wrap_count(post.body, inner_width.saturating_sub(2) as usize);
    // header (1) + blank (1) + body (body_lines) + blank (1) + reactions (1) + 2 borders
    1 + 1 + body_lines as u16 + 1 + 1 + 2
}

/// Approximate the number of wrapped lines a string needs for the given width.
fn wrap_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut lines = 0usize;
    let mut current = 0usize;
    for word in text.split_whitespace() {
        // +1 for the separating space (except at start of line).
        let wlen = word.chars().count();
        let needed = if current == 0 { wlen } else { current + 1 + wlen };
        if needed > width {
            lines += 1;
            current = wlen.min(width);
        } else {
            current = needed;
        }
    }
    if current > 0 {
        lines += 1;
    }
    lines.max(1)
}

/// Render the top tab bar across `area`.
fn draw_tab_bar(frame: &mut Frame, area: Rect, app: &App) {
    let bg_block = Block::default().style(Style::default().bg(COLOR_BG_TAB));
    frame.render_widget(bg_block, area);

    // Split: logo on the left, tabs across the rest.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(12), Constraint::Min(0)])
        .split(area.inner(Margin { vertical: 0, horizontal: 1 }));

    let logo = Paragraph::new(Line::from(vec![
        Span::styled(
            "◆ ",
            Style::default()
                .fg(Color::Rgb(0x00, 0xd4, 0xff))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "chirp",
            Style::default()
                .fg(Color::Rgb(0xff, 0xff, 0xff))
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .style(Style::default().bg(COLOR_BG_TAB));
    frame.render_widget(logo, chunks[0]);

    let titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| Line::from(Span::raw(t.label())))
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.active_tab.index())
        .divider(" ")
        .style(Style::default().fg(Color::Rgb(0xaa, 0xaa, 0xaa)).bg(COLOR_BG_TAB))
        .highlight_style(
            Style::default()
                .fg(Color::Rgb(0xff, 0xff, 0xff))
                .bg(COLOR_BG_TAB)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
    frame.render_widget(tabs, chunks[1]);
}

/// Render the feed (full-width post cards) into `area`.
fn draw_feed(frame: &mut Frame, area: Rect, app: &App) {
    // Background fill.
    frame.render_widget(
        Block::default().style(Style::default().bg(COLOR_BG)),
        area,
    );

    // Inset the cards a bit so they don't touch the terminal edges.
    let inset = area.inner(Margin { vertical: 1, horizontal: 2 });
    if inset.width < 8 || inset.height < 4 {
        return;
    }

    // Lay out card rectangles top-to-bottom with a 1-line gap between cards.
    let mut y = inset.y;
    let card_inner_width = inset.width;
    for (idx, post) in app.posts.iter().enumerate().skip(app.scroll_top) {
        let height = post_card_height(post, card_inner_width);
        if y + height > inset.y + inset.height {
            break;
        }
        let rect = Rect {
            x: inset.x,
            y,
            width: inset.width,
            height,
        };
        draw_post_card(frame, rect, post, idx == app.selected, app);
        y += height + 1; // 1-line vertical gutter
    }
}

fn draw_post_card(frame: &mut Frame, area: Rect, post: &Post, selected: bool, app: &App) {
    let border_color = if selected { COLOR_BORDER_SEL } else { COLOR_BORDER_DIM };
    let border_style = if selected {
        Style::default().fg(border_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(border_color)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(COLOR_BG));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Build the body lines.
    let author_style = Style::default()
        .fg(app.author_color(post.author))
        .add_modifier(Modifier::BOLD);

    // Header line: author on the left, timestamp right-aligned.
    let header_rect = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let header_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(post.timestamp.len() as u16)])
        .split(header_rect);
    frame.render_widget(
        Paragraph::new(Span::styled(post.author, author_style)).style(Style::default().bg(COLOR_BG)),
        header_split[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            post.timestamp,
            Style::default().fg(COLOR_TIMESTAMP_DIM),
        ))
        .alignment(Alignment::Right)
        .style(Style::default().bg(COLOR_BG)),
        header_split[1],
    );

    // npub line
    if inner.height > 1 {
        let npub_rect = Rect { x: inner.x, y: inner.y + 1, width: inner.width, height: 1 };
        frame.render_widget(
            Paragraph::new(Span::styled(
                post.npub,
                Style::default().fg(COLOR_NPUB_DIM),
            ))
            .style(Style::default().bg(COLOR_BG)),
            npub_rect,
        );
    }

    // Body — leave 1 blank line, then wrap.
    if inner.height > 3 {
        let body_y = inner.y + 3;
        let avail_height = inner.height.saturating_sub(4); // header+npub+blank above, reactions below
        let body_height = avail_height.saturating_sub(2); // 1 blank + 1 reactions line
        if body_height > 0 {
            let body_rect = Rect {
                x: inner.x,
                y: body_y,
                width: inner.width,
                height: body_height,
            };
            let body = Paragraph::new(post.body)
                .style(Style::default().fg(COLOR_BODY).bg(COLOR_BG))
                .wrap(Wrap { trim: true });
            frame.render_widget(body, body_rect);
        }
    }

    // Reactions row — pinned to last interior line.
    if inner.height >= 2 {
        let reactions_y = inner.y + inner.height - 1;
        let reactions_rect = Rect {
            x: inner.x,
            y: reactions_y,
            width: inner.width,
            height: 1,
        };
        let sats_str = fmt_sats(post.zap_sats);
        let line = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "♥",
                Style::default().fg(COLOR_HEART).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("{}", post.hearts), Style::default().fg(COLOR_BODY)),
            Span::raw("    "),
            Span::styled(
                "⚡",
                Style::default().fg(COLOR_ZAP).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} sats", sats_str),
                Style::default().fg(COLOR_BODY),
            ),
            Span::raw("    "),
            Span::styled(
                "↺",
                Style::default().fg(COLOR_REPOST).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("{}", post.reposts), Style::default().fg(COLOR_BODY)),
            Span::raw("    "),
            Span::styled(
                "💬",
                Style::default().fg(COLOR_REPLY).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("{}", post.replies), Style::default().fg(COLOR_BODY)),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::default().bg(COLOR_BG)),
            reactions_rect,
        );
    }
}

/// Bottom status bar.
fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(COLOR_BG_TAB)),
        area,
    );

    let mut spans = vec![Span::raw("  ")];
    for r in &app.relays {
        spans.push(Span::styled(
            "●",
            Style::default()
                .fg(if r.connected { COLOR_RELAY_OK } else { COLOR_RELAY_BAD })
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
    let connected_n = app.relays.iter().filter(|r| r.connected).count();
    spans.push(Span::styled(
        format!(" {}/{} relays", connected_n, app.relays.len()),
        Style::default().fg(COLOR_BODY),
    ));
    // If any relay is disconnected, surface its short host so the URL field is
    // user-visible (and the field is referenced — silences dead_code).
    if let Some(down) = app.relays.iter().find(|r| !r.connected) {
        let host = down
            .url
            .trim_start_matches("wss://")
            .trim_start_matches("ws://");
        spans.push(Span::styled(
            format!(" (down: {})", host),
            Style::default().fg(COLOR_RELAY_BAD),
        ));
    }
    spans.push(Span::styled(" │ ", Style::default().fg(COLOR_FOOTER_SEP)));
    spans.push(Span::styled(
        "j/k",
        Style::default().fg(COLOR_BODY).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        ":scroll  ",
        Style::default().fg(COLOR_FOOTER_HINT),
    ));
    spans.push(Span::styled(
        "enter",
        Style::default().fg(COLOR_BODY).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        ":thread  ",
        Style::default().fg(COLOR_FOOTER_HINT),
    ));
    spans.push(Span::styled(
        "d",
        Style::default().fg(COLOR_BODY).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        ":DMs  ",
        Style::default().fg(COLOR_FOOTER_HINT),
    ));
    spans.push(Span::styled(
        "q",
        Style::default().fg(COLOR_BODY).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        ":quit",
        Style::default().fg(COLOR_FOOTER_HINT),
    ));

    let line = Line::from(spans);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(COLOR_BG_TAB)),
        area,
    );
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

/// Apply `Modifier::DIM` to every cell in `area` of the current frame buffer.
/// This produces the "background dimmed while overlay open" effect.
fn dim_area(buf: &mut Buffer, area: Rect) {
    // Snapshot the buffer extent up-front so we can hold a mutable borrow
    // across the iteration without re-borrowing `buf` immutably.
    let buf_area = *buf.area();
    let y_end = area
        .y
        .saturating_add(area.height)
        .min(buf_area.y + buf_area.height);
    let x_end = area
        .x
        .saturating_add(area.width)
        .min(buf_area.x + buf_area.width);
    for y in area.y..y_end {
        for x in area.x..x_end {
            let cell = &mut buf[(x, y)];
            cell.modifier.insert(Modifier::DIM);
        }
    }
}

/// Centered rectangle of `width`x`height` cells inside `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn draw_thread_overlay(frame: &mut Frame, area: Rect, post: &Post, app: &App) {
    // Sized to ~75% of the screen, centered.
    let target_w = (area.width * 75 / 100).max(40);
    // Height grows with reply count but is capped to fit on screen.
    let mut target_h: u16 = 8; // borders + title + root post (5 lines)
    for (lvl, _, _, body) in post.thread {
        let inner_w = target_w.saturating_sub(4 + 2 * (*lvl as u16));
        // 1 header line + wrap_count(body) + 1 trailing gap
        target_h = target_h.saturating_add(1 + wrap_count(body, inner_w as usize) as u16 + 1);
    }
    target_h = target_h.min(area.height.saturating_sub(2)).max(10);
    let rect = centered_rect(target_w, target_h, area);

    // Dim the area BEHIND the overlay (everything in `area`) first.
    dim_area(frame.buffer_mut(), area);

    // Clear the overlay region so the dim modifier doesn't bleed through
    // into the overlay cells.
    frame.render_widget(Clear, rect);

    // Overlay panel block.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(COLOR_OVERLAY_BORDER).add_modifier(Modifier::BOLD))
        .title(Line::from(vec![
            Span::styled(
                " Thread ",
                Style::default()
                    .fg(COLOR_OVERLAY_TITLE)
                    .bg(COLOR_OVERLAY_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_alignment(Alignment::Left)
        .style(Style::default().bg(COLOR_OVERLAY_BG));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    // Render a ✕ close marker on the top-right interior cell.
    if rect.width >= 5 {
        let close_rect = Rect {
            x: rect.x + rect.width - 4,
            y: rect.y,
            width: 3,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                " ✕ ",
                Style::default()
                    .fg(Color::Rgb(0xff, 0x88, 0x88))
                    .bg(COLOR_OVERLAY_BG)
                    .add_modifier(Modifier::BOLD),
            )),
            close_rect,
        );
    }

    if inner.width < 6 || inner.height < 4 {
        return;
    }

    // --- Root post inside the overlay ---
    let mut cur_y = inner.y;

    let author_style = Style::default()
        .fg(app.author_color(post.author))
        .add_modifier(Modifier::BOLD);
    let header = Line::from(vec![
        Span::styled(post.author, author_style),
        Span::styled(
            format!(" · {}", short_time(post.timestamp)),
            Style::default().fg(COLOR_TIMESTAMP_DIM),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(header).style(Style::default().bg(COLOR_OVERLAY_BG)),
        Rect { x: inner.x, y: cur_y, width: inner.width, height: 1 },
    );
    cur_y += 1;

    let body_lines = wrap_count(post.body, inner.width as usize) as u16;
    let body_h = body_lines.min(inner.y + inner.height - cur_y);
    if body_h > 0 {
        frame.render_widget(
            Paragraph::new(post.body)
                .style(Style::default().fg(COLOR_BODY).bg(COLOR_OVERLAY_BG))
                .wrap(Wrap { trim: true }),
            Rect { x: inner.x, y: cur_y, width: inner.width, height: body_h },
        );
        cur_y += body_h;
    }

    // Reactions for the root post.
    if cur_y < inner.y + inner.height {
        let sats_str = fmt_sats(post.zap_sats);
        let reactions = Line::from(vec![
            Span::styled("♥ ", Style::default().fg(COLOR_HEART).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", post.hearts), Style::default().fg(COLOR_BODY)),
            Span::styled("⚡ ", Style::default().fg(COLOR_ZAP).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", sats_str), Style::default().fg(COLOR_BODY)),
            Span::styled("↺ ", Style::default().fg(COLOR_REPOST).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", post.reposts), Style::default().fg(COLOR_BODY)),
            Span::styled("💬 ", Style::default().fg(COLOR_REPLY).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}", post.replies), Style::default().fg(COLOR_BODY)),
        ]);
        frame.render_widget(
            Paragraph::new(reactions).style(Style::default().bg(COLOR_OVERLAY_BG)),
            Rect { x: inner.x, y: cur_y, width: inner.width, height: 1 },
        );
        cur_y += 1;
    }

    // Separator line under the root post.
    if cur_y < inner.y + inner.height {
        let sep = "─".repeat(inner.width as usize);
        frame.render_widget(
            Paragraph::new(Span::styled(sep, Style::default().fg(COLOR_BORDER_DIM)))
                .style(Style::default().bg(COLOR_OVERLAY_BG)),
            Rect { x: inner.x, y: cur_y, width: inner.width, height: 1 },
        );
        cur_y += 1;
    }

    // --- Replies, indented per level ---
    for (lvl, author, time, body) in post.thread {
        if cur_y >= inner.y + inner.height {
            break;
        }
        let indent = 2 * (*lvl as u16);
        let prefix_x = inner.x + indent;
        if prefix_x >= inner.x + inner.width {
            break;
        }
        let avail_w = (inner.x + inner.width) - prefix_x;

        let line = Line::from(vec![
            Span::styled(
                "↳ ",
                Style::default()
                    .fg(COLOR_OVERLAY_REPLY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                *author,
                Style::default()
                    .fg(app.author_color(author))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {}", time),
                Style::default().fg(COLOR_TIMESTAMP_DIM),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::default().bg(COLOR_OVERLAY_BG)),
            Rect { x: prefix_x, y: cur_y, width: avail_w, height: 1 },
        );
        cur_y += 1;

        // Body indented an extra 2 columns under the ↳ marker.
        let body_x = prefix_x + 2;
        if body_x >= inner.x + inner.width || cur_y >= inner.y + inner.height {
            continue;
        }
        let body_w = (inner.x + inner.width) - body_x;
        let body_h = (wrap_count(body, body_w as usize) as u16)
            .min(inner.y + inner.height - cur_y);
        if body_h > 0 {
            frame.render_widget(
                Paragraph::new(*body)
                    .style(Style::default().fg(COLOR_BODY).bg(COLOR_OVERLAY_BG))
                    .wrap(Wrap { trim: true }),
                Rect { x: body_x, y: cur_y, width: body_w, height: body_h },
            );
            cur_y += body_h;
        }

        // Blank line between replies if there's room.
        if cur_y < inner.y + inner.height {
            cur_y += 1;
        }
    }
}

fn short_time(t: &str) -> String {
    // Compress "2 minutes ago" -> "2m", "1 hour ago" -> "1h", etc.
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() >= 2 {
        let num = parts[0];
        let unit = parts[1];
        let suffix = if unit.starts_with("minute") {
            "m"
        } else if unit.starts_with("hour") {
            "h"
        } else if unit.starts_with("second") {
            "s"
        } else if unit.starts_with("day") {
            "d"
        } else {
            ""
        };
        if !suffix.is_empty() {
            return format!("{}{}", num, suffix);
        }
    }
    t.to_string()
}

fn draw_dms_overlay(frame: &mut Frame, area: Rect, app: &App) {
    // Slide-in panel: ~45% of width, anchored to the right edge.
    let panel_w = (area.width * 45 / 100).max(36);
    let panel_h = area.height.saturating_sub(2).max(12);
    let panel = Rect {
        // saturating_sub guards against terminals narrower than `panel_w`;
        // without the +1 inside the sub we'd underflow u16 on tiny widths.
        x: area.x + area.width.saturating_sub(panel_w + 1),
        y: area.y + 1,
        width: panel_w,
        height: panel_h,
    };

    dim_area(frame.buffer_mut(), area);
    frame.render_widget(Clear, panel);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(COLOR_OVERLAY_BORDER).add_modifier(Modifier::BOLD))
        .title(Line::from(Span::styled(
            " Direct Messages ",
            Style::default()
                .fg(COLOR_OVERLAY_TITLE)
                .bg(COLOR_OVERLAY_BG)
                .add_modifier(Modifier::BOLD),
        )))
        .style(Style::default().bg(COLOR_OVERLAY_BG));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);

    // Close ✕ marker in top-right.
    if panel.width >= 5 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                " ✕ ",
                Style::default()
                    .fg(Color::Rgb(0xff, 0x88, 0x88))
                    .bg(COLOR_OVERLAY_BG)
                    .add_modifier(Modifier::BOLD),
            )),
            Rect { x: panel.x + panel.width - 4, y: panel.y, width: 3, height: 1 },
        );
    }

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    // Split inner area: conversation list on left, message thread on right.
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(inner.width / 2), Constraint::Min(0)])
        .split(inner);
    let list_area = split[0];
    let thread_area = split[1];

    // Vertical divider between halves.
    if thread_area.height > 0 {
        for y in thread_area.y..thread_area.y + thread_area.height {
            let cell = &mut frame.buffer_mut()[(thread_area.x, y)];
            cell.set_char('│');
            cell.set_style(Style::default().fg(COLOR_OVERLAY_BORDER).bg(COLOR_OVERLAY_BG));
        }
    }

    // --- Conversation list ---
    // Each conversation occupies 3 rows: author+unread, preview, blank.
    const DM_ROW_H: u16 = 3;
    for (i, dm) in app.dms.iter().enumerate() {
        let row_y = list_area.y + (i as u16) * DM_ROW_H;
        if row_y + 1 > list_area.y + list_area.height {
            break;
        }
        let is_sel = i == app.dm_selected;
        let bg = if is_sel {
            Color::Rgb(0x2a, 0x2a, 0x44)
        } else {
            COLOR_OVERLAY_BG
        };

        // Fill the row background (covers 2 active lines).
        let block_h = (DM_ROW_H - 1).min(list_area.y + list_area.height - row_y);
        let block_rect = Rect {
            x: list_area.x,
            y: row_y,
            width: list_area.width,
            height: block_h,
        };
        frame.render_widget(
            Block::default().style(Style::default().bg(bg)),
            block_rect,
        );

        // Line 1: author + unread badge.
        let mut spans: Vec<Span> = vec![
            Span::styled(
                if is_sel { "▸ " } else { "  " },
                Style::default().fg(COLOR_OVERLAY_BORDER).bg(bg),
            ),
            Span::styled(
                dm.author,
                Style::default()
                    .fg(app.author_color(dm.author))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if dm.unread > 0 {
            spans.push(Span::styled(
                format!("  {}●", dm.unread),
                Style::default()
                    .fg(COLOR_HEART)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(bg)),
            Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height: 1,
            },
        );

        // Line 2: dim preview text, indented under the author name.
        if block_h >= 2 {
            // Truncate preview to fit on one line.
            let avail = list_area.width.saturating_sub(4) as usize;
            let preview = if dm.preview.chars().count() > avail {
                let mut s: String = dm.preview.chars().take(avail.saturating_sub(1)).collect();
                s.push('…');
                s
            } else {
                dm.preview.to_string()
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    preview,
                    Style::default().fg(COLOR_NPUB_DIM).bg(bg),
                ))
                .style(Style::default().bg(bg)),
                Rect {
                    x: list_area.x + 2,
                    y: row_y + 1,
                    width: list_area.width.saturating_sub(2),
                    height: 1,
                },
            );
        }
    }

    // --- Selected conversation transcript ---
    if let Some(dm) = app.dms.get(app.dm_selected) {
        // Title line.
        let title = Line::from(vec![
            Span::styled(
                dm.author,
                Style::default()
                    .fg(app.author_color(dm.author))
                    .bg(COLOR_OVERLAY_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(title).style(Style::default().bg(COLOR_OVERLAY_BG)),
            Rect {
                x: thread_area.x + 2,
                y: thread_area.y,
                width: thread_area.width.saturating_sub(2),
                height: 1,
            },
        );
        // Separator under title.
        let sep_w = thread_area.width.saturating_sub(2) as usize;
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(sep_w),
                Style::default().fg(COLOR_BORDER_DIM).bg(COLOR_OVERLAY_BG),
            ))
            .style(Style::default().bg(COLOR_OVERLAY_BG)),
            Rect {
                x: thread_area.x + 2,
                y: thread_area.y + 1,
                width: thread_area.width.saturating_sub(2),
                height: 1,
            },
        );

        let mut y = thread_area.y + 2;
        let body_w = thread_area.width.saturating_sub(4);
        for (sender, body) in dm.messages {
            if y >= thread_area.y + thread_area.height {
                break;
            }
            let outbound = *sender == "You";
            let sender_style = Style::default()
                .fg(if outbound {
                    Color::Rgb(0xff, 0xff, 0xff)
                } else {
                    app.author_color(sender)
                })
                .bg(COLOR_OVERLAY_BG)
                .add_modifier(Modifier::BOLD);
            frame.render_widget(
                Paragraph::new(Span::styled(*sender, sender_style))
                    .style(Style::default().bg(COLOR_OVERLAY_BG)),
                Rect { x: thread_area.x + 2, y, width: body_w, height: 1 },
            );
            y += 1;
            if y >= thread_area.y + thread_area.height {
                break;
            }
            let lines = wrap_count(body, body_w as usize) as u16;
            let h = lines.min(thread_area.y + thread_area.height - y);
            frame.render_widget(
                Paragraph::new(*body)
                    .style(Style::default().fg(COLOR_BODY).bg(COLOR_OVERLAY_BG))
                    .wrap(Wrap { trim: true }),
                Rect { x: thread_area.x + 2, y, width: body_w, height: h },
            );
            y += h + 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level draw + main loop
// ---------------------------------------------------------------------------

fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Paint full background first so resize gaps don't show terminal default.
    Block::default()
        .style(Style::default().bg(COLOR_BG))
        .render(size, frame.buffer_mut());

    // Vertical: tab bar (2), separator (1), feed (rest), separator (1), footer (1).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(size);

    draw_tab_bar(frame, chunks[0], app);
    draw_separator(frame, chunks[1]);
    draw_feed(frame, chunks[2], app);
    draw_separator(frame, chunks[3]);
    draw_footer(frame, chunks[4], app);

    // Overlays — rendered on top of the now-complete base layout.
    match app.overlay {
        Overlay::None => {}
        Overlay::Thread(idx) => {
            if let Some(post) = app.posts.get(idx) {
                draw_thread_overlay(frame, size, post, app);
            }
        }
        Overlay::Dms => {
            draw_dms_overlay(frame, size, app);
        }
    }
}

fn draw_separator(frame: &mut Frame, area: Rect) {
    let line = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(
            line,
            Style::default().fg(COLOR_BORDER_DIM),
        ))
        .style(Style::default().bg(COLOR_BG)),
        area,
    );
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let mut app = App::new();
    loop {
        terminal.draw(|f| draw(f, &app))?;

        // Poll with a short timeout so we stay responsive to resize events.
        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.on_key(key.code);
                }
                Event::Resize(_, _) => {
                    // ratatui handles the resize on the next draw; nothing to do.
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    // Setup terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app, ensuring cleanup even on error.
    let result = run(&mut terminal);

    // Restore terminal.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
