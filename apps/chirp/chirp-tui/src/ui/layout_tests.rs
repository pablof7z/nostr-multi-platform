use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::AppState;
use crate::feature_snapshot::AccountLine;
use crate::timeline::TimelineRow;
use crate::ui::layout::render;

#[test]
fn home_tab_renders_chrome_and_compose_at_120_by_40() {
    let rendered = render_state(120, 40, state_with_account());

    assert!(rendered.contains("chirp"));
    assert!(rendered.contains("Compose"));
}

#[test]
fn home_tab_renders_post_author_and_content() {
    let rendered = render_state(120, 40, state_with_row());

    assert!(rendered.contains("alice"));
    assert!(rendered.contains("hello from nostr"));
}

#[test]
fn help_overlay_renders_keybindings() {
    let mut state = state_with_account();
    state.toggle_help();

    let rendered = render_state(120, 40, state);

    assert!(rendered.contains("Help"));
    assert!(rendered.contains("Ctrl+Enter"));
    assert!(rendered.contains("open"));
}

#[test]
fn home_tab_handles_narrow_terminal_without_panicking() {
    let mut state = state_with_account();
    state.status = "narrow terminal smoke test".to_string();
    state.rows.push(TimelineRow {
        id: "event-with-display-counts".to_string(),
        author: "alexandria-cassandra-with-a-very-long-kind0-display-name".to_string(),
        author_pubkey: "a".repeat(64),
        content: "reply 0 repost 0 like 0 -- this content should wrap inside the feed pane"
            .to_string(),
        created_at: 1,
        depth: 0,
        has_gap: false,
        relation_counts: Default::default(),
        mention_pubkeys: Vec::new(),
    });

    let rendered = render_state(80, 24, state);

    assert!(rendered.contains("chirp"));
    assert!(rendered.contains("Compose"));
}

fn render_state(width: u16, height: u16, state: AppState) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &state)).unwrap();
    format!("{:?}", terminal.backend().buffer())
}

fn state_with_account() -> AppState {
    let mut state = AppState::default();
    state.features.accounts.push(AccountLine {
        id: "test-id".to_string(),
        display: "tester".to_string(),
        npub: "npub1testtesttesttesttesttesttesttesttesttesttesttesttesttesttest".to_string(),
        signer: "nsec".to_string(),
        active: true,
    });
    state.features.active_account = "tester".to_string();
    state
}

fn state_with_row() -> AppState {
    let mut state = state_with_account();
    state.rows.push(TimelineRow {
        id: "event-1".to_string(),
        author: "alice".to_string(),
        author_pubkey: "alice-pubkey".to_string(),
        content: "hello from nostr".to_string(),
        created_at: 1,
        depth: 0,
        has_gap: false,
        relation_counts: Default::default(),
        mention_pubkeys: Vec::new(),
    });
    state
}
