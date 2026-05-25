use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::AppState;
use crate::timeline::TimelineRow;
use crate::timeline_content::{TimelineMedia, TimelineMediaKind};
use crate::ui::layout::render;

#[test]
fn home_tab_renders_chrome_and_compose_at_120_by_40() {
    let rendered = render_state(120, 40, AppState::default());

    // Title bar still shows the app name.
    assert!(rendered.contains("chirp"));
    // Compose bar still rendered below the body.
    assert!(rendered.contains("Compose"));
}

#[test]
fn home_tab_renders_post_author_and_content() {
    let rendered = render_state(120, 40, state_with_row());

    // Author and body of the depth-0 post are rendered by post_list/post_detail.
    assert!(rendered.contains("alice"));
    assert!(rendered.contains("hello from nostr"));
}

#[test]
fn home_tab_renders_media_attachment_labels() {
    let mut state = state_with_row();
    state.rows[0].media.push(TimelineMedia {
        kind: TimelineMediaKind::Image,
        url: "https://example.com/photo.jpg".to_string(),
        alt: Some("album art".to_string()),
    });

    let rendered = render_state(120, 40, state);

    assert!(rendered.contains("[image]"));
    assert!(rendered.contains("album art"));
}

#[test]
fn detail_pane_wraps_full_reply_content_without_preview_truncation() {
    let mut state = state_with_row();
    state.rows.push(TimelineRow {
        id: "reply-1".to_string(),
        author: "bob".to_string(),
        author_pubkey: "bob-pubkey".to_string(),
        content: concat!(
            "Five flights, one boat ride and two days later, I am back in my island ",
            "after enough leading text to exceed the old preview cutoff ",
            "https://24242.io/c5371b-full-url-tail-token"
        )
        .to_string(),
        media: Vec::new(),
        created_at: 2,
        depth: 1,
        has_gap: false,
        relation_counts: Default::default(),
        mention_pubkeys: Vec::new(),
    });

    let rendered = render_state(160, 50, state);

    assert!(rendered.contains("c5371b-full-url-tail-token"));
    assert!(!rendered.contains("preview cutoff ..."));
}

#[test]
fn help_overlay_renders_keybindings() {
    let mut state = AppState::default();
    state.toggle_help();

    let rendered = render_state(120, 40, state);

    assert!(rendered.contains("Help"));
    assert!(rendered.contains("Ctrl+Enter"));
    assert!(rendered.contains("open selected thread"));
}

#[test]
fn home_tab_handles_narrow_terminal_without_panicking() {
    let mut state = AppState::default();
    state.status = "narrow terminal smoke test".to_string();
    state.rows.push(TimelineRow {
        id: "event-with-display-counts".to_string(),
        author: "alexandria-cassandra-with-a-very-long-kind0-display-name".to_string(),
        author_pubkey: "a".repeat(64),
        content: "reply 0 repost 0 like 0 -- this content should wrap inside the feed pane"
            .to_string(),
        media: Vec::new(),
        created_at: 1,
        depth: 0,
        has_gap: false,
        relation_counts: Default::default(),
        mention_pubkeys: Vec::new(),
    });

    let rendered = render_state(80, 24, state);

    // App chrome still visible at narrow widths.
    assert!(rendered.contains("chirp"));
    assert!(rendered.contains("Compose"));
}

fn render_state(width: u16, height: u16, state: AppState) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &state)).unwrap();
    format!("{:?}", terminal.backend().buffer())
}

fn state_with_row() -> AppState {
    let mut state = AppState::default();
    state.rows.push(TimelineRow {
        id: "event-1".to_string(),
        author: "alice".to_string(),
        author_pubkey: "alice-pubkey".to_string(),
        content: "hello from nostr".to_string(),
        media: Vec::new(),
        created_at: 1,
        depth: 0,
        has_gap: false,
        relation_counts: Default::default(),
        mention_pubkeys: Vec::new(),
    });
    state
}
