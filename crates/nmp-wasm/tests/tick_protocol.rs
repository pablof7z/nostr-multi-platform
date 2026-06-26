// Native-only scheduler tests. The production wasm target schedules the same
// runtime deadline state with a one-shot browser timeout.
#![cfg(not(target_arch = "wasm32"))]

use std::fs;
use std::path::PathBuf;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::publish::{PublishAction, PublishTarget};
use nmp_core::substrate::ActionPayload;
use nmp_network::role::RelayRole;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
use nmp_wasm::{
    DispatchBytes, RawWasmAbiAdapter, RelayBootstrapEntry, ReleaseRef, ResolveRef, SetIdentity,
    StartConfig, WorkerEvent, WorkerRequest,
};

const RELAY_URL: &str = "wss://relay.example";
const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

fn started_runtime() -> RawWasmAbiAdapter {
    let mut runtime = RawWasmAbiAdapter::new();
    runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec![RELAY_URL.to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: RELAY_URL.to_string(),
                role: "both".to_string(),
            }],
            database_name: "scheduler-test".to_string(),
            correlation_id: "start-1".to_string(),
        }))
        .expect("Start must succeed");
    runtime
}

fn seed_account(runtime: &mut RawWasmAbiAdapter) {
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: ACCOUNT.to_string(),
            correlation_id: "seed-account".to_string(),
            identity_relays: Vec::new(),
        }))
        .expect("set identity must succeed");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WorkerEvent::ActionAccepted { .. })),
        "SetIdentity must ACK before publishing; got {events:?}"
    );
}

/// Build a genuinely Schnorr-signed event so that `publish_externally_signed`'s
/// SHA-256 id-hash + Schnorr sig check (#2045 PR-A) passes. Returns the
/// `SignedEvent` and its real hex id (used by relay-OK simulations).
///
/// The key is ephemeral (generated per call); the pubkey in the event does NOT
/// need to match `ACCOUNT` because `verify_externally_signed_event` only checks
/// cryptographic well-formedness, not author-vs-active-account identity.
fn real_signed_event() -> (SignedEvent, String) {
    let keys = nostr::Keys::generate();
    let event = nostr::EventBuilder::new(nostr::Kind::from(1u16), "deadline probe")
        .custom_created_at(nostr::Timestamp::from_secs(1_700_000_000))
        .sign_with_keys(&keys)
        .expect("test key signs");
    let real_id = event.id.to_hex();
    let signed = SignedEvent {
        id: real_id.clone(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event
                .tags
                .iter()
                .map(|t: &nostr::Tag| t.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    };
    (signed, real_id)
}

/// Build a publish dispatch byte envelope with a real signed event. Returns the
/// `WorkerRequest` AND the event's actual NIP-01 hex id (for relay-OK simulation).
///
/// Uses `Explicit` target with `RELAY_URL` so the `NoopOutboxResolver` (which
/// returns nothing for `Auto` targets) actually queues the event and the kernel
/// publish deadline fires. The tests here exercise the deadline scheduler, not
/// the NIP-65 outbox resolver.
fn publish_request(correlation_id: &str) -> (WorkerRequest, String) {
    let (event, event_id) = real_signed_event();
    let payload = PublishAction::Publish {
        handle: event_id.clone(),
        event,
        target: PublishTarget::Explicit {
            relays: vec![RELAY_URL.to_string()],
        },
    }
    .encode();
    (
        WorkerRequest::DispatchBytes(DispatchBytes {
            bytes: encode_dispatch_envelope(
                correlation_id,
                "nmp.publish",
                DISPATCH_ENVELOPE_SCHEMA_VERSION,
                &payload,
            ),
        }),
        event_id,
    )
}

fn resolve_profile_request(consumer_id: &str) -> WorkerRequest {
    WorkerRequest::ResolveRef(ResolveRef {
        namespace: 0,
        key: ACCOUNT.to_string(),
        consumer_id: consumer_id.to_string(),
        shape: 0,
        liveness: 0,
        hints: Vec::new(),
        event_author: None,
        correlation_id: format!("resolve-{consumer_id}"),
    })
}

fn release_profile_request(consumer_id: &str) -> WorkerRequest {
    WorkerRequest::ReleaseRef(ReleaseRef {
        namespace: 0,
        key: ACCOUNT.to_string(),
        consumer_id: consumer_id.to_string(),
        correlation_id: format!("release-{consumer_id}"),
    })
}

fn resolve_event_request(consumer_id: &str, event_id: &str) -> WorkerRequest {
    WorkerRequest::ResolveRef(ResolveRef {
        namespace: 1,
        key: event_id.to_string(),
        consumer_id: consumer_id.to_string(),
        shape: 0,
        liveness: 0,
        hints: vec![RELAY_URL.to_string()],
        event_author: None,
        correlation_id: format!("resolve-event-{consumer_id}"),
    })
}

fn settle_connected_runtime(rt: &mut RawWasmAbiAdapter) {
    let _ = rt.snapshot_bytes_for_test();
    rt.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL);
    let _ = rt
        .fire_maintenance_deadline_for_test()
        .expect("post-start/relay-connected event drain must be armed");
    let _ = rt.snapshot_bytes_for_test();
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "connected runtime fixture must start from an unarmed scheduler"
    );
}

fn assert_action_accepted(events: &[WorkerEvent], expected_action_type: &str) {
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                WorkerEvent::ActionAccepted { action_type, .. }
                    if action_type == expected_action_type
            )
        }),
        "expected ActionAccepted for {expected_action_type}; got {events:?}"
    );
}

fn assert_no_update_bytes(events: &[WorkerEvent]) {
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, WorkerEvent::UpdateBytes { .. })),
        "ref dispatch bookkeeping must not emit a snapshot; got {events:?}"
    );
}

#[test]
fn idle_runtime_deadline_fires_once_and_does_not_rearm() {
    let mut rt = started_runtime();

    assert!(
        rt.maintenance_deadline_armed_for_test(),
        "Start arms a single post-start maintenance deadline"
    );

    let (outbound, dirty) = rt
        .fire_maintenance_deadline_for_test()
        .expect("the post-start deadline must be armed");
    assert!(
        outbound.is_empty(),
        "idle deadline must produce no outbound"
    );
    assert!(!dirty, "idle deadline must not mark a snapshot dirty");
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "idle runtime must not re-arm a fixed cadence"
    );
    assert_eq!(rt.maintenance_deadline_fires_for_test(), 1);

    assert!(
        rt.fire_maintenance_deadline_for_test().is_none(),
        "no second wake exists without a new event or kernel deadline"
    );
}

#[test]
fn resolve_ref_empty_outbound_arms_event_drain_and_drains_lifecycle() {
    let mut rt = started_runtime();
    settle_connected_runtime(&mut rt);

    let events = rt
        .handle(resolve_profile_request("profile-card"))
        .expect("resolve_ref dispatch must succeed");
    assert_action_accepted(&events, "nmp.kernel.resolve_ref");
    assert_no_update_bytes(&events);
    assert_eq!(
        rt.maintenance_deadline_delay_for_test(),
        Some(1_000),
        "resolve_ref returns no outbound directly but must arm one event drain"
    );

    let (outbound, _) = rt
        .fire_maintenance_deadline_for_test()
        .expect("resolve_ref event drain must be armed");
    assert!(
        !outbound.is_empty(),
        "event drain must compile the queued profile interest into outbound"
    );
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "resolve_ref drain must not leave a fixed cadence behind"
    );
}

#[test]
fn release_ref_empty_outbound_arms_event_drain_and_drains_lifecycle_close() {
    let mut rt = started_runtime();
    settle_connected_runtime(&mut rt);

    let _ = rt
        .handle(resolve_profile_request("profile-card-release"))
        .expect("resolve_ref dispatch must succeed");
    let (open_outbound, _) = rt
        .fire_maintenance_deadline_for_test()
        .expect("resolve_ref event drain must be armed");
    assert!(
        !open_outbound.is_empty(),
        "resolve_ref setup must open a lifecycle subscription"
    );
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "resolve_ref setup must settle before release"
    );

    let events = rt
        .handle(release_profile_request("profile-card-release"))
        .expect("release_ref dispatch must succeed");
    assert_action_accepted(&events, "nmp.kernel.release_ref");
    assert_no_update_bytes(&events);
    assert_eq!(
        rt.maintenance_deadline_delay_for_test(),
        Some(1_000),
        "release_ref returns no outbound directly but must arm one event drain"
    );

    let (close_outbound, _) = rt
        .fire_maintenance_deadline_for_test()
        .expect("release_ref event drain must be armed");
    assert!(
        !close_outbound.is_empty(),
        "release event drain must compile the queued teardown into outbound"
    );
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "release_ref drain must not leave a fixed cadence behind"
    );
}

#[test]
fn resolve_event_empty_outbound_arms_event_drain_and_drains_lifecycle() {
    let mut rt = started_runtime();
    settle_connected_runtime(&mut rt);

    let events = rt
        .handle(resolve_event_request("event-claim", &"33".repeat(32)))
        .expect("resolve_ref event dispatch must succeed");
    assert_action_accepted(&events, "nmp.kernel.resolve_ref");
    assert_no_update_bytes(&events);
    assert_eq!(
        rt.maintenance_deadline_delay_for_test(),
        Some(1_000),
        "event resolve_ref returns no outbound directly but must arm one event drain"
    );

    let (outbound, _) = rt
        .fire_maintenance_deadline_for_test()
        .expect("event resolve_ref drain must be armed");
    assert!(
        !outbound.is_empty(),
        "event drain must compile the queued event ref into outbound"
    );
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "event resolve_ref drain must not leave a fixed cadence behind"
    );
}

#[test]
fn outbound_event_wake_stops_when_kernel_declares_no_deadline() {
    let mut rt = started_runtime();
    seed_account(&mut rt);

    let _ = rt
        .fire_maintenance_deadline_for_test()
        .expect("clear the post-start event deadline first");
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "idle post-start drain must not leave a cadence armed"
    );

    let (publish_req, event_id) = publish_request("publish-clears-before-wake");
    let events = rt
        .handle(publish_req)
        .expect("publish dispatch must succeed");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WorkerEvent::ActionAccepted { .. })),
        "publish dispatch must ACK; got {events:?}"
    );
    assert_eq!(
        rt.maintenance_deadline_delay_for_test(),
        Some(1_000),
        "publish outbound should arm one bounded event drain"
    );
    assert!(
        rt.next_runtime_deadline_delay_for_test().is_some(),
        "in-flight publish starts with a kernel deadline"
    );

    let _ = rt.inject_relay_text_frame_for_test(
        RelayRole::Content,
        RELAY_URL,
        format!(r#"["OK","{event_id}",true,""]"#),
    );
    assert_eq!(
        rt.next_runtime_deadline_delay_for_test(),
        None,
        "OK before the event drain should clear the kernel publish deadline"
    );

    let _ = rt.snapshot_bytes_for_test();
    let (outbound, dirty) = rt
        .fire_maintenance_deadline_for_test()
        .expect("event drain must be armed");
    assert!(
        outbound.is_empty(),
        "regression setup expects no new outbound during the follow-up drain"
    );
    assert!(
        !dirty,
        "snapshot pull clears the dirty bit before the follow-up drain"
    );
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "previous outbound must not keep re-arming without a kernel deadline"
    );
}

#[test]
fn publish_outbound_declares_bounded_kernel_deadline() {
    let mut rt = started_runtime();
    seed_account(&mut rt);

    let _ = rt
        .fire_maintenance_deadline_for_test()
        .expect("clear the post-start event deadline first");
    let (publish_req, _event_id) = publish_request("publish-deadline");
    let events = rt
        .handle(publish_req)
        .expect("publish dispatch must succeed");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WorkerEvent::ActionAccepted { .. })),
        "publish dispatch must ACK; got {events:?}"
    );
    assert!(
        rt.next_runtime_deadline_delay_for_test().is_some(),
        "in-flight publish must declare a kernel runtime deadline"
    );
    assert_eq!(
        rt.maintenance_deadline_delay_for_test(),
        Some(1_000),
        "dispatch still gets one event drain before the longer publish deadline"
    );

    let _ = rt.snapshot_bytes_for_test();
    let _ = rt
        .fire_maintenance_deadline_for_test()
        .expect("event drain must be armed after publish outbound");
    let delay = rt
        .maintenance_deadline_delay_for_test()
        .expect("event drain should re-arm at the kernel-declared publish deadline");
    assert!(
        delay > 1_000,
        "follow-up wake must be the publish deadline, not another fixed 1s event drain"
    );
}

#[test]
fn deadline_with_both_role_relay_claims_expansion_does_not_panic() {
    let mut runtime = started_runtime();

    runtime.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL);

    let _ = runtime
        .fire_maintenance_deadline_for_test()
        .expect("relay-connected event must arm a deadline");
}

#[test]
fn relay_event_deadline_signals_dirty_snapshot_then_coalesces() {
    let mut rt = started_runtime();

    let _ = rt.snapshot_bytes_for_test();
    rt.inject_relay_connected_for_test(RelayRole::Content, RELAY_URL);

    let (_, dirty_after_connect) = rt
        .fire_maintenance_deadline_for_test()
        .expect("relay-connected event must arm a deadline");
    assert!(
        dirty_after_connect,
        "deadline after a relay-connected mutation must signal dirty"
    );

    let _ = rt.snapshot_bytes_for_test();
    assert!(
        !rt.maintenance_deadline_armed_for_test(),
        "snapshot-cleared relay event must not leave an idle cadence armed"
    );
}

#[test]
fn production_scheduler_source_has_no_interval_driver() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["Cargo.toml", "src/lib.rs", "src/runtime.rs", "src/tick.rs"] {
        let path = manifest.join(relative);
        let source = fs::read_to_string(&path).expect("scheduler source must be readable");
        for forbidden in [
            "Interval::new",
            "start_tick_interval",
            "tick_interval",
            "setInterval",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} must not reintroduce fixed interval polling token `{}`",
                path.display(),
                forbidden
            );
        }
    }
}
