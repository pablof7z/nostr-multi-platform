//! Tests for [`super`]'s WOT bootstrap runtime: interest emission, account
//! switching, and the typed-snapshot sidecar (Wave A, ADR-0037).

use super::*;
use crate::interest::KIND_MUTE_LIST;
use nmp_planner::InterestLifecycle;
use nmp_core::slots::{new_active_account_slot, ActiveAccountSlot};
use nostr::Keys;

/// ADR-0050 §D3a — the runtime now sends through a `CommandSender` over an
/// `ActorMail` inbox. Build the pair and unwrap commands for the assertions.
fn wot_channel() -> (
    nmp_core::CommandSender,
    std::sync::mpsc::Receiver<nmp_core::ActorMail>,
) {
    let (tx, rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    (nmp_core::CommandSender::new(tx), rx)
}

fn unwrap_mail(mail: nmp_core::ActorMail) -> ActorCommand {
    match mail {
        nmp_core::ActorMail::Command(cmd) => cmd,
        other => panic!("expected ActorMail::Command, got {other:?}"),
    }
}

fn author(n: u16) -> String {
    format!("{n:064x}")
}

/// Build a pubkey-only active-account slot holding `keys`' hex pubkey.
///
/// This is the canonical fixture: the slot carries ONLY the hex pubkey
/// string (`ActiveAccountSlot`), exactly as the kernel populates it for
/// EVERY backend including bunker (remote-signer) accounts. The secret
/// `nostr::Keys` are never present — proving the runtime activates from
/// identity alone.
fn active_slot(keys: &Keys) -> ActiveAccountSlot {
    let slot = new_active_account_slot();
    *slot.lock().unwrap() = Some(keys.public_key().to_hex());
    slot
}

fn contact_event(event_author: &str, follows: usize) -> KernelEvent {
    KernelEvent {
        id: nmp_core::substrate::EventId::from("1".repeat(64)),
        author: event_author.to_string(),
        kind: KIND_CONTACT_LIST,
        created_at: 1_000,
        tags: (0..follows)
            .map(|i| vec!["p".to_string(), author(i as u16)])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn contact_event_with_pubkeys(event_author: &str, follows: &[&str]) -> KernelEvent {
    KernelEvent {
        id: nmp_core::substrate::EventId::from("2".repeat(64)),
        author: event_author.to_string(),
        kind: KIND_CONTACT_LIST,
        created_at: 1_000,
        tags: follows
            .iter()
            .map(|pubkey| vec!["p".to_string(), (*pubkey).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn mute_event(event_author: &str, mutes: &[&str]) -> KernelEvent {
    KernelEvent {
        id: nmp_core::substrate::EventId::from("3".repeat(64)),
        author: event_author.to_string(),
        kind: KIND_MUTE_LIST,
        created_at: 1_000,
        tags: mutes
            .iter()
            .map(|pubkey| vec!["p".to_string(), (*pubkey).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

/// Finding C — bunker (remote-signer-only) accounts must activate WOT
/// bootstrap. The kernel writes the active pubkey into `ActiveAccountSlot`
/// for every backend, including bunker, while the local-keys slot stays
/// `None` (no secret material in-process). This proves the runtime resolves
/// its active pubkey and pushes the bootstrap interest from the pubkey slot
/// alone — the whole point of the least-privilege identity accessor.
#[test]
fn bunker_only_account_activates_wot_bootstrap() {
    let keys = Keys::generate();
    let active = keys.public_key().to_hex();
    let (tx, rx) = wot_channel();
    // Pubkey-only slot: Some(hex), zero secret keys — the bunker shape.
    let runtime = WotBootstrapRuntime::new(active_slot(&keys), tx);

    runtime.on_kernel_event(&contact_event(&active, 8));

    let cmd = unwrap_mail(
        rx.recv()
            .expect("bunker account must still push WOT bootstrap"),
    );
    let ActorCommand::PushInterest(interest) = cmd else {
        panic!("expected PushInterest for a bunker-only account");
    };
    assert_eq!(interest.id, active_follow_graph_interest_id());
    assert_eq!(interest.shape.authors.len(), 8);
}

#[test]
fn active_kind3_pushes_large_one_shot_wot_interest() {
    let keys = Keys::generate();
    let active = keys.public_key().to_hex();
    let (tx, rx) = wot_channel();
    let runtime = WotBootstrapRuntime::new(active_slot(&keys), tx);

    runtime.on_kernel_event(&contact_event(&active, 1_052));

    let cmd = unwrap_mail(rx.recv().expect("wot bootstrap command"));
    let ActorCommand::PushInterest(interest) = cmd else {
        panic!("expected PushInterest");
    };
    assert_eq!(interest.id, active_follow_graph_interest_id());
    assert!(matches!(interest.lifecycle, InterestLifecycle::OneShot));
    assert_eq!(interest.shape.limit, None);
    assert_eq!(interest.shape.authors.len(), 1_052);
    assert_eq!(
        interest.shape.kinds.into_iter().collect::<Vec<_>>(),
        crate::interest::WOT_BOOTSTRAP_KINDS
    );
}

#[test]
fn account_switch_snapshot_withdraws_previous_bootstrap() {
    let keys = Keys::generate();
    let active = keys.public_key().to_hex();
    let (tx, rx) = wot_channel();
    let slot = active_slot(&keys);
    let runtime = WotBootstrapRuntime::new(Arc::clone(&slot), tx);

    runtime.on_kernel_event(&contact_event(&active, 30));
    let _ = unwrap_mail(rx.recv().expect("initial push"));
    *slot.lock().unwrap() = None;

    // Both registered projection closures call `current_snapshot()` every tick.
    // The account-switch withdraw is fired from that shared path, so it MUST be
    // idempotent across the two calls: the generic projection reconciles the
    // switch, and the typed projection (running second in the same tick) sees
    // already-reconciled state and does not re-fire.
    let _ = runtime.snapshot_json();
    let _ = runtime.snapshot_typed();

    let cmd = unwrap_mail(rx.recv().expect("withdraw command"));
    let ActorCommand::WithdrawInterest(id) = cmd else {
        panic!("expected WithdrawInterest");
    };
    assert_eq!(id, active_follow_graph_interest_id());
    assert!(
        rx.try_recv().is_err(),
        "the account-switch withdraw must fire exactly once across both projection closures"
    );
}

/// Wave A proof (runtime layer): the registered typed closure body
/// (`snapshot_typed`) yields the typed-sidecar entry whose payload decodes
/// back to the live runtime state — driven through a real `on_kernel_event`
/// ingest, not a hand-built struct.
#[test]
fn snapshot_typed_lands_live_runtime_state_in_the_sidecar_and_round_trips() {
    let keys = Keys::generate();
    let active = keys.public_key().to_hex();
    let (tx, _rx) = wot_channel();
    let runtime = WotBootstrapRuntime::new(active_slot(&keys), tx);

    runtime.on_kernel_event(&contact_event(&active, 12));

    let entry = runtime
        .snapshot_typed()
        .expect("a non-poisoned runtime must emit a typed sidecar entry");
    assert_eq!(entry.key, "nmp.wot.bootstrap");
    assert_eq!(entry.schema_id, crate::wire::typed_fb::SCHEMA_ID);
    assert_eq!(entry.file_identifier, "NWBS");

    let decoded = crate::wire::typed_fb::decode_wot_bootstrap(&entry.payload)
        .expect("sidecar payload must decode as NWBS");
    assert_eq!(decoded.active_pubkey.as_deref(), Some(active.as_str()));
    assert_eq!(decoded.active_follow_count, 12);
    assert!(decoded.bootstrap_requested);
    // One follow-list event was ingested (from the active author), so the
    // graph records exactly one distinct follow-edge author.
    assert_eq!(decoded.graph_follow_authors, 1);
}

/// The typed sidecar emits in lock-step with the generic projection: it is
/// `Some` whenever — and only whenever — `snapshot_json` is non-Null. (The
/// happy path; both only yield `None`/`Null` on a poisoned lock.)
#[test]
fn typed_and_json_projections_emit_in_lockstep() {
    let keys = Keys::generate();
    let (tx, _rx) = wot_channel();
    let runtime = WotBootstrapRuntime::new(active_slot(&keys), tx);

    // No account event yet: generic projection is still non-Null (active
    // pubkey present, zero counts), so the typed sidecar must also emit.
    assert!(!runtime.snapshot_json().is_null());
    assert!(runtime.snapshot_typed().is_some());
}

#[test]
fn runtime_read_handle_scores_the_ingested_graph() {
    let keys = Keys::generate();
    let active = keys.public_key().to_hex();
    let alice = author(10);
    let bob = author(11);
    let candidate = author(12);
    let muted = author(13);
    let unknown = author(14);
    let (tx, _rx) = wot_channel();
    let runtime = WotBootstrapRuntime::new(active_slot(&keys), tx);

    runtime.on_kernel_event(&contact_event_with_pubkeys(&active, &[&alice, &bob]));
    runtime.on_kernel_event(&contact_event_with_pubkeys(&alice, &[&candidate]));
    runtime.on_kernel_event(&contact_event_with_pubkeys(&bob, &[&candidate]));
    runtime.on_kernel_event(&mute_event(&active, &[&muted]));

    let decision = runtime
        .score(&active, &candidate)
        .expect("runtime lock should not be poisoned");
    assert_eq!(decision.score, 20);
    assert_eq!(decision.reason, "second-degree");
    assert!(!decision.hide);

    let strict = runtime
        .score_with_minimum_score(&active, &candidate, 30)
        .expect("runtime lock should not be poisoned");
    assert!(strict.hide);

    let muted_decision = runtime
        .score(&active, &muted)
        .expect("runtime lock should not be poisoned");
    assert_eq!(muted_decision.reason, "muted-by-self");
    assert!(muted_decision.hide);

    let candidates = vec![candidate.clone(), muted.clone(), unknown.clone()];
    let batch = runtime
        .batch_score(&active, &candidates)
        .expect("runtime lock should not be poisoned");
    assert_eq!(
        batch.iter().map(|d| d.reason).collect::<Vec<_>>(),
        vec!["second-degree", "muted-by-self", "unknown"]
    );

    assert_eq!(
        runtime
            .mutual_follows(&active, &candidate)
            .expect("runtime lock should not be poisoned"),
        vec![alice, bob]
    );
    assert_eq!(
        runtime
            .graph_stats()
            .expect("runtime lock should not be poisoned"),
        WotGraphStats {
            follow_authors: 3,
            mute_authors: 1,
        }
    );
}
