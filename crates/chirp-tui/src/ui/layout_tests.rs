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
    });
    state
}
