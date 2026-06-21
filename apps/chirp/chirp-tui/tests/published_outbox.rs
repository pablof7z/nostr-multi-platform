use chirp_tui::app::{AppState, OutboxSelection};
use chirp_tui::feature_snapshot::FeatureSnapshot;
use chirp_tui::features::FeatureTab;
use chirp_tui::ui::layout::render;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn publish_history_keeps_retry_decision_and_failure_detail() {
    let snapshot = FeatureSnapshot::from_json_fixture(
        r#"{
          "projections": {
            "publish_queue": [
              {
                "event_id": "aaaa",
                "kind": 1,
                "title": "Note",
                "status": "accepted_locally",
                "can_retry": false
              },
              {
                "event_id": "bbbb",
                "kind": 7,
                "title": "Reaction",
                "status": "failed",
                "can_retry": true,
                "relay_outcomes": [
                  {
                    "relay_url": "wss://relay.example",
                    "status": "failed",
                    "relay_reason": "nip65_write",
                    "message": "blocked: spam"
                  }
                ]
              }
            ]
          }
        }"#,
    );

    assert_eq!(snapshot.history.len(), 1);
    let row = &snapshot.history[0];
    assert_eq!(row.event_id, "bbbb");
    assert_eq!(row.kind, 7);
    assert!(row.can_retry);
    assert_eq!(row.relays[0].message, "blocked: spam");
}

#[test]
fn outbox_selection_survives_active_to_history_transition() {
    let mut state = AppState {
        features: FeatureSnapshot::from_json_fixture(
            r#"{
              "projections": {
                "publish_outbox": [{ "handle": "aaaa", "title": "Note" }]
              }
            }"#,
        ),
        outbox_selected: Some(OutboxSelection::Active(0)),
        ..AppState::default()
    };

    state.features = FeatureSnapshot::from_json_fixture(
        r#"{
          "projections": {
            "publish_queue": [
              {
                "event_id": "aaaa",
                "kind": 1,
                "title": "Note",
                "status": "failed",
                "can_retry": true
              }
            ]
          }
        }"#,
    );
    state.clamp_outbox_selection();

    assert_eq!(state.outbox_selected, Some(OutboxSelection::History(0)));
}

#[test]
fn published_history_selection_renders_detail_without_live_relay() {
    let features = FeatureSnapshot::from_json_fixture(
        r#"{
          "projections": {
            "accounts": [
              {
                "id": "account-1",
                "display_name": "Tester",
                "npub": "npub1fixture",
                "signer_kind": "local",
                "is_active": true
              }
            ],
            "publish_queue": [
              {
                "event_id": "evtpub1",
                "kind": 1,
                "title": "Note",
                "status": "ok",
                "can_retry": false,
                "relay_outcomes": [
                  {
                    "relay_url": "wss://relay.fixture",
                    "status": "ok",
                    "relay_reason": "nip65_write",
                    "message": "accepted"
                  }
                ]
              }
            ]
          }
        }"#,
    );
    assert_eq!(features.history.len(), 1);

    let mut state = AppState {
        features,
        outbox_selected: Some(OutboxSelection::History(0)),
        ..AppState::default()
    };
    state.set_tab(FeatureTab::Settings);
    state.settings_cursor = 2;

    let rendered = render_state(240, 40, state);

    assert!(rendered.contains("Published"));
    assert!(rendered.contains("Published Detail"));
    assert!(rendered.contains("event"));
    assert!(rendered.contains("evtpub1"));
    assert!(rendered.contains("kind"));
    assert!(rendered.contains("Note (1)"));
    assert!(rendered.contains("action"));
    assert!(rendered.contains("d clear"));
    assert!(rendered.contains("relay.fixture"));
    assert!(rendered.contains("accepted"));
}

fn render_state(width: u16, height: u16, state: AppState) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &state)).unwrap();
    format!("{:?}", terminal.backend().buffer())
}
