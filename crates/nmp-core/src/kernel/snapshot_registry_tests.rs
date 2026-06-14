//! Host-extensible snapshot output — end-to-end proof for the
//! `nmp_app_register_snapshot_projection` seam.
//!
//! The bar (direction review #15): a host-registered projection must appear
//! in the JSON `make_update` emits — not merely in `SnapshotRegistry::run`
//! called in isolation. `make_update` is the JSON-emitting path the host
//! actually consumes, so the proof drives it and parses the result, mirroring
//! `t171_planner_error_projection_tests.rs` and `state_projection_tests.rs`.

use super::snapshot_registry::{new_snapshot_projection_slot, SnapshotRegistry};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::TypedProjectionData;

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

/// A projection registered before `make_update` must surface under
/// `projections["<key>"]` in the emitted snapshot JSON.
///
/// Pre-wiring: `KernelSnapshot` has no `projections` field → key absent.
/// Post-wiring: `make_update` runs `run_snapshot_projections()` → the
/// host's `{"count":42}` appears under `test.counter`.
#[test]
fn registered_projection_surfaces_through_make_update() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Bind a shared slot and register a projection — exactly what the actor
    // (`set_snapshot_projection_handle`) + FFI
    // (`nmp_app_register_snapshot_projection`) wiring does in production.
    let slot = new_snapshot_projection_slot();
    slot.lock()
        .unwrap()
        .register("test.counter", || serde_json::json!({ "count": 42 }));
    kernel.set_snapshot_projection_handle(slot);

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    let count = parsed
        .get("projections")
        .and_then(|p| p.get("test.counter"))
        .and_then(|c| c.get("count"))
        .and_then(serde_json::Value::as_u64);
    assert_eq!(
        count,
        Some(42),
        "host projection must appear under projections[\"test.counter\"], got: {snapshot_json}"
    );
}

/// Multiple namespaces coexist — a marketplace and a todo app can each carry
/// their own snapshot namespace without colliding.
#[test]
fn multiple_projections_each_get_their_namespace() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    {
        let mut registry = slot.lock().unwrap();
        registry.register("market.listings", || serde_json::json!([{ "id": "a" }]));
        registry.register("todo.items", || serde_json::json!({ "open": 3 }));
    }
    kernel.set_snapshot_projection_handle(slot);

    let parsed: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    let projections = parsed.get("projections").expect("projections object");
    assert_eq!(
        projections.get("market.listings"),
        Some(&serde_json::json!([{ "id": "a" }]))
    );
    assert_eq!(
        projections.get("todo.items"),
        Some(&serde_json::json!({ "open": 3 }))
    );
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
            // generic claimed-event projection (F-CR-06 / ADR-0034):
            // primary_id -> ClaimedEventDto for every event a renderer
            // has called `claim_event` on and that has since arrived in
            // the read-cache. Always present (empty `{}` is the no-claim
            // steady state) so a host that pre-allocates the map slot
            // never sees an absent key.
            "claimed_events",
            // generic claimed-profile projection: pubkey -> ProfileCard for
            // profile references a component has called `claim_profile` on.
            "claimed_profiles",
            // app-declared relay configuration (formerly `relay_edit_rows`).
            "configured_relays",
            // derived view: per-author mention payloads scoped to the
            // open author-view items (aim.md §4.2). Always present (D1).
            "mention_profiles",
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
            // pre-merged profile map: pubkey -> ProfileCard, merged once in
            // Rust from claimed_profiles > author_view.profile > mention_profiles
            // (each only-if-absent). Always present (D1) so consumers can delete
            // their per-platform merge code.
            "resolved_profiles",
            // settings-hub view (relays subtitle pre-format)
            "settings_hub",
            // D5: `author_view`, `thread_view`, `timeline`, `inserted`,
            // `updated`, `removed` are absent — no view is open.
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
    // four documented drain-on-emit conditionals.
    let conditional = [
        "action_results",
        "signed_events",
        "action_stages",
        "action_lifecycle",
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

/// A projection registered on the shared slot AFTER it was bound onto the
/// kernel still fires: the slot is `Arc`-shared, so a later registration
/// through any clone is visible to the next tick (the production FFI path
/// registers through the `NmpApp` clone after the actor already bound its
/// clone onto the kernel).
#[test]
fn projection_registered_after_binding_still_fires() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));

    // First tick: no host projection registered yet — the map carries only
    // the kernel-owned built-in publish cluster, never the `late.value` key.
    let first: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    assert!(
        first
            .get("projections")
            .and_then(|p| p.get("late.value"))
            .is_none(),
        "a host projection must not appear before it is registered"
    );

    // Register through the still-held `Arc` clone — as the FFI path does.
    slot.lock()
        .unwrap()
        .register("late.value", || serde_json::json!("present"));

    // Next tick picks it up.
    let second: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    assert_eq!(
        second
            .get("projections")
            .and_then(|p| p.get("late.value"))
            .and_then(serde_json::Value::as_str),
        Some("present"),
        "a projection registered after binding must fire on the next tick"
    );
}

/// `run_snapshot_projections` with no slot bound yields an empty map — D6:
/// a kernel constructed outside the actor never panics on the projection
/// path.
#[test]
fn unbound_slot_yields_empty_projections() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(kernel.run_snapshot_projections().is_empty());
}

/// D6 — a host projection closure that panics is contained: its key is
/// omitted (the same shape as an unregistered namespace), every other
/// projection in the same tick still produces its value, and the actor
/// thread is never unwound.
///
/// Without the per-closure `catch_unwind` guard, a single buggy host plugin
/// would panic *inside* `make_update` on the actor thread — the actor's
/// outer `catch_unwind` then catches a terminal `Panic` frame and the
/// kernel is permanently dead. A snapshot projection MUST never be able to
/// kill the kernel.
#[test]
fn panicking_projection_is_contained_and_others_survive() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    {
        let mut registry = slot.lock().unwrap();
        // A well-behaved projection registered alongside the bad one.
        registry.register("good.value", || serde_json::json!({ "ok": true }));
        // A buggy host plugin: panics every time it is polled.
        registry.register("bad.value", || -> serde_json::Value {
            panic!("buggy host projection");
        });
    }
    kernel.set_snapshot_projection_handle(slot);

    // First tick: the panic must not propagate out of `make_update`.
    let first: serde_json::Value = serde_json::from_str(&kernel.make_update_json_for_test(true))
        .expect("snapshot json survives a panic");
    let projections = first
        .get("projections")
        .expect("the surviving projection must still produce a projections object");
    assert_eq!(
        projections.get("good.value"),
        Some(&serde_json::json!({ "ok": true })),
        "a panicking sibling must not poison the other projections in the same tick",
    );
    assert!(
        projections.get("bad.value").is_none(),
        "a panicking projection's key must be omitted, not surfaced as garbage: {first}",
    );

    // Second tick: the kernel is still alive and still emits a valid
    // snapshot — the panic did not unwind the actor / kernel.
    let second: serde_json::Value = serde_json::from_str(&kernel.make_update_json_for_test(true))
        .expect("the kernel must survive a panicking projection and keep ticking");
    assert_eq!(
        second.get("projections").and_then(|p| p.get("good.value")),
        Some(&serde_json::json!({ "ok": true })),
        "the good projection must keep firing on every subsequent tick",
    );
}

/// ADR-0037 — a registered typed projection's opaque bytes are collected by
/// `run_typed`, keyed by the projection key, carried verbatim. The typed
/// registry shares the slot with the generic one but is a separate map, so a
/// typed-only registration contributes nothing to `run` (the generic path).
#[test]
fn registered_typed_projection_surfaces_through_run_typed() {
    let slot = new_snapshot_projection_slot();
    slot.lock().unwrap().register_typed("nmp.feed.home", || {
        Some(typed_entry("nmp.feed.home", &[0xde, 0xad, 0xbe, 0xef]))
    });

    let registry = slot.lock().unwrap();
    let typed = registry.run_typed();
    assert_eq!(typed.len(), 1, "one typed projection was registered");
    assert_eq!(typed[0].key, "nmp.feed.home");
    assert_eq!(typed[0].payload, vec![0xde, 0xad, 0xbe, 0xef]);
    assert!(
        registry.run().is_empty(),
        "a typed-only registration must not appear in the generic projection map"
    );
}

/// A typed projection that returns `None` contributes no sidecar entry this
/// tick — the sidecar carries only the projections that have something to emit.
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

/// D6 — a typed projection closure that panics is contained: its entry is
/// omitted and every sibling typed projection in the same tick still produces
/// its bytes. The actor thread is never unwound (same guarantee as the generic
/// `run` path).
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

/// `SnapshotRegistry::remove(key)` drops the projection from BOTH the generic
/// and typed maps, leaving sibling keys untouched. This is the teardown half of
/// the transient-feed seam (M2 author/thread feeds, ADR-0042 §5.1): without it a
/// closed feed's `register_feed` closure keeps emitting a stale empty subtree on
/// every tick.
#[test]
fn remove_drops_generic_and_typed_for_one_key_only() {
    let mut registry = SnapshotRegistry::new();
    // A transient feed registers BOTH a generic and a typed projection under its
    // key; a sibling (e.g. the home feed) is registered too.
    registry.register("nmp.feed.author.alice", || serde_json::json!({ "cards": [] }));
    registry.register_typed("nmp.feed.author.alice", || {
        Some(typed_entry("nmp.feed.author.alice", &[0xAB]))
    });
    registry.register("nmp.feed.home", || serde_json::json!({ "cards": [{ "id": "h" }] }));

    // Removing the transient key reports success and clears it from both maps.
    assert!(registry.remove("nmp.feed.author.alice"));
    let generic = registry.run();
    assert!(
        !generic.contains_key("nmp.feed.author.alice"),
        "generic projection must be gone after remove"
    );
    assert!(
        registry
            .run_typed()
            .iter()
            .all(|t| t.key != "nmp.feed.author.alice"),
        "typed sidecar must be gone after remove"
    );

    // The sibling (home feed) is untouched.
    assert!(
        generic.contains_key("nmp.feed.home"),
        "removing one key must not disturb siblings"
    );

    // Idempotent: a second remove of the now-absent key reports `false`.
    assert!(!registry.remove("nmp.feed.author.alice"));
    // Removing a never-registered key is a harmless `false`.
    assert!(!registry.remove("nmp.feed.thread.never"));
}

// ───────────────────────────────────────────────────────────────────────
// Per-projection change-gating (perf): the opt-in seam that lets a projection
// whose inputs did not change be served from cache instead of re-invoking
// (and re-serializing) on every emit. The ungated `register` path keeps its
// always-run semantics; `register_gated` consults an `Arc<AtomicU64>` rev.
// ───────────────────────────────────────────────────────────────────────

/// A gated projection whose gate value is UNCHANGED between runs is NOT
/// re-invoked: the registry serves the value the closure last produced from its
/// per-key memo. This is the load-bearing perf proof — the multi-MB serializer
/// must not re-run on a clean tick.
#[test]
fn gated_projection_with_unchanged_gate_is_not_reinvoked() {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    let gate = Arc::new(AtomicU64::new(0));
    let calls = Arc::new(AtomicUsize::new(0));

    let mut registry = SnapshotRegistry::new();
    let calls_for_closure = Arc::clone(&calls);
    registry.register_gated("gated.heavy", Arc::clone(&gate) as _, move || {
        calls_for_closure.fetch_add(1, Ordering::SeqCst);
        serde_json::json!({ "n": 1 })
    });

    // First run: no memo yet, so the closure fires and produces its value.
    let first = registry.run();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the first run must invoke the gated closure (cold memo)"
    );
    assert_eq!(
        first.get("gated.heavy"),
        Some(&serde_json::json!({ "n": 1 }))
    );

    // Second + third run with the gate UNCHANGED: the closure must NOT fire
    // again — the cached value is served instead.
    let second = registry.run();
    let third = registry.run();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "an unchanged gate must serve the cache, never re-invoke the closure"
    );
    assert_eq!(
        second.get("gated.heavy"),
        Some(&serde_json::json!({ "n": 1 })),
        "the cached value is returned verbatim on a clean tick"
    );
    assert_eq!(
        third.get("gated.heavy"),
        Some(&serde_json::json!({ "n": 1 }))
    );
}

/// Bumping the gate marks the projection dirty: the next run re-invokes the
/// closure and returns the NEW value, then caches it again until the next bump.
#[test]
fn bumping_the_gate_reinvokes_and_returns_the_new_value() {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    let gate = Arc::new(AtomicU64::new(0));
    let calls = Arc::new(AtomicUsize::new(0));

    let mut registry = SnapshotRegistry::new();
    let calls_for_closure = Arc::clone(&calls);
    let gate_for_closure = Arc::clone(&gate);
    registry.register_gated("gated.value", Arc::clone(&gate) as _, move || {
        calls_for_closure.fetch_add(1, Ordering::SeqCst);
        // The value reflects the current rev so we can prove freshness.
        serde_json::json!({ "rev": gate_for_closure.load(Ordering::SeqCst) })
    });

    let first = registry.run();
    assert_eq!(
        first.get("gated.value"),
        Some(&serde_json::json!({ "rev": 0 }))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Clean tick — cached.
    let _ = registry.run();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "clean tick served from cache"
    );

    // Bump the gate: the next run must re-invoke and return the fresh value.
    gate.store(1, Ordering::SeqCst);
    let after_bump = registry.run();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "a bumped gate must re-invoke the closure exactly once"
    );
    assert_eq!(
        after_bump.get("gated.value"),
        Some(&serde_json::json!({ "rev": 1 })),
        "the re-invoked closure must return the new value, not the stale cache"
    );

    // And it caches the new value again until the next bump.
    let _ = registry.run();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "after a bump-driven run the new value is cached again"
    );
}

/// D6 — a panicking GATED projection is isolated exactly like the ungated path:
/// its key is omitted, the memo is not poisoned (a later clean run still works),
/// and sibling projections in the same tick are unaffected.
#[test]
fn panicking_gated_projection_is_isolated_and_does_not_poison_the_registry() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    let gate = Arc::new(AtomicU64::new(0));
    // A flag that flips the gated closure from "panic" to "succeed" so we can
    // prove the registry is not poisoned by an earlier panic.
    let should_panic = Arc::new(AtomicBool::new(true));

    let mut registry = SnapshotRegistry::new();
    // A well-behaved gated sibling.
    let sibling_gate = Arc::new(AtomicU64::new(0));
    registry.register_gated(
        "gated.good",
        Arc::clone(&sibling_gate) as _,
        || serde_json::json!({ "ok": true }),
    );
    let should_panic_for_closure = Arc::clone(&should_panic);
    registry.register_gated("gated.bad", Arc::clone(&gate) as _, move || {
        if should_panic_for_closure.load(Ordering::SeqCst) {
            panic!("buggy gated projection");
        }
        serde_json::json!({ "recovered": true })
    });

    // First run: the bad projection panics, its key is omitted, the sibling
    // still produces its value.
    let first = registry.run();
    assert_eq!(
        first.get("gated.good"),
        Some(&serde_json::json!({ "ok": true })),
        "a panicking gated sibling must not poison the other projections",
    );
    assert!(
        first.get("gated.bad").is_none(),
        "a panicking gated projection's key must be omitted, got: {first:?}",
    );

    // The registry is NOT poisoned: flip the closure to succeed and bump the
    // gate; a subsequent run produces the value normally.
    should_panic.store(false, Ordering::SeqCst);
    gate.store(1, Ordering::SeqCst);
    let second = registry.run();
    assert_eq!(
        second.get("gated.bad"),
        Some(&serde_json::json!({ "recovered": true })),
        "the registry must keep working after an earlier gated panic",
    );
}

/// The ungated default (`register`) keeps its always-run semantics: it fires on
/// EVERY run, with no gate and no memo — the gated path is strictly opt-in and
/// must never change the behavior of existing call sites.
#[test]
fn ungated_default_projection_runs_every_time() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = SnapshotRegistry::new();
    let calls_for_closure = Arc::clone(&calls);
    registry.register("ungated.value", move || {
        let n = calls_for_closure.fetch_add(1, Ordering::SeqCst) + 1;
        serde_json::json!({ "calls": n })
    });

    let _ = registry.run();
    let _ = registry.run();
    let third = registry.run();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "the ungated default must run on every tick (no gating)"
    );
    assert_eq!(
        third.get("ungated.value"),
        Some(&serde_json::json!({ "calls": 3 })),
        "the ungated projection produces a fresh value every run"
    );
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
    slot.lock().unwrap().register_typed("nmp.feed.home", || {
        Some(typed_entry("nmp.feed.home", &[0xab, 0xcd]))
    });
    kernel.set_snapshot_projection_handle(slot);

    let typed = kernel.run_typed_projections();
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].key, "nmp.feed.home");
    assert_eq!(typed[0].payload, vec![0xab, 0xcd]);
}

// ───────────────────────────────────────────────────────────────────────
// Per-tick observer seam — generic no-result callback fired once per
// `make_update` (the re-home target for per-tick reconcilers like the NIP-57
// zap-subscription, which produced no projection data and only abused the
// projection registry for a per-tick callback).
// ───────────────────────────────────────────────────────────────────────

/// A tick observer bound onto the kernel fires **exactly once per
/// `make_update`** — driven through the real frame path, NOT
/// `run_tick_observers()` in isolation. This is the load-bearing proof that the
/// `make_update` wiring exists: a test that called `run_tick_observers()`
/// directly would pass even if the per-tick invocation point were never wired.
#[test]
fn tick_observer_fires_once_per_make_update() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    let count = Arc::new(AtomicUsize::new(0));
    let count_for_obs = Arc::clone(&count);
    slot.lock().unwrap().register_tick_observer(move || {
        count_for_obs.fetch_add(1, Ordering::SeqCst);
    });
    kernel.set_snapshot_projection_handle(slot);

    let _ = kernel.make_update_json_for_test(true);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a registered tick observer must fire exactly once per make_update tick"
    );

    let _ = kernel.make_update_json_for_test(true);
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "a second make_update tick fires the observer again"
    );
}

/// D6 — a panicking tick observer must NOT break the tick: `make_update` still
/// produces a valid snapshot frame, and every sibling observer in the same tick
/// still fires. A host tick observer is untrusted plugin code running on the
/// actor thread inside the snapshot tick; an unguarded panic would unwind the
/// actor thread into a terminal `Panic` frame and permanently kill the kernel.
#[test]
fn panicking_tick_observer_does_not_break_the_tick() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    let sibling = Arc::new(AtomicUsize::new(0));
    let sibling_for_obs = Arc::clone(&sibling);
    {
        let mut registry = slot.lock().unwrap();
        registry.register_tick_observer(|| panic!("hostile tick observer"));
        registry.register_tick_observer(move || {
            sibling_for_obs.fetch_add(1, Ordering::SeqCst);
        });
    }
    kernel.set_snapshot_projection_handle(slot);

    // The tick completes and produces a valid frame despite the panicking hook.
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON despite a panic");
    assert!(
        parsed.get("projections").is_some(),
        "the tick still emits a snapshot frame"
    );
    assert_eq!(
        sibling.load(Ordering::SeqCst),
        1,
        "a sibling tick observer still fires when another observer panics"
    );
}

/// `run_tick_observers` with no slot bound is a no-op — D6: a kernel
/// constructed outside the actor never panics on the tick-observer path.
#[test]
fn unbound_slot_tick_observers_is_a_noop() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No slot bound: invoking the tick observers must not panic.
    kernel.run_tick_observers();
}

/// V-38: the wallet projection lifecycle test moved to `nmp-nip47` (the
/// crate that now owns `WalletStatus` + the `"wallet"` projection wiring).
/// See `crates/nmp-nip47/tests/snapshot_projection.rs`.
#[cfg(any())]
fn _wallet_projection_moved_to_nmp_nip47() {
    use crate::actor::{new_wallet_status_slot, WalletStatus};

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // The shared wallet-status slot — in production the actor's `WalletRuntime`
    // is the sole writer (D4) and `nmp_app_new` captures a clone in the
    // `"wallet"` projection closure. Here the test plays both roles.
    let wallet_status = new_wallet_status_slot();
    let projection_slot = new_snapshot_projection_slot();
    {
        // Register the SAME closure `nmp_app_new` installs: serialize the slot,
        // contributing `null` when no wallet is connected (D6: a poisoned
        // mutex also collapses to `null`).
        let wallet_status = wallet_status.clone();
        projection_slot
            .lock()
            .unwrap()
            .register("wallet", move || match wallet_status.lock() {
                Ok(slot) => slot
                    .as_ref()
                    .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null),
                Err(_) => serde_json::Value::Null,
            });
    }
    kernel.set_snapshot_projection_handle(projection_slot);

    // No wallet connected → projections["wallet"] is JSON null.
    let before: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    assert!(
        before
            .get("projections")
            .and_then(|p| p.get("wallet"))
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "with no wallet connected projections[\"wallet\"] must be null, got: {before}"
    );

    // Connect a wallet — write to the shared slot exactly as the actor's
    // `sync_wallet_status` does.
    *wallet_status.lock().unwrap() = Some(WalletStatus {
        status: "ready".to_string(),
        relay_url: "wss://wallet.example/".to_string(),
        wallet_npub: "npub1walletexample".to_string(),
        wallet_pubkey_hex: "ab".repeat(32),
        balance_msats: Some(21_000),
        balance_sats: Some(21),
        balance_sats_display: Some("21".to_string()),
        wallet_npub_short: "npub1walle…xample".to_string(), // pre-computed (V-23)
        is_ready: true,
        is_connected: true,
        connection_state: None,
        status_label: "Ready".to_string(), // ADR-0032 / #623
        status_tone: "active".to_string(),
    });
    let connected: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    let wallet = connected
        .get("projections")
        .and_then(|p| p.get("wallet"))
        .expect("projections[\"wallet\"] must be present once a wallet connects");
    assert_eq!(
        wallet.get("status").and_then(serde_json::Value::as_str),
        Some("ready"),
        "a connected wallet must project status=ready",
    );
    assert_eq!(
        wallet.get("relay_url").and_then(serde_json::Value::as_str),
        Some("wss://wallet.example/"),
        "the wallet relay URL must be projected",
    );
    assert_eq!(
        wallet
            .get("balance_msats")
            .and_then(serde_json::Value::as_u64),
        Some(21_000),
        "the wallet balance must be projected when known",
    );
    // V-23 thin-shell: the projection carries pre-computed sats, the formatted
    // display string, the abbreviated npub, and the boolean status helpers so
    // the iOS shell never derives any of these in Swift.
    assert_eq!(
        wallet
            .get("balance_sats")
            .and_then(serde_json::Value::as_u64),
        Some(21),
        "balance_sats must be projected alongside balance_msats (V-23)",
    );
    assert_eq!(
        wallet
            .get("balance_sats_display")
            .and_then(serde_json::Value::as_str),
        Some("21"),
        "balance_sats_display must be projected for the shell (V-23)",
    );
    assert_eq!(
        wallet
            .get("wallet_npub_short")
            .and_then(serde_json::Value::as_str),
        Some("npub1walle…xample"),
        "wallet_npub_short must replace Swift shortNpub() (V-23)",
    );
    assert_eq!(
        wallet.get("is_ready").and_then(serde_json::Value::as_bool),
        Some(true),
        "is_ready must be projected to replace Swift derivation (V-23)",
    );
    assert_eq!(
        wallet
            .get("is_connected")
            .and_then(serde_json::Value::as_bool),
        Some(true),
        "is_connected must be projected to replace Swift derivation (V-23)",
    );

    // Disconnect → the projection clears back to null, not a stale `ready`.
    *wallet_status.lock().unwrap() = None;
    let disconnected: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    assert!(
        disconnected
            .get("projections")
            .and_then(|p| p.get("wallet"))
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "after disconnect projections[\"wallet\"] must clear to null, got: {disconnected}"
    );
}
