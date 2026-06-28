use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::AppState;
use crate::feature_snapshot::{AccountLine, RelayEditLine};
use crate::features::FeatureTab;
use crate::snapshot::{RelayRow, RelayWireSubRow};
use crate::timeline::TimelineRow;
use crate::ui::layout::render;
use crate::ui::nostr_user::profile_wire::ProfileWire;

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
    assert!(rendered.contains("Shift+Enter"));
    assert!(rendered.contains("open"));
}

#[test]
fn modal_form_renders_as_centered_overlay() {
    let mut state = state_with_account();
    state.start_modal(
        "Create group",
        vec![
            "Protocol (nip29/mls)",
            "Name",
            "Relay(s)",
            "NIP-29 local id",
            "MLS invitees",
        ],
        "create-group",
    );

    let rendered = render_state(120, 40, state);

    assert!(rendered.contains("Create group"));
    assert!(rendered.contains("Protocol"));
    assert!(rendered.contains("MLS invitees"));
    assert!(rendered.contains("Shift+Tab"));
}

#[test]
fn home_tab_handles_narrow_terminal_without_panicking() {
    let mut state = state_with_account();
    state.status = "narrow terminal smoke test".to_string();
    let author_pubkey = "a".repeat(64);
    state.rows.push(TimelineRow {
        id: "event-with-display-counts".to_string(),
        author_pubkey: author_pubkey.clone(),
        author_profile: profile(
            &author_pubkey,
            "alexandria-cassandra-with-a-very-long-kind0-display-name",
        ),
        content: "reply 0 repost 0 like 0 -- this content should wrap inside the feed pane"
            .to_string(),
        created_at: 1,
        depth: 0,
        has_gap: false,
        thread_attribution: Vec::new(),
        relation_counts: Default::default(),
        relay_provenance: Vec::new(),
        content_tree: None,
        content_render: Default::default(),
        mention_pubkeys: Vec::new(),
        repost: None,
    });

    let rendered = render_state(80, 24, state);

    assert!(rendered.contains("chirp"));
    assert!(rendered.contains("Compose"));
}

#[test]
fn settings_tab_renders_all_relay_inventory_and_raw_filters() {
    let mut state = state_with_account();
    state.set_tab(FeatureTab::Settings);
    state.features.configured_relays.push(RelayEditLine {
        url: "wss://relay.example".to_string(),
        role: "both,indexer".to_string(),
    });
    state.relays.push(RelayRow {
        relay_url: "wss://relay.example".to_string(),
        role: "content".to_string(),
        connection: "connected".to_string(),
        total_sub_count: 1,
        active_sub_count: 1,
        total_events_rx: 12,
        wire_subs: vec![RelayWireSubRow {
            wire_id: "sub-feed".to_string(),
            filter_summary: r#"{"kinds":[1],"limit":20}"#.to_string(),
            state: "open".to_string(),
            consumer_count: 1,
            opened_ms: 1_700_000_000_000,
            events_rx: 12,
            ..Default::default()
        }],
        ..Default::default()
    });

    let rendered = render_state(150, 40, state);

    assert!(rendered.contains("All Relays 1"));
    assert!(rendered.contains("relay.example"));
    assert!(rendered.contains("Both + Index"));
    assert!(rendered.contains("raw"));
    assert!(rendered.contains("\"kinds\""));
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
        author_pubkey: "alice-pubkey".to_string(),
        author_profile: profile("alice-pubkey", "alice"),
        content: "hello from nostr".to_string(),
        created_at: 1,
        depth: 0,
        has_gap: false,
        thread_attribution: Vec::new(),
        relation_counts: Default::default(),
        relay_provenance: Vec::new(),
        content_tree: None,
        content_render: Default::default(),
        mention_pubkeys: Vec::new(),
        repost: None,
    });
    state
}

fn profile(pubkey: &str, display_name: &str) -> ProfileWire {
    ProfileWire {
        pubkey: pubkey.to_string(),
        display_name: Some(display_name.to_string()),
        about: None,
        picture_url: None,
        nip05: None,
        npub: pubkey.to_string(),
        npub_short: pubkey.to_string(),
    }
}
