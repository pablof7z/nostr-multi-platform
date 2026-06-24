use super::*;
use crate::time::Instant;

#[test]
fn event_to_unix_ms_conversions() {
    // 0 event_ms → None (sentinel for "never observed").
    assert_eq!(event_to_unix_ms(1_000_000, 0), None);
    // event 40_000ms after start → anchor + offset.
    assert_eq!(event_to_unix_ms(1_000_000, 40_000), Some(1_040_000));
    // the conversion is a pure function of (anchor, offset): the SAME inputs
    // always map to the SAME output — this determinism is what makes the
    // projection byte-stable across snapshots (no live clock read).
    assert_eq!(
        event_to_unix_ms(1_000_000, 40_000),
        event_to_unix_ms(1_000_000, 40_000)
    );
    // overflow on the u128→u64 narrowing saturates rather than panicking (D6).
    assert_eq!(
        event_to_unix_ms(10, u128::from(u64::MAX) + 5),
        Some(u64::MAX)
    );
}

/// The regression gate this whole change exists for: two consecutive
/// `relay_diagnostics_snapshot()` calls with NO intervening relay event MUST
/// serialize to BYTE-IDENTICAL output. Before the started-at wall-clock anchor
/// (Opus review #4), the builder read `SystemTime::now()` + `Instant::now()`
/// fresh each call, so a fixed event's timestamp jittered ~1ms tick-to-tick and
/// this would be flaky. With the anchor it is deterministic by construction.
#[test]
fn snapshot_is_byte_stable_without_intervening_event() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // `start()` captures the wall-clock anchor; required for the conversion.
    kernel.start();
    kernel.relay_connecting_url(RelayRole::Content, "wss://relay.example/");
    kernel.relay_connected_url(RelayRole::Content, "wss://relay.example/");
    kernel.record_transport_event(RelayRole::Content, "wss://relay.example/", Instant::now());

    let first =
        serde_json::to_vec(&kernel.relay_diagnostics_snapshot()).expect("snapshot serializes");
    // Burn a little wall-clock time to prove the output does NOT track "now".
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second =
        serde_json::to_vec(&kernel.relay_diagnostics_snapshot()).expect("snapshot serializes");

    assert_eq!(
        first, second,
        "relay_diagnostics must serialize byte-identically when no relay \
         event intervened (no clock churn in the projection)"
    );
}

#[test]
fn snapshot_emits_one_row_per_known_relay() {
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed the app's relay set explicitly — production no longer hardcodes a
    // fallback, so the diagnostics snapshot draws its Content/Indexer rows from
    // the configured set the app declares.
    kernel.set_configured_relays(vec![
        crate::kernel::AppRelay::new(
            crate::relay::FALLBACK_CONTENT_RELAY.to_string(),
            "both".to_string(),
        ),
        crate::kernel::AppRelay::new(
            crate::relay::FALLBACK_INDEXER_RELAY.to_string(),
            "indexer".to_string(),
        ),
    ]);
    let snap = kernel.relay_diagnostics_snapshot();
    // Bootstrap roles (Content + Indexer) are always present.
    let roles: Vec<_> = snap.relays.iter().map(|r| r.role.as_str()).collect();
    assert!(
        roles.iter().any(|r| *r == "content" || *r == "both"),
        "expected content/both lane in roles {:?}",
        roles
    );
    assert!(
        roles.iter().any(|r| *r == "indexer"),
        "expected Indexer lane in roles {:?}",
        roles
    );
    // Every relay row has roll-up counters zeroed (no subs yet).
    for row in &snap.relays {
        assert_eq!(row.total_sub_count, 0);
        assert_eq!(row.active_sub_count, 0);
        assert_eq!(row.eosed_sub_count, 0);
        assert_eq!(row.total_events_rx, 0);
    }
    // The interest snapshot includes the always-on lanes.
    assert!(snap.interests.iter().any(|i| i.key == "Timeline"));
    // Every interest carries a non-empty RAW state token (no semantic tone is
    // emitted by core any more — #1768; shells derive their own hue).
    for interest in &snap.interests {
        assert!(!interest.state.is_empty());
    }
}

#[test]
fn snapshot_emits_every_transport_url_for_same_role() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connecting_url(RelayRole::Content, "wss://relay-a.test/");
    kernel.relay_connected_url(RelayRole::Content, "wss://relay-a.test/");
    kernel.relay_connecting_url(RelayRole::Content, "wss://relay-b.test/");
    kernel.relay_connected_url(RelayRole::Content, "wss://relay-b.test/");
    kernel.record_tx_to(RelayRole::Content, "wss://relay-b.test/", 128);

    let snap = kernel.relay_diagnostics_snapshot();
    let relay_a = snap
        .relays
        .iter()
        .find(|row| row.relay_url == "wss://relay-a.test")
        .expect("diagnostics must include the first content socket URL");
    let relay_b = snap
        .relays
        .iter()
        .find(|row| row.relay_url == "wss://relay-b.test")
        .expect("diagnostics must include the second content socket URL");

    assert_eq!(relay_a.role, "content");
    assert_eq!(relay_a.connection, "connected");
    assert_eq!(relay_b.role, "content");
    assert_eq!(relay_b.connection, "connected");
    assert_eq!(relay_b.bytes_tx, 128);
}

/// Ingesting N NOTICEs for a relay must produce `notice_count == N` and a
/// `notices` list capped at 32 entries (oldest dropped), with entries ordered
/// newest-first in the projection.
#[test]
fn notice_count_and_bounded_log() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;
    use std::collections::HashMap;

    let url = "wss://relay-notice.test/";
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connecting_url(RelayRole::Content, url);
    kernel.relay_connected_url(RelayRole::Content, url);

    // Ingest 35 NOTICEs — more than the cap of 32.
    let total: usize = 35;
    for i in 0..total {
        // Synthesise a minimal NOTICE frame through handle_message.
        let frame = crate::kernel::RelayFrame::Text(
            serde_json::json!(["NOTICE", format!("msg {i}")]).to_string(),
        );
        kernel.handle_message(RelayRole::Content, url, frame);
    }

    let snap = kernel.relay_diagnostics_snapshot();
    let row = snap
        .relays
        .iter()
        .find(|r| r.relay_url.starts_with("wss://relay-notice.test"))
        .expect("relay row must be present");

    // Total counter is exact (not capped).
    assert_eq!(row.notice_count, total as u64, "notice_count must equal total NOTICEs received");

    // Ring is capped at 32 (MAX_NOTICE_LOG).
    assert_eq!(row.notices.len(), 32, "notices ring must be capped at 32");

    // Newest-first: the last ingested message ("msg 34") must be first.
    assert_eq!(row.notices[0].text, "msg 34", "notices must be ordered newest-first");
    assert_eq!(row.notices[31].text, "msg 3", "oldest retained entry must be msg 3");

    // last_notice still reflects the most recent notice text.
    assert_eq!(row.last_notice.as_deref(), Some("msg 34"));

    // Build a map to cross-check all 32 retained entries.
    let text_set: HashMap<&str, ()> =
        row.notices.iter().map(|n| (n.text.as_str(), ())).collect();
    for i in 3..=34_usize {
        assert!(
            text_set.contains_key(format!("msg {i}").as_str()),
            "expected msg {i} in retained notices"
        );
    }
    // Msgs 0, 1, 2 must have been evicted (oldest-dropped).
    for i in 0..3_usize {
        assert!(
            !text_set.contains_key(format!("msg {i}").as_str()),
            "msg {i} must have been evicted from the bounded ring"
        );
    }
}

#[test]
fn relay_row_event_count_uses_session_transport_counter_after_subs_close() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connecting_url(RelayRole::Indexer, "wss://indexer-relay.example/");
    kernel.relay_connected_url(RelayRole::Indexer, "wss://indexer-relay.example/");
    kernel.record_transport_event(
        RelayRole::Indexer,
        "wss://indexer-relay.example/",
        Instant::now(),
    );

    let snap = kernel.relay_diagnostics_snapshot();
    let row = snap
        .relays
        .iter()
        .find(|row| row.relay_url == "wss://indexer-relay.example")
        .expect("diagnostics must include the indexer socket URL");

    assert_eq!(row.total_sub_count, 0, "completed subs may be evicted");
    assert_eq!(row.total_events_rx, 1);
}

#[test]
fn set_relay_info_surfaces_on_diagnostics_row() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;
    use crate::substrate::RelayInfoDoc;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let url = "wss://relay.example/";
    kernel.relay_connecting_url(RelayRole::Content, url);
    kernel.relay_connected_url(RelayRole::Content, url);

    // No document yet — the row's `info` is absent.
    let before = kernel.relay_diagnostics_snapshot();
    let row_before = before
        .relays
        .iter()
        .find(|r| r.relay_url == "wss://relay.example")
        .expect("connected URL must appear");
    assert!(row_before.info.is_none(), "info absent before fetch");

    // ADR-0051 — fold a fetched document onto the URL (the actor's
    // `SetRelayInfo` dispatch arm does exactly this).
    let doc = RelayInfoDoc {
        url: url.to_string(),
        name: Some("Example".to_string()),
        icon: Some("https://relay.example/icon.png".to_string()),
        supported_nips: vec![1, 11, 42],
        limitation_auth_required: Some(true),
        ..RelayInfoDoc::default()
    };
    kernel.set_relay_info(url, doc);

    let after = kernel.relay_diagnostics_snapshot();
    let row = after
        .relays
        .iter()
        .find(|r| r.relay_url == "wss://relay.example")
        .expect("connected URL must appear");
    let info = row
        .info
        .as_ref()
        .expect("info present after set_relay_info");
    assert_eq!(info.name.as_deref(), Some("Example"));
    assert_eq!(info.icon.as_deref(), Some("https://relay.example/icon.png"));
    assert_eq!(info.supported_nips, vec![1, 11, 42]);
    assert_eq!(info.auth_required, Some(true));
}

#[test]
fn relay_info_freshness_gate() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;
    use crate::substrate::RelayInfoDoc;
    use std::time::Duration;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let url = "wss://relay.example/";
    kernel.relay_connecting_url(RelayRole::Content, url);
    kernel.relay_connected_url(RelayRole::Content, url);

    // No doc → never fresh.
    assert!(!kernel.relay_info_is_fresh(url, Duration::from_secs(300)));
    kernel.set_relay_info(url, RelayInfoDoc::for_url(url));
    // Just stored → fresh under any reasonable TTL.
    assert!(kernel.relay_info_is_fresh(url, Duration::from_secs(300)));
    // Zero TTL → immediately stale (boundary exclusive on the "fresh" side).
    assert!(!kernel.relay_info_is_fresh(url, Duration::from_secs(0)));
}
