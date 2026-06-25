// This test uses native-only helpers (inject_relay_*, snapshot_bytes_for_test)
// that only exist under #[cfg(not(target_arch = "wasm32"))].  Skip the entire
// file when compiled for wasm32 so `wasm-pack test` does not abort the build.
#![cfg(not(target_arch = "wasm32"))]

// PR-1 acceptance: snapshot is kernel-authored.
//
// Proves that the snapshot emitted after `Start` carries real kernel data
// rather than the former hand-rolled fabrication:
//
//  1. The Tier-3 `relay_statuses` row for the bootstrap URL has
//     `connection == "connected"` (not the old hardcoded `"configured"`),
//     `events_rx > 0`, and a non-zero `last_tick_ms` — observable only after
//     `handle_relay_connected` + a NIP-01 EVENT frame mutate kernel state.
//
//  2. The Tier-2 `configured_relays` sidecar contains ALL bootstrap URLs.
//     The former `encode_snapshot_frame(…, &[])` emitted an empty sidecar
//     array; this assertion passes only when `make_update_frame` runs the
//     kernel's typed-projection pipeline.
//
//  3. The unconditional profile-cluster typed sidecars (`accounts`,
//     `active_account`, `profile`) are present in the decoded projections.
//     These keys are always emitted by `builtin_typed_projections` regardless
//     of relay state — confirming the full Tier-2 sidecar pipeline fired.

use nmp_core::{
    decode_snapshot_envelope, decode_snapshot_typed_projections,
    typed_projections::{
        decode_configured_relays, ACCOUNTS_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_ID,
        CONFIGURED_RELAYS_SCHEMA_ID, PROFILE_SCHEMA_ID,
    },
};
use nmp_network::role::RelayRole;
use nmp_wasm::{RelayBootstrapEntry, StartConfig, RawWasmAbiAdapter, WorkerRequest};

const RELAY_URL_A: &str = "wss://nos.lol";
const RELAY_URL_B: &str = "wss://relay.damus.io";

#[test]
fn snapshot_carries_kernel_authored_relay_statuses() {
    let mut runtime = RawWasmAbiAdapter::new();

    // Start with TWO bootstrap relays so the configured_relays sidecar
    // assertion below must contain both URLs (not just a single-entry list).
    runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec![RELAY_URL_A.to_string(), RELAY_URL_B.to_string()],
            relay_bootstrap: vec![
                RelayBootstrapEntry {
                    url: RELAY_URL_A.to_string(),
                    role: "both".to_string(),
                },
                RelayBootstrapEntry {
                    url: RELAY_URL_B.to_string(),
                    role: "both".to_string(),
                },
            ],
            database_name: "chirp-test".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .unwrap();

    // Drive a relay-connected event so the kernel's RelayHealth flips to
    // "connected" — required for `connection == "connected"` in the snapshot.
    runtime.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL_A);

    // Inject one synthetic NIP-01 EVENT so `events_rx > 0`.
    let event_frame = r#"["EVENT","sub1",{"id":"aaaa","pubkey":"3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d","created_at":1700000000,"kind":1,"tags":[],"content":"test","sig":"0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"}]"#;
    runtime.inject_relay_text_frame_for_test(
        RelayRole::Content,
        RELAY_URL_A,
        event_frame.to_string(),
    );

    let bytes = runtime.snapshot_bytes_for_test();
    assert!(!bytes.is_empty(), "snapshot must not be empty");

    // ── Tier-3: relay_statuses ────────────────────────────────────────────────
    let env = decode_snapshot_envelope(&bytes).expect("snapshot must decode");

    let relay_row = env
        .relay_statuses
        .iter()
        .find(|r| r.relay_url == RELAY_URL_A)
        .unwrap_or_else(|| {
            panic!(
                "relay_statuses must contain the bootstrap URL; got: {:?}",
                env.relay_statuses
            )
        });

    assert_eq!(
        relay_row.connection, "connected",
        "relay connection state must be 'connected' after inject_relay_connected"
    );
    assert!(
        relay_row.events_rx > 0,
        "events_rx must be > 0 after injecting an EVENT frame; got {}",
        relay_row.events_rx
    );
    assert!(
        env.last_tick_ms > 0,
        "last_tick_ms must be non-zero after a real make_update call"
    );

    // ── Tier-2: configured_relays sidecar ────────────────────────────────────
    let projections =
        decode_snapshot_typed_projections(&bytes).expect("typed projections must decode");

    let cr_entry = projections
        .iter()
        .find(|p| p.schema_id == CONFIGURED_RELAYS_SCHEMA_ID)
        .unwrap_or_else(|| {
            let keys: Vec<_> = projections.iter().map(|p| &p.schema_id).collect();
            panic!(
                "configured_relays sidecar must be present; got keys: {keys:?}"
            )
        });

    let model =
        decode_configured_relays(&cr_entry.payload).expect("configured_relays must decode");

    for url in &[RELAY_URL_A, RELAY_URL_B] {
        assert!(
            model.relays.iter().any(|row| row.url == *url),
            "configured_relays sidecar must contain '{}'; got: {:?}",
            url,
            model.relays
        );
    }

    // ── Tier-2: unconditional profile-cluster sidecars ───────────────────────
    //
    // `accounts`, `active_account`, and `profile` are pushed unconditionally
    // by `builtin_typed_projections` on every make_update tick (even on a
    // fresh kernel with no accounts). Their presence confirms the full Tier-2
    // typed-projection pipeline fired, not just the relay-specific path.
    for schema_id in &[ACCOUNTS_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_ID, PROFILE_SCHEMA_ID] {
        assert!(
            projections.iter().any(|p| p.schema_id == *schema_id),
            "unconditional profile-cluster sidecar '{}' must be present; got keys: {:?}",
            schema_id,
            projections.iter().map(|p| &p.schema_id).collect::<Vec<_>>()
        );
    }
}
