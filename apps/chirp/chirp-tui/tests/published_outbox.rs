use chirp_tui::app::{AppState, OutboxSelection};
use chirp_tui::feature_snapshot::FeatureSnapshot;

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
                    "relay_reason": "NIP-65 write relay",
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
