//! The per-envelope §D5 gift-wrap port chain: encrypt → sign → wrap → publish.
//!
//! Each step enqueues the next via the cloned `command_sender`. [`EnvelopeChain`]
//! owns all data for one envelope and self-drives; the recipient chain's success
//! continuation launches the self-copy chain ([`SelfCopyLaunch`]). Extracted from
//! `dm_send.rs` to keep that file within its LOC ceiling.

use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::publish::PublishTarget;
use nmp_core::substrate::{
    build_nip44_encrypt_for_account, build_record_action_failure, build_sign_event_for_account,
};
use nmp_core::CommandSender;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
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
            report_envelope_failure(
                worker_tx,
                label,
                &correlation_id,
                format!("seal reparse: {e}"),
            );
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
        report_envelope_failure(
            worker_tx,
            label,
            &correlation_id,
            format!("seal verify: {e}"),
        );
        return;
    }
    let envelope = match nmp_nip59::wrap_signed_seal(&receiver, &seal_event) {
        Ok(ev) => ev,
        Err(e) => {
            report_envelope_failure(
                worker_tx,
                label,
                &correlation_id,
                format!("outer wrap: {e}"),
            );
            return;
        }
    };

    // The kind:1059 envelope is already signed by its ephemeral key — route via
    // the signed-event publish path so the kernel forwards it verbatim
    // (re-signing would destroy the unlinkability gift-wrap provides).
    let correlation_id_for_send_err = correlation_id.clone();
    if worker_tx
        .send(ActorCommand::Publish(PublishCommand::SignedEvent {
            raw: nostr_event_to_raw(&envelope),
            target: PublishTarget::Explicit {
                relays,
                route_class: nmp_core::publish::PublishRouteClass::VerifiedPrivateInbox,
            },
            correlation_id,
        }))
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
    // issue #1682 — structured token routed to the actor via `ShowErrorToken`
    // (this worker thread holds only a `CommandSender`). The envelope `label`
    // is the shell-interpolatable subject; `reason` is the raw diagnostic.
    let token = nmp_core::ui_token::UiToken::error(
        crate::ui_codes::DM_GIFTWRAP_FAILED,
        format!("cannot send DM: gift-wrap ({label}) failed: {reason}"),
    )
    .with_subject(label)
    .with_detail(reason);
    let fallback = token.fallback_prose().to_string();
    let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
    if let Some(id) = correlation_id.clone() {
        let _ = worker_tx.send(build_record_action_failure(id, fallback));
    }
}

/// Convert a `nostr::UnsignedEvent` (the kind:13 seal, freshly built on-actor)
/// to the substrate flat `UnsignedEvent` the `SignEventForAccount` port accepts.
fn nostr_unsigned_to_substrate(unsigned: &nostr::UnsignedEvent) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: unsigned.pubkey.to_hex(),
        kind: u32::from(unsigned.kind.as_u16()),
        tags: unsigned
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect(),
        content: unsigned.content.clone(),
        created_at: unsigned.created_at.as_secs(),
    }
}

/// Convert a signed `nostr::Event` (the kind:1059 gift-wrap) to the kernel's
/// flat `RawEvent`. The signature and id are carried through verbatim — the
/// signed-event publish path verifies them and forwards the event unchanged.
fn nostr_event_to_raw(event: &nostr::Event) -> nmp_store::RawEvent {
    nmp_store::RawEvent {
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
#[path = "chain_send_failure_tests.rs"]
mod send_failure_tests;
