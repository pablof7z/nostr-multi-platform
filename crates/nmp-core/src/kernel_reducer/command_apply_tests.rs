//! Unit tests for [`super::KernelReducer::apply_actor_command`] (#2045 PR-A) —
//! the interest / relay-info / lifecycle verbs.
//!
//! The publish, sign-roundtrip (NeedsSign), and unsupported-command tests live
//! in the sibling `command_apply_publish_tests.rs` (file-size ceiling split).
//! Together the two files cover every Group-A (Applied), Group-B (NeedsSign),
//! and Group-C (Unsupported) outcome.
//!
//! The tests are written to the public `CommandApplyOutcome` shape and use the
//! same `KernelReducer` seam the wasm runtime drives — no direct `Kernel`
//! access for production paths (only for verification helpers).
//!
//! Include guard: this file is `#[path]`-included by `kernel_reducer.rs`.

use super::*;
use crate::actor::{ActorCommand, InterestsCommand, LifecycleCommand, RelayCommand};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::ObservedProjectionId;
use nmp_network::role::RelayRole;

// ─── shared constants / helpers ──────────────────────────────────────────────

const RELAY: &str = "wss://relay.example";

/// Build a `SubIdentity` with `SubScope::Global` and caller-supplied string keys.
fn global_id(owner: &str, key: &str) -> SubIdentity {
    SubIdentity::new(SubOwnerKey::new(owner), SubKey::new(key), SubScope::Global)
}

/// Build a minimal `LogicalInterest` matching the registry fixture convention.
fn simple_interest(id: u64) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape::default(),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

// ─── Group A: Applied (synchronous) — interest verbs ─────────────────────────

#[test]
fn ensure_interest_returns_applied_empty() {
    // EnsureInterest installs a logical interest and returns Applied(empty).
    // The registry should gain one active entry.
    let mut r = KernelReducer::new();
    let identity = global_id("test-owner", "test-key-1");
    let interest = simple_interest(1);

    let outcome =
        r.apply_actor_command(ActorCommand::Interests(InterestsCommand::EnsureInterest {
            identity,
            interest,
        }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "EnsureInterest must return Applied(empty)"
    );
    // Registry now holds one active interest (proves the call reached the
    // kernel, not just returned the right outcome).
    assert_eq!(
        r.kernel.lifecycle_mut().registry_mut().iter_active().len(),
        1,
        "registry must have one active interest after EnsureInterest"
    );
}

#[test]
fn drop_interest_owner_of_unregistered_identity_is_applied_noop() {
    // D6: DropInterestOwner for an identity that was never registered must not
    // panic — it is a no-op that still returns Applied(empty).
    let mut r = KernelReducer::new();
    let identity = global_id("phantom-owner", "never-registered");

    let outcome = r.apply_actor_command(ActorCommand::Interests(
        InterestsCommand::DropInterestOwner(identity),
    ));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "DropInterestOwner on phantom identity must return Applied(empty)"
    );
    assert_eq!(
        r.kernel.lifecycle_mut().registry_mut().iter_active().len(),
        0,
        "registry must remain empty"
    );
}

#[test]
fn ensure_then_drop_interest_owner_clears_registry() {
    // EnsureInterest registers; DropInterestOwner with the same identity
    // removes it — the registry should be empty after the drop.
    let mut r = KernelReducer::new();
    let identity = global_id("drop-owner", "drop-key");
    let interest = simple_interest(2);

    // Install the interest.
    let _ = r.apply_actor_command(ActorCommand::Interests(InterestsCommand::EnsureInterest {
        identity: identity.clone(),
        interest,
    }));
    assert_eq!(
        r.kernel.lifecycle_mut().registry_mut().iter_active().len(),
        1
    );

    // Drop the owner.
    let outcome = r.apply_actor_command(ActorCommand::Interests(
        InterestsCommand::DropInterestOwner(identity),
    ));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "DropInterestOwner on registered identity must return Applied(empty)"
    );
    assert_eq!(
        r.kernel.lifecycle_mut().registry_mut().iter_active().len(),
        0,
        "registry must be empty after drop"
    );
}

#[test]
fn open_interest_with_relay_connected_emits_req_frame() {
    // OpenInterest after relay connect: Applied with at least one REQ frame.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);

    let outcome = r.apply_actor_command(ActorCommand::Interests(InterestsCommand::OpenInterest {
        filter_json: r#"{"kinds":[1]}"#.to_string(),
        consumer_id: "chirp-home".to_string(),
        scope: 1, // Global
    }));

    match outcome {
        CommandApplyOutcome::Applied(frames) => {
            assert!(
                frames.iter().any(|m| m.text.contains("REQ")),
                "OpenInterest with relay connected must produce a REQ frame; got: {:?}",
                frames.iter().map(|m| &m.text).collect::<Vec<_>>()
            );
        }
        other => panic!("expected Applied, got {other:?}"),
    }
}

#[test]
fn open_observed_interest_with_relay_connected_emits_req_frame() {
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    let shape = InterestShape {
        kinds: [1].into_iter().collect(),
        ..Default::default()
    };

    let outcome = r.apply_actor_command(ActorCommand::Interests(
        InterestsCommand::OpenObservedInterest {
            filter_json: r#"{"kinds":[1]}"#.to_string(),
            consumer_id: "observed-feed".to_string(),
            scope: 1,
            relay_pin: None,
            is_indexer_discovery: false,
            observer_id: ObservedProjectionId(1),
            replay_shapes: vec![shape],
            replay_limit: 64,
        },
    ));

    match outcome {
        CommandApplyOutcome::Applied(frames) => {
            assert!(
                frames.iter().any(|m| m.text.contains("REQ")),
                "OpenObservedInterest with relay connected must produce a REQ frame; got: {:?}",
                frames.iter().map(|m| &m.text).collect::<Vec<_>>()
            );
        }
        other => panic!("expected Applied, got {other:?}"),
    }
}

#[test]
fn open_observed_interest_preserves_indexer_discovery_routing() {
    let mut r = KernelReducer::new();
    let shape = InterestShape {
        kinds: [10154].into_iter().collect(),
        ..Default::default()
    };

    let outcome = r.apply_actor_command(ActorCommand::Interests(
        InterestsCommand::OpenObservedInterest {
            filter_json: r#"{"kinds":[10154]}"#.to_string(),
            consumer_id: "podcast-discovery".to_string(),
            scope: 1,
            relay_pin: None,
            is_indexer_discovery: true,
            observer_id: ObservedProjectionId(7),
            replay_shapes: vec![shape],
            replay_limit: 64,
        },
    ));

    assert!(matches!(outcome, CommandApplyOutcome::Applied(_)));
    let active = r.kernel.lifecycle_mut().registry_mut().iter_active();
    assert_eq!(active.len(), 1);
    assert!(
        active[0].is_indexer_discovery,
        "OpenObservedInterest must preserve the observed-projection routing bit"
    );

    let _ = r.apply_actor_command(ActorCommand::Interests(InterestsCommand::CloseInterest {
        filter_json: r#"{"kinds":[10154]}"#.to_string(),
        consumer_id: "podcast-discovery".to_string(),
        scope: 1,
        relay_pin: None,
        is_indexer_discovery: true,
    }));
    assert!(
        r.kernel
            .lifecycle_mut()
            .registry_mut()
            .iter_active()
            .is_empty(),
        "close must reconstruct the same indexer-discovery identity"
    );
}

#[test]
fn close_interest_after_open_emits_close_frame() {
    // CloseInterest: Applied with at least one CLOSE frame when the sub was open.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    let filter = r#"{"kinds":[1]}"#.to_string();
    let _ = r.apply_actor_command(ActorCommand::Interests(InterestsCommand::OpenInterest {
        filter_json: filter.clone(),
        consumer_id: "chirp-home".to_string(),
        scope: 1,
    }));

    let outcome = r.apply_actor_command(ActorCommand::Interests(InterestsCommand::CloseInterest {
        filter_json: filter,
        consumer_id: "chirp-home".to_string(),
        scope: 1,
        relay_pin: None,
        is_indexer_discovery: false,
    }));

    match outcome {
        CommandApplyOutcome::Applied(frames) => {
            assert!(
                frames.iter().any(|m| m.text.contains("CLOSE")),
                "CloseInterest must produce a CLOSE frame; got: {:?}",
                frames.iter().map(|m| &m.text).collect::<Vec<_>>()
            );
        }
        other => panic!("expected Applied, got {other:?}"),
    }
}

// ─── Group A: Applied (synchronous) — relay-info / lifecycle / contacts ──────

#[test]
fn set_relay_info_valid_json_returns_applied_empty() {
    // SetRelayInfo with a NIP-11-shaped JSON → Applied(empty), no panic.
    let mut r = KernelReducer::new();
    let doc_json = r#"{"name":"test relay","description":"a relay for tests"}"#;

    let outcome = r.apply_actor_command(ActorCommand::Relay(RelayCommand::SetRelayInfo {
        relay_url: RELAY.to_string(),
        doc_json: doc_json.to_string(),
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "SetRelayInfo (valid JSON) must return Applied(empty)"
    );
}

#[test]
fn set_relay_info_malformed_json_returns_applied_empty_no_panic() {
    // D6: SetRelayInfo with garbage JSON must not panic — the silent-drop path
    // still returns Applied(empty).
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Relay(RelayCommand::SetRelayInfo {
        relay_url: RELAY.to_string(),
        doc_json: "not json at all".to_string(),
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "SetRelayInfo (malformed JSON) must return Applied(empty)"
    );
}

#[test]
fn mark_changed_since_emit_sets_dirty_flag() {
    // MarkChangedSinceEmit → Applied(empty) and `changed_since_emit` is true.
    let mut r = KernelReducer::new();
    // Clear the dirty flag first.
    let _ = r.make_update_frame(true);
    assert!(
        !r.changed_since_emit(),
        "dirty flag must be clear after make_update_frame"
    );

    let outcome = r.apply_actor_command(ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    ));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "MarkChangedSinceEmit must return Applied(empty)"
    );
    assert!(
        r.changed_since_emit(),
        "changed_since_emit must be true after MarkChangedSinceEmit"
    );
}
