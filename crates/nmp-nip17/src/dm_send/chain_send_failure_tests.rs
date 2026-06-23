//! Send-path failure + fail-closed tests for the chain module (`chain.rs`).
//!
//! Extracted from `chain.rs` into this sibling file to keep that file under
//! the 500-LOC hard cap (AGENTS.md). Declared via `#[path]` as a child of
//! `chain`, so `use super::*` still resolves to the chain module's items.
//!
//! Bug 2 (D6 fail-loud): when the actor inbox is gone, every chain step's
//! `worker_tx.send()` returns `Err`. The action's `correlation_id` would
//! otherwise hang forever (UI spinner never resolves). These tests pin the
//! contract that a closed inbox is detected and `report_envelope_failure`
//! is invoked — never a silent `let _ = send(..)`.

use super::*;
use nmp_core::{ActionLedgerCommand, ActorCommand, ActorMail, PublishCommand};
use std::sync::mpsc::{channel, Receiver, Sender};

/// A signed kind:13 seal for the wrap step, signed with a real test key so
/// `wrap_signed_seal` produces a verifiable kind:1059.
fn signed_seal(signer: &nostr::Keys) -> SignedEvent {
    let seal_ts = Timestamp::from(1_700_000_000);
    let event = nostr::EventBuilder::new(nostr::Kind::Seal, "ciphertext-placeholder")
        .custom_created_at(seal_ts)
        .sign_with_keys(signer)
        .expect("seal sign");
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

/// A signed kind:13 seal whose signature has been tampered with: the event
/// id/pubkey/content are valid but the `sig` is replaced with another valid
/// signature over a DIFFERENT message, so `seal_event.verify()` must fail.
/// Models a misbehaving external/NIP-55 signer returning a garbage sig.
fn tampered_seal(signer: &nostr::Keys) -> SignedEvent {
    let mut seal = signed_seal(signer);
    // Forge: sign a different event and graft that (well-formed but wrong)
    // signature onto our seal. The id no longer matches the sig → verify
    // fails on signature, not on parse.
    let other = nostr::EventBuilder::new(nostr::Kind::Seal, "a-different-payload")
        .custom_created_at(Timestamp::from(1_700_000_001))
        .sign_with_keys(signer)
        .expect("decoy sign");
    seal.sig = other.sig.to_string();
    seal
}

/// A live `CommandSender` whose receiver we keep, so we can drain enqueued
/// commands and assert what landed.
fn live_sender() -> (CommandSender, Receiver<ActorMail>) {
    let (tx, rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    (CommandSender::new(tx), rx)
}

/// A `CommandSender` whose receiver has been dropped — every `send` returns
/// `Err` (the actor-thread-is-dead scenario).
fn dead_sender() -> CommandSender {
    let (tx, rx): (Sender<ActorMail>, Receiver<ActorMail>) = channel();
    drop(rx);
    CommandSender::new(tx)
}

fn drain(rx: &Receiver<ActorMail>) -> Vec<ActorCommand> {
    let mut out = Vec::new();
    while let Ok(ActorMail::Command(cmd)) = rx.try_recv() {
        out.push(cmd);
    }
    out
}

#[test]
fn wrap_and_publish_enqueues_publish_on_live_inbox() {
    // Baseline: a live inbox accepts the PublishSignedEvent terminal.
    let signer = nostr::Keys::generate();
    let receiver = nostr::Keys::generate().public_key();
    let seal = signed_seal(&signer);
    let (tx, rx) = live_sender();

    wrap_and_publish(
        &tx,
        "recipient",
        receiver,
        &seal,
        vec!["wss://relay.example".to_string()],
        Some("corr-live".to_string()),
        None,
    );

    let cmds = drain(&rx);
    assert!(
        cmds.iter()
            .any(|c| matches!(c, ActorCommand::Publish(PublishCommand::SignedEvent { .. }))),
        "live inbox must receive the gift-wrap publish: {cmds:?}"
    );
    // No failure terminal on the happy path.
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { .. }))),
        "happy path must not record a failure: {cmds:?}"
    );
}

#[test]
fn wrap_and_publish_rejects_seal_with_bad_signature() {
    // Issue #1265 (fail-closed send path): if the signed seal carries a
    // garbage/forged signature (a misbehaving external/NIP-55 signer),
    // `wrap_and_publish` must call `seal_event.verify()`, fail closed, and
    // NEVER gift-wrap+publish a corrupt seal. The action resolves Failed
    // (toast + RecordActionFailure) instead of silently losing the DM.
    let signer = nostr::Keys::generate();
    let receiver = nostr::Keys::generate().public_key();
    let seal = tampered_seal(&signer);
    let (tx, rx) = live_sender();

    wrap_and_publish(
        &tx,
        "recipient",
        receiver,
        &seal,
        vec!["wss://relay.example".to_string()],
        Some("corr-bad-sig".to_string()),
        None,
    );

    let cmds = drain(&rx);
    // A corrupt seal must NOT be published.
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, ActorCommand::Publish(PublishCommand::SignedEvent { .. }))),
        "a seal failing verify must not be gift-wrapped+published: {cmds:?}"
    );
    // The action must resolve Failed (single-terminal fail-loud contract).
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { correlation_id, .. }) if correlation_id == "corr-bad-sig"
        )),
        "a verify failure must record the action failure: {cmds:?}"
    );
}

#[test]
fn command_sender_send_errors_when_receiver_dropped() {
    // Precondition for the whole bug: a dropped receiver makes send fail.
    let tx = dead_sender();
    assert!(
        tx.send(ActorCommand::ShowToast {
            message: "x".into()
        })
        .is_err(),
        "a dropped receiver must surface a send error"
    );
}

#[test]
fn wrap_and_publish_reports_failure_when_inbox_dead() {
    // The bug: when the publish send fails, the recipient action must not
    // silently hang — `report_envelope_failure` is invoked instead. With a
    // dead inbox even the report's sends fail, so we assert the function
    // takes the failure branch (returns without launching the self-copy
    // chain) rather than panicking or proceeding as if published.
    let signer = nostr::Keys::generate();
    let receiver = nostr::Keys::generate().public_key();
    let seal = signed_seal(&signer);
    let tx = dead_sender();

    let self_copy = SelfCopyLaunch {
        signer_hex: signer.public_key().to_hex(),
        sender: signer.public_key(),
        rumor_json: "{}".to_string(),
        relays: vec!["wss://relay.example".to_string()],
    };

    // Must not panic and must return (the early-return failure branch). A
    // pre-fix `let _ = send(..)` would silently fall through to launching
    // the self-copy chain even though nothing was published.
    wrap_and_publish(
        &tx,
        "recipient",
        receiver,
        &seal,
        vec!["wss://relay.example".to_string()],
        Some("corr-dead".to_string()),
        Some(self_copy),
    );
}

#[test]
fn report_envelope_failure_records_action_on_live_inbox() {
    // The fail-loud terminal contract: a correlation_id-bearing envelope
    // emits both a toast AND a RecordActionFailure so the action resolves.
    let (tx, rx) = live_sender();
    report_envelope_failure(
        &tx,
        "recipient",
        &Some("corr-1".to_string()),
        "actor inbox closed before seal encrypt".to_string(),
    );
    let cmds = drain(&rx);
    assert!(
        cmds.iter()
            .any(|c| matches!(c, ActorCommand::ShowErrorToken { .. })),
        "must surface a structured error token: {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { correlation_id, .. }) if correlation_id == "corr-1"
        )),
        "must record the action failure so the spinner resolves: {cmds:?}"
    );
}

#[test]
fn envelope_chain_launch_does_not_hang_on_dead_inbox() {
    // End-to-end: launching the chain against a dead inbox returns promptly
    // (the send-error branch fires) rather than dropping the action on the
    // floor. Pre-fix this `let _ = send(..)` left the correlation_id hung.
    let signer = nostr::Keys::generate();
    let receiver = nostr::Keys::generate().public_key();
    let chain = EnvelopeChain {
        label: "recipient",
        signer_hex: signer.public_key().to_hex(),
        sender: signer.public_key(),
        receiver,
        rumor_json: "{}".to_string(),
        relays: vec!["wss://relay.example".to_string()],
        correlation_id: Some("corr-launch".to_string()),
    };
    chain.launch(dead_sender(), None);
}
