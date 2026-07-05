//! Full-pipeline regression test for chirp#115.
//!
//! # Why this exists
//!
//! `crates/nmp-nip17/tests/dispatch_integration.rs` only exercises
//! `ActionRegistry::start_bytes` (the pure validation half of dispatching an
//! action) — it never calls `.execute()`, never drives a real actor loop, and
//! never inspects the `action_lifecycle` projection. Kernel-side unit tests
//! (`action_lifecycle_kernel_tests.rs`, `action_stages_tests.rs`,
//! `rung3_cleared_signal_tests.rs`) drive `Kernel::record_action_stage`
//! directly, bypassing the `ActionModule` / `ActionRegistry` / actor-dispatch
//! layers entirely. Neither suite would have caught a break anywhere between
//! "the registry hands `execute()` a `correlation_id`" and "the host's next
//! snapshot frame actually carries an `action_lifecycle` row for that id" —
//! which is the shape of the gap chirp#115 reported: tapping "Publish as DM
//! inboxes" (kind:10050) leaves `RelaySettingsView`'s button on "Publishing…"
//! forever because `action_lifecycle`'s `in_flight` / `recent_terminal` never
//! populate for the dispatched action.
//!
//! This test dispatches `nmp.nip17.publish_relay_list` through the REAL path a
//! production host drives: `ActionRegistry::start_bytes` (mint the
//! `correlation_id`) → `ActionRegistry::execute_bytes` (run
//! `PublishDmRelayListAction::execute`, which enqueues
//! `ActorCommand::Publish(PublishCommand::UnsignedEvent{..})` onto the actor's
//! real command channel) → the actor thread's `dispatch_command` →
//! `dispatch_publish` → `cmd_publish::publish_unsigned_event` → the kernel's
//! `record_action_stage` / `record_action_failure_coded` → `emit_now` →
//! `make_update` → `action_lifecycle_projection()` → the typed FlatBuffers
//! sidecar encoder — and asserts the resulting snapshot frame actually carries
//! an `action_lifecycle` row (the wire-level "changedKeys" signal a host like
//! Chirp keys its spinner off) with our `correlation_id` in a terminal state.
//!
//! No live relay/network is involved: the actor is spawned with no local
//! signing identity configured (`signer_pubkey: None`, no
//! `IdentityCommand::AddSigner` ever sent), so the publish deterministically
//! fails closed at the "no active account" guard
//! (`crates/nmp-core/src/actor/commands/publish_failures.rs::toast_no_account`)
//! — which itself calls `kernel.record_action_failure_coded(..)`, i.e. a real
//! terminal write into the same ledger a successful relay round-trip would
//! use. That keeps the test hermetic and fast while still exercising the
//! entire dispatch→actor→ledger→emit chain end to end. Because the guard
//! resolves synchronously inside the SAME dispatch call that records
//! `Requested` (before the dispatcher's one `emit_now`), the first frame this
//! test observes already carries the TERMINAL state — there is no
//! intervening frame where the id sits in `in_flight` only. That is a property
//! of this specific (synchronous fail-closed) repro path, not something this
//! test can or should assert against; the genuinely async in-flight case
//! (letting the resolver reach a real out-relay TCP connect) is out of scope
//! here.
//!
//! # Root cause + fix
//!
//! This full-pipeline path was already sound on master — verified empirically
//! and via `cargo test`, this test passes unmodified before and after the
//! fix below. The actual chirp#115 break is the ADR-0070 declared-projections
//! narrow-set gate: a host that calls `declare_consumed_projections([...])`
//! with a plausible, valid subset that simply never names
//! `"action_lifecycle"` gets zero warning in any build configuration (the
//! drift gate only catches EXTRA/typo'd declared keys, never missing ones),
//! and the kernel silently, permanently omits `action_lifecycle` from every
//! frame for that host — exactly the symptom reported (os_log evidence:
//! `in_flight` / `recent_terminal` empty for an entire 5-minute run, never
//! populated at all). That regression is pinned directly at
//! `nmp-core::kernel::snapshot_registry::declared_projections_tests::narrow_declared_set_missing_action_lifecycle_still_surfaces_dispatched_action_terminal`
//! (fails before, passes after the `DeclaredProjections::permits` fix in
//! `crates/nmp-core/src/kernel/snapshot_registry/declared.rs`). This test
//! stays as the full-pipeline dispatch-to-wire proof the investigation
//! demanded; that kernel-level test is the minimal reproduction of the actual
//! break point.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use nmp_core::actor::{ActorCommand, LifecycleCommand};
use nmp_core::substrate::{ActionContext, ActionPayload};
use nmp_core::typed_projections::{decode_action_lifecycle, ACTION_LIFECYCLE_SCHEMA_ID};
use nmp_core::{decode_snapshot_typed_projections, TypedProjectionData};
use nmp_nip17::dm_relay_list::PublishDmRelayListAction;
use nmp_nip17::PublishDmRelayListInput;

/// Build a fresh `ActionRegistry` with the `nmp.nip17.publish_relay_list`
/// action module wired — mirrors `dispatch_integration.rs`'s
/// `registry_with_nip17`, trimmed to the one namespace this test dispatches.
fn registry_with_publish_relay_list() -> nmp_core::__ffi_internal::ActionRegistry {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::ActionRegistrar;
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(PublishDmRelayListAction);
    registry
}

/// Drain `upd_rx` until a frame's typed-projection sidecar carries an
/// `action_lifecycle` row whose decoded model satisfies `pred`, or the
/// deadline elapses. Returns the matching decoded model.
///
/// Mirrors the deadline-loop `wait_for_*` helpers in
/// `nmp-testing/tests/nip46_bunker_signing.rs` — no blind `sleep`, no
/// polling loop that could spin forever (D-doctrine: no busy-poll without a
/// bound).
fn wait_for_action_lifecycle(
    upd_rx: &Receiver<Vec<u8>>,
    timeout: Duration,
    mut pred: impl FnMut(&nmp_core::typed_projections::ActionLifecycleModel) -> bool,
) -> Option<nmp_core::typed_projections::ActionLifecycleModel> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match upd_rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(frame) => {
                let Ok(typed) = decode_snapshot_typed_projections(&frame) else {
                    continue;
                };
                let Some(row) = find_action_lifecycle_row(&typed) else {
                    continue;
                };
                let Ok(model) = decode_action_lifecycle(&row.payload) else {
                    continue;
                };
                if pred(&model) {
                    return Some(model);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn find_action_lifecycle_row(typed: &[TypedProjectionData]) -> Option<&TypedProjectionData> {
    typed.iter().find(|t| t.key == ACTION_LIFECYCLE_SCHEMA_ID)
}

/// Full-pipeline proof: dispatching `nmp.nip17.publish_relay_list` through the
/// real `ActionRegistry` → actor → ledger → emit chain (with an un-narrowed,
/// default-declared kernel, matching the plain `nmp_core::testing::spawn_actor`
/// harness) must produce an `action_lifecycle` typed sidecar row carrying our
/// `correlation_id`'s terminal verdict. See the module doc for why this
/// dispatch resolves synchronously to a terminal (no intervening
/// `in_flight`-only frame) and for where the ACTUAL chirp#115 break lives.
#[test]
fn dispatch_publish_relay_list_populates_action_lifecycle() {
    let (cmd_tx, upd_rx) = nmp_core::testing::spawn_actor();
    cmd_tx
        .send(ActorCommand::Lifecycle(LifecycleCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
            initial_relays: Vec::new(),
        }))
        .expect("send Start");

    let registry = registry_with_publish_relay_list();
    let mut ctx = ActionContext::default();
    let action = PublishDmRelayListInput {
        relays: vec!["wss://relay.example".to_string()],
    };
    let payload = action.encode();

    let correlation_id = registry
        .start_bytes(
            &mut ctx,
            1_700_000_000_000,
            "nmp.nip17.publish_relay_list",
            &payload,
        )
        .expect("well-formed publish_relay_list payload must validate");

    registry
        .execute_bytes(
            &ctx,
            "nmp.nip17.publish_relay_list",
            &payload,
            &correlation_id,
            &|cmd| {
                cmd_tx
                    .send(cmd)
                    .expect("actor command channel must accept the enqueued Publish command");
            },
        )
        .expect("execute_bytes must enqueue exactly one ActorCommand::Publish and return Ok");

    // The dispatched action's terminal verdict (here: `Failed` from the
    // no-active-account guard) must reach the wire under our correlation_id —
    // proves `record_action_stage(Requested)` + the guard's
    // `record_action_failure_coded` both survived the full dispatch → actor →
    // ledger → emit → typed-encode chain, not just the ledger in isolation.
    let terminal = wait_for_action_lifecycle(&upd_rx, Duration::from_secs(10), |model| {
        model
            .recent_terminal
            .iter()
            .any(|row| row.correlation_id == correlation_id)
    });
    assert!(
        terminal.is_some(),
        "action_lifecycle.recent_terminal never carried a terminal verdict for \
         correlation_id {correlation_id} — the host's spinner would hang forever \
         waiting on this action (chirp#115)"
    );

    let _ = cmd_tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));
}
