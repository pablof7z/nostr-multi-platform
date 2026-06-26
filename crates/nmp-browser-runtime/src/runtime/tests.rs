//! Unit tests for the browser runtime pump loop and the full builder→start
//! wiring (issue #2046 / PR-B + signer/capability track #2049/#2065/#2066/#2067).
//!
//! The low-level tests drive `pump::drain_inbox` directly with a seeded
//! `KernelReducer` so each `CommandApplyOutcome` arm (Applied / NeedsSign /
//! Unsupported) and the bounded-drain budget are asserted in isolation. The
//! high-level tests go through the public `BrowserAppBuilder` to prove
//! `register_defaults` wiring and the command-inbox round-trip.
//!
//! Signer-track additions:
//! - `local_key_provider_brokers_sign_inline` — LocalKey auto-broker.
//! - `deliver_signer_response_host_brokered_path` — host-brokered delivery.
//! - Registry resolve/envelope tests live in `signer/registry.rs`.
//! - Broker unit tests live in `signer/completion.rs`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc};

use nmp_core::actor::{ActorCommand, ActorMail, LifecycleCommand, PublishCommand};
use nmp_core::KernelReducer;
use nmp_signer_iface::{SignerOp, UnsignedEvent};
use nmp_signers::{LocalKeySigner, Signer};

use super::event::BrowserRuntimeEvent;
use super::pump::{drain_inbox, BROWSER_COMMAND_DRAIN_BUDGET};
use crate::relay::WakeCell;
use crate::signer::{CapabilityProviderRegistry, SignerCompletion, SignerCompletionTx};

fn enqueue(cmds: Vec<ActorCommand>) -> mpsc::Receiver<ActorMail> {
    let (tx, rx) = mpsc::channel::<ActorMail>();
    for c in cmds {
        tx.send(ActorMail::Command(c)).expect("send");
    }
    // Drop `tx`: a disconnected-but-non-empty channel still drains every queued
    // item before `Disconnected` is observed, matching the live runtime where
    // the sender outlives the drain.
    rx
}

/// Empty registry + completion channel for tests that don't need auto-brokering.
fn empty_broker() -> (CapabilityProviderRegistry, SignerCompletionTx) {
    let reg = CapabilityProviderRegistry::new();
    let (tx, _rx) = mpsc::channel::<SignerCompletion>();
    (reg, tx)
}

/// A no-op wake cell for `drain_inbox` tests that don't assert on wake firing.
fn noop_wake() -> WakeCell {
    Rc::new(RefCell::new(Rc::new(|| {}) as Rc<dyn Fn()>))
}

#[test]
fn applied_command_produces_no_events_and_no_pending() {
    let mut reducer = KernelReducer::new();
    let rx = enqueue(vec![ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    )]);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());

    assert!(out.events.is_empty(), "Applied must emit no host event");
    assert!(!out.yielded, "single command must not hit the drain budget");
    assert!(pending.is_empty(), "Applied must not park a sign request");
}

#[test]
fn needs_sign_parks_continuation_and_emits_sign_request() {
    let mut reducer = KernelReducer::new();
    // A 64-hex pubkey so the kind:0 publish reaches the sign round-trip.
    reducer.set_active_account_for_test("ab".repeat(32));

    let cmd = ActorCommand::Publish(PublishCommand::Profile {
        fields: serde_json::Map::new(),
        correlation_id: Some("cid-profile".to_string()),
    });
    let rx = enqueue(vec![cmd]);
    let mut pending = HashMap::new();
    // Empty registry — no provider for "ab"*32 → SignRequest must be emitted.
    let (reg, tx) = empty_broker();

    let out = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());

    assert_eq!(out.events.len(), 1, "exactly one SignRequest expected");
    let BrowserRuntimeEvent::SignRequest {
        account_pubkey,
        unsigned_json,
        ..
    } = &out.events[0]
    else {
        panic!("expected SignRequest, got {:?}", out.events[0]);
    };
    assert_eq!(account_pubkey, &"ab".repeat(32));
    assert!(
        unsigned_json.contains("\"kind\":0"),
        "unsigned profile json must carry kind:0"
    );
    assert_eq!(pending.len(), 1, "publish continuation must be parked");
}

#[test]
fn unsupported_command_surfaces_command_failed() {
    let mut reducer = KernelReducer::new();
    // `Stop` is not handled by the headless interpreter → Unsupported.
    let rx = enqueue(vec![ActorCommand::Lifecycle(LifecycleCommand::Stop)]);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let out = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());

    assert_eq!(out.events.len(), 1, "Unsupported must surface one failure");
    let BrowserRuntimeEvent::CommandFailed { reason } = &out.events[0] else {
        panic!("expected CommandFailed, got {:?}", out.events[0]);
    };
    assert!(
        reason.contains("browser_command_unsupported"),
        "failure reason must name the headless-unsupported discriminant: {reason}"
    );
    assert!(pending.is_empty());
}

#[test]
fn drain_is_bounded_by_budget_and_remainder_drains_next_pump() {
    let mut reducer = KernelReducer::new();
    // Unsupported commands emit exactly one event each — a precise per-pump
    // count. Enqueue budget + 10.
    let total = BROWSER_COMMAND_DRAIN_BUDGET + 10;
    let cmds: Vec<ActorCommand> = (0..total)
        .map(|_| ActorCommand::Lifecycle(LifecycleCommand::Stop))
        .collect();
    let rx = enqueue(cmds);
    let mut pending = HashMap::new();
    let (reg, tx) = empty_broker();

    let first = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());
    assert_eq!(
        first.events.len(),
        BROWSER_COMMAND_DRAIN_BUDGET,
        "first pump applies exactly the budget"
    );
    assert!(first.yielded, "budget hit must signal a re-pump");

    let second = drain_inbox(&mut reducer, &rx, &mut pending, &reg, &tx, &noop_wake());
    assert_eq!(second.events.len(), 10, "remainder drains on the next pump");
    assert!(!second.yielded, "remainder is under budget — no further yield");
}

// ── Full builder → start wiring ───────────────────────────────────────────────

fn started_handle() -> crate::BrowserRuntimeHandle {
    crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default())
        .start()
}

/// Build a handle with a `LocalKeySigner` pre-registered and the active
/// account seeded to that signer's pubkey.
fn handle_with_local_key_signer() -> (crate::BrowserRuntimeHandle, String) {
    // Use a deterministic secret so the pubkey is stable within the test.
    let signer = LocalKeySigner::from_secret_hex(&"ee".repeat(32)).expect("valid secret");
    let pubkey_hex = signer.pubkey().to_hex();
    let signer: Arc<dyn Signer> = Arc::new(signer);

    let builder = crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default());
    builder.with_capability_providers([Arc::clone(&signer)]);
    let mut handle = builder.start();
    handle.set_active_account_for_test(pubkey_hex.clone());
    (handle, pubkey_hex)
}

#[test]
fn start_registers_defaults_and_pumps_clean() {
    let mut handle = started_handle();

    // An empty inbox pumps to a clean no-op.
    let out = handle.pump();
    assert!(out.outbound.is_empty());
    assert!(out.events.is_empty());
    assert!(!out.yielded);
    assert_eq!(handle.pending_sign_count(), 0);

    // register_defaults wired substrate + projections: a frame serialises non-empty.
    let frame = handle.make_update_frame(true);
    assert!(!frame.is_empty(), "update frame must be non-empty after start");
}

#[test]
fn command_sender_round_trips_through_pump() {
    let mut handle = started_handle();
    let sender = handle.command_sender();
    // Unsupported on the headless path → surfaced as CommandFailed (not dropped).
    sender
        .send(ActorCommand::Lifecycle(LifecycleCommand::Stop))
        .expect("send through command inbox");

    let out = handle.pump();
    assert_eq!(out.events.len(), 1);
    assert!(matches!(
        out.events[0],
        BrowserRuntimeEvent::CommandFailed { .. }
    ));
}

#[test]
fn configured_relays_snapshot_is_empty_after_without_initial_relays() {
    let handle = started_handle();
    assert!(
        handle.configured_relays().as_slice().is_empty(),
        "without_initial_relays must start with no configured relays"
    );
}

// ── Signer-track tests (#2049/#2065/#2066/#2067) ──────────────────────────────

/// LocalKey auto-broker: pump a profile publish → NeedsSign →
/// LocalKey handles it inline → pending_sign_count()==0 in the same turn.
#[test]
fn local_key_provider_brokers_sign_inline() {
    let (mut handle, _pubkey_hex) = handle_with_local_key_signer();
    let sender = handle.command_sender();

    // A kind:0 profile publish requires a sign round-trip.
    sender
        .send(ActorCommand::Publish(PublishCommand::Profile {
            fields: serde_json::Map::new(),
            correlation_id: Some("lk-inline-cid".to_string()),
        }))
        .expect("send through command inbox");

    let out = handle.pump();

    // LocalKey broker handles it inline → no SignRequest event.
    let sign_requests: Vec<_> = out
        .events
        .iter()
        .filter(|e| matches!(e, BrowserRuntimeEvent::SignRequest { .. }))
        .collect();
    assert!(
        sign_requests.is_empty(),
        "LocalKey provider must not emit SignRequest (auto-brokered inline): \
         events = {:?}",
        out.events
    );

    // The completion is drained in the same pump turn.
    assert_eq!(
        handle.pending_sign_count(),
        0,
        "LocalKey must resolve the sign inline — pending must be 0 after pump()"
    );
}

/// `capability_envelope` returns the correct metadata for the registered signer.
#[test]
fn capability_envelope_reflects_local_key_signer() {
    let (handle, pubkey_hex) = handle_with_local_key_signer();

    let env = handle
        .capability_envelope(&pubkey_hex)
        .expect("envelope must exist for registered pubkey");
    assert!(env.sign_event, "sign_event always true");
    assert!(env.nip04, "LocalKeySigner advertises nip04");
    assert!(env.nip44, "LocalKeySigner advertises nip44");
    assert!(
        matches!(env.backend, nmp_signers::SignerBackend::LocalKey),
        "backend must be LocalKey"
    );

    // Unknown pubkey → None.
    assert!(
        handle.capability_envelope("deadbeef").is_none(),
        "unregistered pubkey must return None"
    );
}

/// Install a counting wake on the handle and return the shared counter.
fn install_counting_wake(handle: &mut crate::BrowserRuntimeHandle) -> Rc<Cell<u32>> {
    let count = Rc::new(Cell::new(0u32));
    let count_clone = Rc::clone(&count);
    handle.set_wake(Rc::new(move || {
        count_clone.set(count_clone.get() + 1);
    }));
    count
}

/// Drive a no-provider profile publish to a parked `SignRequest` and return the
/// emitted correlation id + unsigned-event JSON.
fn park_host_brokered_sign(
    handle: &mut crate::BrowserRuntimeHandle,
    correlation_id: &str,
) -> (String, String) {
    let sender = handle.command_sender();
    sender
        .send(ActorCommand::Publish(PublishCommand::Profile {
            fields: serde_json::Map::new(),
            correlation_id: Some(correlation_id.to_string()),
        }))
        .expect("send");
    let out = handle.pump();
    let sign_req = out
        .events
        .iter()
        .find(|e| matches!(e, BrowserRuntimeEvent::SignRequest { .. }))
        .expect("no-provider path must emit SignRequest");
    let BrowserRuntimeEvent::SignRequest {
        correlation_id,
        unsigned_json,
        ..
    } = sign_req
    else {
        unreachable!()
    };
    (correlation_id.clone(), unsigned_json.clone())
}

/// Host-brokered FAILURE delivery (D4 + D8 blockers): `deliver_signer_response`
/// must NOT touch the reducer (pending unchanged until pump) but MUST fire the
/// wake so the enqueued completion is applied on the next pump, where the
/// failure surfaces as `CommandFailed` and the parked publish is cleared.
#[test]
fn deliver_signer_response_failure_enqueues_fires_wake_and_applies_on_pump() {
    let mut handle = started_handle();
    handle.set_active_account_for_test("ab".repeat(32));
    let wake_count = install_counting_wake(&mut handle);

    let (corr, _unsigned) = park_host_brokered_sign(&mut handle, "host-broker-fail-cid");
    assert_eq!(handle.pending_sign_count(), 1, "one publish parked");

    // Deliver a failure. D4: this must NOT mutate the reducer — it only enqueues
    // and fires the wake.
    handle.deliver_signer_response(corr, Err("user rejected".to_string()));
    assert_eq!(
        wake_count.get(),
        1,
        "deliver_signer_response must fire the wake (D8 re-entry)"
    );
    assert_eq!(
        handle.pending_sign_count(),
        1,
        "D4: reducer untouched until pump() — parked publish still present"
    );

    // The scheduled pump applies the completion: pending cleared, failure shown.
    let out = handle.pump();
    assert_eq!(
        handle.pending_sign_count(),
        0,
        "pump must apply the enqueued completion and clear the parked publish"
    );
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, BrowserRuntimeEvent::CommandFailed { .. })),
        "failure delivery must surface CommandFailed on the applying pump: {:?}",
        out.events
    );
}

/// Host-brokered SUCCESS delivery: a real signature is enqueued via
/// `deliver_signer_response`; the next pump applies it via the success branch
/// (parked publish cleared, NO failure event) — i.e. the signed JSON parsed and
/// `publish_pre_signed` was invoked. (Outbound frame *counts* depend on a wired
/// routing substrate, which this unit harness does not install; the invariant
/// proven here is that the success completion is applied on the scheduled pump.)
#[test]
fn deliver_signer_response_success_applies_via_success_branch() {
    // Sign with a real LocalKey so the delivered JSON is a valid signed event.
    let signer = LocalKeySigner::from_secret_hex(&"f1".repeat(32)).expect("valid secret");
    let pubkey_hex = signer.pubkey().to_hex();

    let mut handle = started_handle();
    handle.set_active_account_for_test(pubkey_hex.clone());
    let wake_count = install_counting_wake(&mut handle);

    let (corr, unsigned_json) = park_host_brokered_sign(&mut handle, "host-broker-ok-cid");
    assert_eq!(handle.pending_sign_count(), 1, "one publish parked");

    // Produce a real signature for the exact unsigned event the kernel emitted.
    let unsigned: UnsignedEvent =
        serde_json::from_str(&unsigned_json).expect("unsigned json must parse");
    let signed_json = match signer.sign(unsigned) {
        SignerOp::Ready(Ok(signed)) => signed.to_nip01_json(),
        other => panic!("local-key sign must be Ready(Ok): {other:?}"),
    };

    handle.deliver_signer_response(corr, Ok(signed_json));
    assert_eq!(wake_count.get(), 1, "success delivery must fire the wake");
    assert_eq!(
        handle.pending_sign_count(),
        1,
        "D4: reducer untouched until pump()"
    );

    let out = handle.pump();
    assert_eq!(
        handle.pending_sign_count(),
        0,
        "pump must apply the signed completion and clear the parked publish"
    );
    // Success branch: no CommandFailed / SignFailed — the signed JSON parsed and
    // routed via publish_pre_signed (vs. the error/Unknown branches).
    assert!(
        !out.events.iter().any(|e| matches!(
            e,
            BrowserRuntimeEvent::CommandFailed { .. } | BrowserRuntimeEvent::SignFailed { .. }
        )),
        "a valid signed delivery must take the success branch (no failure event): {:?}",
        out.events
    );
}
