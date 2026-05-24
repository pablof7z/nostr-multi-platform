use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::AppState;
use crate::timeline::TimelineRow;
use crate::ui::layout::render;

#[test]
fn renders_three_pane_skeleton_at_120_by_40() {
    let rendered = render_state(120, 40, AppState::default());

    assert!(rendered.contains("chirp"));
    assert!(rendered.contains("Feed"));
    assert!(rendered.contains("Detail"));
    assert!(rendered.contains("Profile"));
    assert!(rendered.contains("Compose"));
}

#[test]
fn renders_feed_rows_from_state() {
    let rendered = render_state(120, 40, state_with_row());

    assert!(rendered.contains("items: 1/1"));
    assert!(rendered.contains("alice"));
    assert!(rendered.contains("hello from nostr"));
}

#[test]
fn basic_mode_collapses_to_feed_only_body() {
    let mut state = state_with_row();
    state.set_basic();

    let rendered = render_state(120, 40, state);

    assert!(rendered.contains("[basic]"));
    assert!(rendered.contains("Feed"));
    assert!(!rendered.contains("Detail"));
    assert!(!rendered.contains("Profile"));
}

#[test]
fn medium_width_hides_profile_before_detail() {
    let rendered = render_state(90, 30, AppState::default());

    assert!(rendered.contains("Feed"));
    assert!(rendered.contains("Detail"));
    assert!(!rendered.contains("Profile"));
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
fn min_terminal_renders_long_names_and_count_text_without_overflow() {
    let mut state = AppState::default();
    state.status =
        "loaded author display names and relation counts for a deliberately narrow terminal"
            .to_string();
    state.rows.push(TimelineRow {
        id: "event-with-display-counts".to_string(),
        author: "alexandria-cassandra-with-a-very-long-kind0-display-name".to_string(),
        author_pubkey: "a".repeat(64),
        content: "reply 0 repost 0 like 0 -- this content should wrap inside the feed pane"
            .to_string(),
        created_at: 1,
        depth: 3,
        has_gap: true,
        relation_counts: Default::default(),
    });

    let rendered = render_state(80, 24, state);

    assert!(rendered.contains("chirp"));
    assert!(rendered.contains("Feed"));
    assert!(rendered.contains("Compose"));
    assert!(rendered.contains("reply"));
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
        created_at: 1,
        depth: 0,
        has_gap: false,
        relation_counts: Default::default(),
    });
    state
}
