//! Dispatch helpers for the ADR-0050 signer-session capability port verbs —
//! the NIP-44 cipher port (§D1) and the mailbox-completion delivery (§D3b).
//!
//! These are the bodies of the `Nip44EncryptForAccount` /
//! `Nip44DecryptForAccount` / `DeliverSignerResponse` `dispatch_command` arms,
//! extracted here to keep `dispatch.rs` within its file-size budget. The arms in
//! `dispatch.rs` stay thin (call one helper, then `maybe_emit_after_dispatch`).

use super::commands;
use super::dispatch::ActorContext;
use super::pending_sign::ParkedOp;
use super::tick::maybe_emit_after_dispatch;
use super::CipherContinuation;
use crate::relay::OutboundMessage;

/// Resolve a NIP-44 cipher op (the shared body of the encrypt / decrypt arms,
/// §D1).
///
/// `cipher_result` is the `nip44_{encrypt,decrypt}_nonblocking` outcome. A local
/// account resolves `Ready` and the continuation runs INLINE on the actor
/// thread; a remote (NIP-46 / NIP-55) account resolves `Pending` and is parked
/// under the `CipherContinuation` sink with the SIGNING account's per-op
/// deadline (§D4). A setup error (no account, bad peer pubkey) resolves the
/// continuation with `Err` so the worker's failure path runs (D6). Local-vs-
/// bunker is invisible; D13 — only ciphertext/plaintext crosses the boundary.
pub(super) fn dispatch_cipher_op(
    ctx: &mut ActorContext,
    cipher_result: Result<nmp_signer_iface::SignerOp<String>, String>,
    signer_pubkey: Option<&str>,
    continuation: CipherContinuation,
) {
    match cipher_result {
        Err(reason) => continuation.call(Err(reason)),
        Ok(mut op) => match op.poll() {
            Some(Ok(text)) => continuation.call(Ok(text)),
            Some(Err(e)) => continuation.call(Err(e.to_string())),
            None => {
                let deadline = ctx.identity.sign_deadline_for(signer_pubkey);
                ctx.parked_ops
                    .push(ParkedOp::cipher_continuation(op, continuation, deadline));
            }
        },
    }
}

/// Full `SignEventForAccount` arm (ADR-0043 Decision 2 generic sign port).
/// Sign with the active (`signer_pubkey == None`) or named account, then deliver
/// the resolved `SignedEvent` (or error) to the boxed continuation. Local-vs-
/// bunker is invisible (D8); the continuation is the sole consumer (D0 — core
/// never parses it); only a `SignedEvent` crosses (D13). A local key resolves
/// `Ready` inline; a remote signer parks under the `SignContinuation` sink with
/// the NAMED account's per-op deadline (ADR-0050 §D4).
pub(super) fn sign_for_account(
    ctx: &mut ActorContext,
    unsigned: &crate::substrate::UnsignedEvent,
    signer_pubkey: Option<String>,
    continuation: super::SignContinuation,
) -> Option<Vec<OutboundMessage>> {
    let sign_result = match &signer_pubkey {
        None => commands::sign_active_nonblocking(ctx.identity, unsigned),
        Some(pk) => commands::sign_with_account_nonblocking(ctx.identity, pk, unsigned),
    };
    match sign_result {
        // No account / no signer: resolve the continuation with the error so the
        // worker's failure path runs and the host spinner clears (D6).
        Err(reason) => continuation.call(Err(reason)),
        Ok(mut op) => match op.poll() {
            Some(Ok(signed)) => continuation.call(Ok(signed)),
            Some(Err(e)) => continuation.call(Err(e.to_string())),
            None => {
                let deadline = ctx.identity.sign_deadline_for(signer_pubkey.as_deref());
                ctx.parked_ops
                    .push(ParkedOp::sign_continuation(op, continuation, deadline));
            }
        },
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Full `Nip44EncryptForAccount` arm (§D1) — encrypt through the runtime's
/// non-blocking NIP-44 helper, resolve via [`dispatch_cipher_op`], emit, and
/// return the (always-empty) outbound so the dispatch arm is a one-liner.
pub(super) fn nip44_encrypt_for_account(
    ctx: &mut ActorContext,
    peer_pubkey: &str,
    plaintext: &str,
    signer_pubkey: Option<String>,
    continuation: CipherContinuation,
) -> Option<Vec<OutboundMessage>> {
    let result = commands::nip44_encrypt_nonblocking(
        ctx.identity,
        signer_pubkey.as_deref(),
        peer_pubkey,
        plaintext,
    );
    dispatch_cipher_op(ctx, result, signer_pubkey.as_deref(), continuation);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Full `Nip44DecryptForAccount` arm (§D1) — the inbound twin of
/// [`nip44_encrypt_for_account`].
pub(super) fn nip44_decrypt_for_account(
    ctx: &mut ActorContext,
    peer_pubkey: &str,
    ciphertext: &str,
    signer_pubkey: Option<String>,
    continuation: CipherContinuation,
) -> Option<Vec<OutboundMessage>> {
    let result = commands::nip44_decrypt_nonblocking(
        ctx.identity,
        signer_pubkey.as_deref(),
        peer_pubkey,
        ciphertext,
    );
    dispatch_cipher_op(ctx, result, signer_pubkey.as_deref(), continuation);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Full `DeliverSignerResponse` arm (§D3b) — fan the inbound response out to
/// every remote handle for correlation-keyed dispatch (each drops a non-matching
/// id — the trait contract). One writer (D4, actor thread). The command arrived
/// on the single waking inbox (§D3a) and the parked-op drain runs unconditionally
/// after the command lane this iteration, so the resolved op is picked up the
/// same tick — no ≤250ms idle-tick dependence.
pub(super) fn deliver_signer_response(
    ctx: &mut ActorContext,
    response_json: &str,
) -> Option<Vec<OutboundMessage>> {
    ctx.identity.deliver_to_remote_signers(response_json);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}
