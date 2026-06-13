//! The per-envelope §D5 gift-wrap port chain: encrypt → sign → wrap → publish.
//!
//! Each step enqueues the next via the cloned `command_sender`. [`EnvelopeChain`]
//! owns all data for one envelope and self-drives; the recipient chain's success
//! continuation launches the self-copy chain ([`SelfCopyLaunch`]). Extracted from
//! `dm_send.rs` to keep that file within its LOC ceiling.

use nmp_core::publish::PublishTarget;
use nmp_core::substrate::{
    build_nip44_encrypt_for_account, build_sign_event_for_account, SignedEvent, UnsignedEvent,
};
use nmp_core::{ActorCommand, CommandSender};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::{JsonUtil, PublicKey, Timestamp};

/// Owned data + launcher for ONE envelope's port chain (encrypt → sign → wrap →
/// publish). Self-contained: every step enqueues the next via `worker_tx`.
pub(super) struct EnvelopeChain {
    /// `"recipient"` or `"self-copy"` — for D6 toast wording.
    pub(super) label: &'static str,
    /// The pinned signing account (§D5) — passed to every port step.
    pub(super) signer_hex: String,
    /// The sender (seal author) pubkey.
    pub(super) sender: PublicKey,
    /// This envelope's gift-wrap receiver.
    pub(super) receiver: PublicKey,
    /// The seal-content plaintext (the rumor JSON).
    pub(super) rumor_json: String,
    /// This receiver's kind:10050 DM-inbox relays (non-empty — gated up front).
    pub(super) relays: Vec<String>,
    /// `Some(id)` for the recipient envelope (carries the action terminal);
    /// `None` for the self-copy (background delivery only).
    pub(super) correlation_id: Option<String>,
}

/// Data needed to launch the self-copy chain once the recipient chain succeeds.
pub(super) struct SelfCopyLaunch {
    pub(super) signer_hex: String,
    pub(super) sender: PublicKey,
    pub(super) rumor_json: String,
    pub(super) relays: Vec<String>,
}

impl EnvelopeChain {
    /// Step 1 — enqueue `Nip44EncryptForAccount`. Its continuation runs steps
    /// 2–3 + publish, then (recipient only) launches the self-copy chain.
    pub(super) fn launch(self, worker_tx: CommandSender, on_success: Option<SelfCopyLaunch>) {
        let EnvelopeChain {
            label,
            signer_hex,
            sender,
            receiver,
            rumor_json,
            relays,
            correlation_id,
        } = self;

        let tx_for_seal = worker_tx.clone();
        // Retained for the fail-loud path below: the closure moves
        // `correlation_id`, but if the send itself fails the closure never
        // runs, so we need our own copy to report the hung action.
        let correlation_id_for_send_err = correlation_id.clone();
        let cmd = build_nip44_encrypt_for_account(
            receiver.to_hex(),
            rumor_json,
            Some(signer_hex.clone()),
            move |outcome| {
                // Runs on the actor thread (inline local / idle-drain bunker).
                // D8: only enqueues the next port step.
                let ciphertext = match outcome {
                    Ok(ct) => ct,
                    Err(reason) => {
                        report_envelope_failure(
                            &tx_for_seal,
                            label,
                            &correlation_id,
                            format!("seal encrypt: {reason}"),
                        );
                        return;
                    }
                };
                seal_and_sign(
                    tx_for_seal,
                    label,
                    signer_hex,
                    sender,
                    receiver,
                    ciphertext,
                    relays,
                    correlation_id,
                    on_success,
                );
            },
        );
        // D6 fail-loud: a dead actor inbox means the encrypt step's
        // continuation never runs, so the action's `correlation_id` would hang
        // forever (UI spinner never resolves). Report the failure so the action
        // resolves `Failed`.
        if worker_tx.send(cmd).is_err() {
            report_envelope_failure(
                &worker_tx,
                label,
                &correlation_id_for_send_err,
                "actor inbox closed before seal encrypt".to_string(),
            );
        }
    }
}

/// Step 2 — build the kind:13 seal `UnsignedEvent` (pure) and enqueue
/// `SignEventForAccount`. Its continuation runs step 3 (wrap + publish).
#[allow(clippy::too_many_arguments)]
fn seal_and_sign(
    worker_tx: CommandSender,
    label: &'static str,
    signer_hex: String,
    sender: PublicKey,
    receiver: PublicKey,
    ciphertext: String,
    relays: Vec<String>,
    correlation_id: Option<String>,
    on_success: Option<SelfCopyLaunch>,
) {
    // The seal's `created_at` is an independent NIP-59 tweak (the kind:1059 wrap
    // draws its OWN tweak inside `wrap_signed_seal`).
    let seal_ts = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    let seal_unsigned = nmp_nip59::build_seal_unsigned(sender, ciphertext, seal_ts);
    let seal_substrate = nostr_unsigned_to_substrate(&seal_unsigned);

    let tx_for_wrap = worker_tx.clone();
    // Retained for the fail-loud send path: the closure moves `correlation_id`.
    let correlation_id_for_send_err = correlation_id.clone();
    let cmd = build_sign_event_for_account(
        seal_substrate,
        Some(signer_hex), // §D5 pin — never None.
        move |outcome| {
            let signed_seal = match outcome {
                Ok(signed) => signed,
                Err(reason) => {
                    report_envelope_failure(
                        &tx_for_wrap,
                        label,
                        &correlation_id,
                        format!("seal sign: {reason}"),
                    );
                    return;
                }
            };
            wrap_and_publish(
                &tx_for_wrap,
                label,
                receiver,
                &signed_seal,
                relays,
                correlation_id,
                on_success,
            );
        },
    );
    // D6 fail-loud: a dead inbox means the sign step's continuation never runs,
    // so the action would hang. Report so it resolves `Failed`.
    if worker_tx.send(cmd).is_err() {
        report_envelope_failure(
            &worker_tx,
            label,
            &correlation_id_for_send_err,
            "actor inbox closed before seal sign".to_string(),
        );
    }
}

/// Step 3 — assemble the kind:1059 wrap (pure, fresh ephemeral key in-process)
/// and enqueue `PublishSignedEvent`. On the recipient envelope's success, launch
/// the self-copy chain.
fn wrap_and_publish(
    worker_tx: &CommandSender,
    label: &'static str,
    receiver: PublicKey,
    signed_seal: &SignedEvent,
    relays: Vec<String>,
    correlation_id: Option<String>,
    on_success: Option<SelfCopyLaunch>,
) {
    // Reconstruct the signed kind:13 seal as a `nostr::Event` from the flat
    // NIP-01 JSON so the pure `wrap_signed_seal` can ephemeral-wrap it.
    let seal_event = match nostr::Event::from_json(signed_seal.to_nip01_json()) {
        Ok(ev) => ev,
        Err(e) => {
            report_envelope_failure(worker_tx, label, &correlation_id, format!("seal reparse: {e}"));
            return;
        }
    };
    // Fail-closed (issue #1265): verify the seal's signature BEFORE gift-wrapping.
    // A misbehaving external/NIP-55 signer can return a malformed/garbage sig; an
    // unverified seal would gift-wrap+publish a corrupt event the recipient cannot
    // decrypt (the DM is silently lost). Mirror the reparse-failure arm — resolve
    // the action Failed with a D6 toast — symmetric with `parse_seal_for_decrypt`
    // on the inbox side.
    if let Err(e) = seal_event.verify() {
        report_envelope_failure(worker_tx, label, &correlation_id, format!("seal verify: {e}"));
        return;
    }
    let envelope = match nmp_nip59::wrap_signed_seal(&receiver, &seal_event) {
        Ok(ev) => ev,
        Err(e) => {
            report_envelope_failure(worker_tx, label, &correlation_id, format!("outer wrap: {e}"));
            return;
        }
    };

    // The kind:1059 envelope is already signed by its ephemeral key — route via
    // the signed-event publish path so the kernel forwards it verbatim
    // (re-signing would destroy the unlinkability gift-wrap provides).
    let correlation_id_for_send_err = correlation_id.clone();
    if worker_tx
        .send(ActorCommand::PublishSignedEvent {
            raw: nostr_event_to_raw(&envelope),
            target: PublishTarget::Explicit { relays },
            correlation_id,
        })
        .is_err()
    {
        // D6 fail-loud: the publish command never landed, so the action would
        // hang. Report so it resolves `Failed`. (`report_envelope_failure`'s
        // own sends will also fail on a dead inbox, but reporting is correct;
        // the dead-inbox case is terminal regardless.)
        report_envelope_failure(
            worker_tx,
            label,
            &correlation_id_for_send_err,
            "actor inbox closed before gift-wrap publish".to_string(),
        );
        return;
    }

    // Recipient success → launch the self-copy chain (sequential ordering).
    if let Some(self_copy) = on_success {
        let SelfCopyLaunch {
            signer_hex,
            sender,
            rumor_json,
            relays,
        } = self_copy;
        EnvelopeChain {
            label: "self-copy",
            signer_hex,
            sender,
            receiver: sender, // self-copy gift-wraps to the sender's own pubkey.
            rumor_json,
            relays,
            correlation_id: None, // background delivery — never the action verdict.
        }
        .launch(worker_tx.clone(), None);
    }
}

/// In-continuation (post-`ctx`, on the actor thread) failure: surface a D6 toast
/// via the command sender, and — only when this envelope carries a
/// `correlation_id` (i.e. the recipient envelope) — record the action failure.
/// A self-copy failure is background: toast only (the recipient already got the
/// message, so the action promise is satisfied — single-terminal contract).
pub(super) fn report_envelope_failure(
    worker_tx: &CommandSender,
    label: &'static str,
    correlation_id: &Option<String>,
    reason: String,
) {
    let toast = format!("cannot send DM: gift-wrap ({label}) failed: {reason}");
    let _ = worker_tx.send(ActorCommand::ShowToast {
        message: toast.clone(),
    });
    if let Some(id) = correlation_id.clone() {
        let _ = worker_tx.send(ActorCommand::RecordActionFailure {
            correlation_id: id,
            reason: toast,
        });
    }
}

/// Convert a `nostr::UnsignedEvent` (the kind:13 seal, freshly built on-actor)
/// to the substrate flat `UnsignedEvent` the `SignEventForAccount` port accepts.
fn nostr_unsigned_to_substrate(unsigned: &nostr::UnsignedEvent) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: unsigned.pubkey.to_hex(),
        kind: u32::from(unsigned.kind.as_u16()),
        tags: unsigned.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: unsigned.content.clone(),
        created_at: unsigned.created_at.as_secs(),
    }
}

/// Convert a signed `nostr::Event` (the kind:1059 gift-wrap) to the kernel's
/// flat `RawEvent`. The signature and id are carried through verbatim — the
/// signed-event publish path verifies them and forwards the event unchanged.
fn nostr_event_to_raw(event: &nostr::Event) -> nmp_core::store::RawEvent {
    nmp_core::store::RawEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: u32::from(event.kind.as_u16()),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}

#[cfg(test)]
mod send_failure_tests {
    //! Bug 2 (D6 fail-loud): when the actor inbox is gone, every chain step's
    //! `worker_tx.send()` returns `Err`. The action's `correlation_id` would
    //! otherwise hang forever (UI spinner never resolves). These tests pin the
    //! contract that a closed inbox is detected and `report_envelope_failure`
    //! is invoked — never a silent `let _ = send(..)`.

    use super::*;
    use nmp_core::ActorMail;
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
                .any(|c| matches!(c, ActorCommand::PublishSignedEvent { .. })),
            "live inbox must receive the gift-wrap publish: {cmds:?}"
        );
        // No failure terminal on the happy path.
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, ActorCommand::RecordActionFailure { .. })),
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
                .any(|c| matches!(c, ActorCommand::PublishSignedEvent { .. })),
            "a seal failing verify must not be gift-wrapped+published: {cmds:?}"
        );
        // The action must resolve Failed (single-terminal fail-loud contract).
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                ActorCommand::RecordActionFailure { correlation_id, .. } if correlation_id == "corr-bad-sig"
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
                .any(|c| matches!(c, ActorCommand::ShowToast { .. })),
            "must surface a toast: {cmds:?}"
        );
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                ActorCommand::RecordActionFailure { correlation_id, .. } if correlation_id == "corr-1"
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
}
