//! The self-encrypt -> sign -> publish port chain shared by
//! `CreateCashuWalletCommand` (kind:17375) and `CashuCompleteDepositCommand`
//! (kind:7375). Mirrors `nmp_nip17::dm_send::chain`'s shape — every step
//! enqueues the next via a cloned [`CommandSender`] — but simpler: NIP-44
//! *self*-encrypts (peer == author) and publishes the already-signed event
//! directly (no NIP-59 gift-wrap).
//!
//! Using the signer-transparent ports (`Nip44EncryptForAccount` then
//! `SignEventForAccount`) instead of `nmp_nip60`'s raw-`Keys` codecs
//! (`build_wallet_event`, `build_token_event`) is the point of #2895 W2's
//! "signer-transparent NIP-44" requirement: a bunker account that cannot
//! NIP-44 fails closed through the port's own `Err` path, rather than this
//! crate ever holding or requesting a secret key (D13).

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::publish::{PublishRouteClass, PublishTarget};
use nmp_core::substrate::{
    build_nip44_encrypt_for_account, build_record_action_failure, build_sign_event_for_account,
};
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

use super::ui_codes;

/// Launch the chain for one self-encrypted NIP-60 event.
///
/// `on_signed` runs on the actor thread once the event is signed but BEFORE
/// the publish command is enqueued — the caller's chance to record its
/// durable pre-publish state (journal transition, ledger fact) against the
/// REAL event id, which is only known once signing succeeds.
#[allow(clippy::too_many_arguments)]
pub(super) fn launch_self_encrypted_publish(
    worker_tx: CommandSender,
    signer_hex: String,
    kind: u32,
    plaintext: String,
    relays: Vec<String>,
    correlation_id: Option<String>,
    on_signed: impl FnOnce(&CommandSender, &SignedEvent) + Send + 'static,
) {
    let tx_for_sign = worker_tx.clone();
    let signer_for_unsigned = signer_hex.clone();
    let correlation_for_send_err = correlation_id.clone();
    let cmd = build_nip44_encrypt_for_account(
        signer_hex.clone(),
        plaintext,
        Some(signer_hex),
        move |outcome| {
            // Runs on the actor thread (inline local / idle-drain bunker).
            // D8: only enqueues the next port step.
            let ciphertext = match outcome {
                Ok(ct) => ct,
                Err(reason) => {
                    report_chain_failure(
                        &tx_for_sign,
                        &correlation_id,
                        format!("nip44 self-encrypt: {reason}"),
                    );
                    return;
                }
            };
            let unsigned = UnsignedEvent {
                pubkey: signer_for_unsigned.clone(),
                kind,
                tags: Vec::new(),
                content: ciphertext,
                created_at: 0,
            };
            sign_and_publish(
                tx_for_sign,
                signer_for_unsigned,
                unsigned,
                relays,
                correlation_id,
                on_signed,
            );
        },
    );
    // D6 fail-loud: a dead actor inbox means the encrypt step's continuation
    // never runs, so a caller's `correlation_id` would hang forever.
    if worker_tx.send(cmd).is_err() {
        report_chain_failure(
            &worker_tx,
            &correlation_for_send_err,
            "actor inbox closed before nip44 self-encrypt".to_string(),
        );
    }
}

fn sign_and_publish(
    worker_tx: CommandSender,
    signer_hex: String,
    unsigned: UnsignedEvent,
    relays: Vec<String>,
    correlation_id: Option<String>,
    on_signed: impl FnOnce(&CommandSender, &SignedEvent) + Send + 'static,
) {
    let tx_for_publish = worker_tx.clone();
    let correlation_for_send_err = correlation_id.clone();
    let cmd = build_sign_event_for_account(unsigned, Some(signer_hex), move |outcome| {
        let signed = match outcome {
            Ok(signed) => signed,
            Err(reason) => {
                report_chain_failure(&tx_for_publish, &correlation_id, format!("sign: {reason}"));
                return;
            }
        };
        on_signed(&tx_for_publish, &signed);
        let raw = nmp_store::RawEvent {
            id: signed.id.clone(),
            pubkey: signed.unsigned.pubkey.clone(),
            created_at: signed.unsigned.created_at,
            kind: signed.unsigned.kind,
            tags: signed.unsigned.tags.clone(),
            content: signed.unsigned.content.clone(),
            sig: signed.sig.clone(),
        };
        let _ = tx_for_publish.send(ActorCommand::Publish(PublishCommand::SignedEvent {
            raw,
            // A pre-signed publish must route through an explicit relay set
            // (the kernel rejects `Auto` for `SignedEvent` — presigned
            // publish is not the normal app write path). `ImportedOrPresigned`
            // is the closest-fit route class: this command pre-signs through
            // the port rather than letting the publish pipeline sign.
            target: PublishTarget::Explicit {
                relays,
                route_class: PublishRouteClass::ImportedOrPresigned,
            },
            correlation_id,
        }));
    });
    if worker_tx.send(cmd).is_err() {
        report_chain_failure(
            &worker_tx,
            &correlation_for_send_err,
            "actor inbox closed before sign".to_string(),
        );
    }
}

fn report_chain_failure(
    worker_tx: &CommandSender,
    correlation_id: &Option<String>,
    reason: String,
) {
    let token =
        UiToken::error(ui_codes::OPERATION_FAILED, reason.clone()).with_detail(reason.clone());
    let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
    if let Some(id) = correlation_id.clone() {
        let _ = worker_tx.send(build_record_action_failure(id, reason));
    }
}
