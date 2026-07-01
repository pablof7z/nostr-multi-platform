//! Host-extensible snapshot output — end-to-end proof for the typed
//! FlatBuffers sidecar seam (ADR-0037).
//!
//! The generic (`serde_json::Value`) projection lane has been removed; only
//! typed projections are tested here. Tests that previously exercised
//! `SnapshotRegistry::register` / `run` have been deleted alongside that
//! deleted API.

use super::snapshot_registry::{new_snapshot_projection_slot, SnapshotRegistry};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::time::{Duration, UNIX_EPOCH};
use crate::update_envelope::{TypedProjectionData, WireProjectionState};
use std::sync::Arc;

/// Build a minimal opaque [`TypedProjectionData`] entry for the typed-sidecar
/// tests (ADR-0037). Payload bytes are arbitrary — `nmp-core` never reads them.
fn typed_entry(key: &str, payload: &[u8]) -> TypedProjectionData {
    TypedProjectionData {
        key: key.to_string(),
        schema_id: key.to_string(),
        schema_version: 1,
        file_identifier: "TEST".to_string(),
        payload: payload.to_vec(),
        ..Default::default()
    }
}

/// With no *host* projection registered, the `projections` map carries only
/// the kernel-owned built-in projections — and no host namespace.
///
/// D0: `make_update` always inserts the publish / relay-settings cluster
/// (`publish_queue` / `publish_outbox` / `configured_relays` /
/// `relay_role_options`), the identity pair (`accounts` / `active_account`),
/// and the views cluster — all kernel-owned domain state,
/// not host registrations — so the map is never empty and `skip_serializing_if`
/// no longer drops it. A host that registers nothing simply contributes no
/// extra keys: the social shell still sees only the built-ins it expects.
#[test]
fn no_host_projection_leaves_only_the_builtin_projections() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parsed: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    let projections = parsed
        .get("projections")
        .expect("the built-in projections keep the projections map on the wire");
    let map = projections
        .as_object()
        .expect("projections must serialize as a JSON object");
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();
    // D5: view-dependent keys (`timeline`, `inserted`, `updated`, `removed`,
    // `author_view`, `thread_view`) are absent when no view is open. The
    // expected set is the static cluster only.
    assert_eq!(
        keys,
        [
            // identity pair
            "accounts",
            "active_account",
            // ADR-0063 Lane H: claimed_profiles / mention_profiles /
            // resolved_profiles / claimed_events JSON projections deleted.
            // Profile/event resolution is now served by refs.profile / refs.event
            // NRRD row-delta sidecars.
            //
            // app-declared relay configuration (formerly `relay_edit_rows`).
            "configured_relays",
            // publish cluster — outbox header summary (§6 anti-pattern #1)
            "outbox_summary",
            // views cluster (D0) — `profile` is always present
            "profile",
            // publish cluster
            "publish_outbox",
            "publish_queue",
            // diagnostics roll-up (aim.md §4.5 / §6 anti-pattern #1 cleanup)
            "relay_diagnostics",
            "relay_role_options",
            // settings-hub view (relays subtitle pre-format)
            "settings_hub",
            // D5: dynamic feed keys and the retired timeline delta keys are
            // absent when no dynamic feed is registered.
        ],
        "with no host projection and no open views the map carries only the static built-ins"
    );
}

/// Pin [`KERNEL_BUILTIN_PROJECTION_KEYS`] against the actual insertion code:
/// every key a no-host-projection tick emits must be listed in the const, and
/// the const must not list a key the kernel can no longer produce (the
/// conditional drain-on-emit keys — `action_results` / `signed_events` /
/// `action_stages` / `action_lifecycle` — are absent on an idle tick, so the
/// reverse check exempts exactly that documented quartet).
///
/// This is what keeps the registry-coverage gate in `nmp-app-chirp`
/// (`every_codegen_registry_key_is_registered_at_runtime`) honest: that gate
/// treats const membership as "the kernel produces this key", which is only
/// sound while this test pins the const to the real insertion sites.
#[test]
fn builtin_projection_keys_const_matches_runtime() {
    use crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parsed: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    let emitted: std::collections::BTreeSet<&str> = parsed
        .get("projections")
        .and_then(|p| p.as_object())
        .expect("projections map present")
        .keys()
        .map(String::as_str)
        .collect();

    // Forward: every emitted built-in key is declared in the const.
    for key in &emitted {
        assert!(
            KERNEL_BUILTIN_PROJECTION_KEYS.contains(key),
            "kernel emitted built-in projection key {key:?} that is missing from \
             KERNEL_BUILTIN_PROJECTION_KEYS — add it so the registry-coverage \
             gate keeps seeing the full producer surface"
        );
    }

    // Reverse: every const key is either emitted on an idle tick or one of the
    // four documented drain-on-emit conditionals, OR one of the two ADR-0063
    // (#1671) keyed row-delta carriers (`refs.profile` / `refs.event`). The
    // latter are typed-sidecar-ONLY built-ins (an opaque NRRD per-key batch
    // consumed by the host `RefRowCache`) — they have no generic JSON
    // `projections` map entry, so they will never appear in the JSON snapshot
    // this test parses, yet they ARE produced every tick on the typed sidecar.
    let conditional = [
        "action_results",
        "signed_events",
        "action_stages",
        "action_lifecycle",
        "refs.profile",
        "refs.event",
    ];
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        assert!(
            emitted.contains(key) || conditional.contains(key),
            "KERNEL_BUILTIN_PROJECTION_KEYS lists {key:?}, but an idle tick does \
             not emit it and it is not a documented drain-on-emit conditional — \
             the const has drifted from snapshot_projections_with_publish_cluster"
        );
    }
}

/// ADR-0037: `run_typed` carries registered opaque bytes by projection key.
#[test]
fn registered_typed_projection_surfaces_through_run_typed() {
    let slot = new_snapshot_projection_slot();
    slot.lock().unwrap().register_typed("test.feed.home", || {
        Some(typed_entry("test.feed.home", &[0xde, 0xad, 0xbe, 0xef]))
    });

    let mut registry = slot.lock().unwrap();
    let typed = registry.run_typed();
    assert_eq!(typed.len(), 1, "one typed projection was registered");
    assert_eq!(typed[0].key, "test.feed.home");
    assert_eq!(typed[0].payload, vec![0xde, 0xad, 0xbe, 0xef]);
}

/// `None` means "no changed payload this tick", not "clear".
#[test]
fn typed_projection_returning_none_is_skipped() {
    let slot = new_snapshot_projection_slot();
    {
        let mut registry = slot.lock().unwrap();
        registry.register_typed("present", || Some(typed_entry("present", &[1, 2, 3])));
        registry.register_typed("absent", || None);
    }
    let typed = slot.lock().unwrap().run_typed();
    assert_eq!(typed.len(), 1, "the `None`-returning projection is skipped");
    assert_eq!(typed[0].key, "present");
}

/// D6: a panicking typed projection is omitted without killing siblings.
#[test]
fn panicking_typed_projection_is_contained_and_others_survive() {
    let slot = new_snapshot_projection_slot();
    {
        let mut registry = slot.lock().unwrap();
        registry.register_typed("good", || Some(typed_entry("good", &[0x42])));
        registry.register_typed("bad", || -> Option<TypedProjectionData> {
            panic!("buggy typed host projection");
        });
    }
    let typed = slot.lock().unwrap().run_typed();
    assert_eq!(
        typed.len(),
        1,
        "the panicking typed projection is dropped, the good one survives"
    );
    assert_eq!(typed[0].key, "good");
}

/// `remove(key)` drops the typed entry and emits one `Cleared` row.
#[test]
fn remove_drops_typed_and_emits_cleared_row() {
    let mut registry = SnapshotRegistry::new();
    // A transient feed registers a typed projection; a sibling is also present.
    registry.register_typed("nmp.feed.author.alice", || {
        Some(typed_entry("nmp.feed.author.alice", &[0xAB]))
    });
    registry.register_typed("test.feed.home", || {
        Some(typed_entry("test.feed.home", &[0x01]))
    });

    // Removing the transient key reports success.
    assert!(registry.remove("nmp.feed.author.alice"));

    // The next run_typed emits one Cleared row for the removed key.
    let typed = registry.run_typed();
    let clear = typed
        .iter()
        .find(|t| t.key == "nmp.feed.author.alice")
        .expect("Cleared row");
    assert_eq!(clear.state, WireProjectionState::Cleared);
    assert!(clear.payload.is_empty(), "Cleared rows carry no payload");

    // Cleared row is one-shot: the next run does not re-emit it.
    let typed_again = registry.run_typed();
    assert!(
        typed_again.iter().all(|t| t.key != "nmp.feed.author.alice"),
        "typed Cleared row must be one-shot"
    );

    // The sibling (home feed) is untouched.
    let home = typed.iter().find(|t| t.key == "test.feed.home");
    assert!(home.is_some(), "removing one key must not disturb siblings");

    // Idempotent: a second remove of the now-absent key reports `false`.
    assert!(!registry.remove("nmp.feed.author.alice"));
    // Removing a never-registered key is a harmless `false`.
    assert!(!registry.remove("nmp.feed.thread.never"));
}

/// `run_typed_projections` with no slot bound yields an empty vector — D6: a
/// kernel constructed outside the actor never panics on the typed path.
#[test]
fn unbound_slot_yields_empty_typed_projections() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(kernel.run_typed_projections().is_empty());
}

/// A typed projection bound onto the kernel surfaces through
/// `Kernel::run_typed_projections` — the path `make_update` drives to build the
/// snapshot frame's `typed_projections` sidecar.
#[test]
fn typed_projection_surfaces_through_kernel_run_typed_projections() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    slot.lock().unwrap().register_typed("test.feed.home", || {
        Some(typed_entry("test.feed.home", &[0xab, 0xcd]))
    });
    kernel.set_snapshot_projection_handle(slot);

    let typed = kernel.run_typed_projections();
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].key, "test.feed.home");
    assert_eq!(typed[0].payload, vec![0xab, 0xcd]);
}

/// Time-aware typed projections receive the current kernel clock value when the
/// snapshot is emitted, not a registration-time/default timestamp.
#[test]
fn time_aware_typed_projection_uses_injected_time_on_emit() {
    const BASE_SECS: u64 = 1_987_654_321;
    const ADVANCE_SECS: u64 = 37;
    const KEY: &str = "host.time_probe";

    let clock = Arc::new(MonotonicSecondClock::new(
        UNIX_EPOCH + Duration::from_secs(BASE_SECS),
    ));
    let mut reducer = crate::KernelReducer::new();
    let kernel_clock: Arc<dyn crate::Clock> = clock.clone();
    reducer.set_clock_for_test(kernel_clock);
    reducer.register_typed_snapshot_projection_with_time(KEY, |now_secs| {
        Some(typed_entry(KEY, &now_secs.to_le_bytes()))
    });

    let payload_for = |frame: &crate::UpdateFrameBytes| {
        crate::decode_snapshot_typed_projections(frame)
            .expect("typed projections decode")
            .into_iter()
            .find(|entry| entry.key == KEY)
            .unwrap_or_else(|| panic!("{KEY} projection must be emitted"))
            .payload
    };

    let first_frame = reducer.make_update_frame(true);
    assert_eq!(payload_for(&first_frame), BASE_SECS.to_le_bytes().to_vec());

    clock.advance_secs(ADVANCE_SECS);
    let second_frame = reducer.make_update_frame(true);
    assert_eq!(
        payload_for(&second_frame),
        (BASE_SECS + ADVANCE_SECS).to_le_bytes().to_vec()
    );
}
