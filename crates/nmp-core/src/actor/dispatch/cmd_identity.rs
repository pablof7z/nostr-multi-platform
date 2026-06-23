//! Identity-mutation dispatch arms for `dispatch_command`.
//!
//! Covers: `AddSigner`, `CreateAccount`, `SwitchActive`, `RemoveAccount`,
//! `BunkerHandshakeProgress`, `BunkerConnectionStateChanged`,
//! `Nip55SignerStateChanged`, `SignEventForReturn`.
//!
//! Extracted from `dispatch.rs` to keep `mod.rs` under the LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.

use crate::actor::commands;
use crate::actor::pending_sign::{ParkedOp, ParkedSignerOps};
use crate::actor::{session_persistence, ActorCommand};
use crate::relay::OutboundMessage;

use super::helpers::{build_unsigned_for_return, signed_event_to_json, update_local_key_slots};
use super::ActorContext;

/// Dispatch `ActorCommand::SignEventForReturn`.
pub(super) fn sign_event_for_return(
    account_pubkey: String,
    unsigned_json: String,
    correlation_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // D13 sign-and-return: sign the host's draft with the named (or
    // active) account and hand the signed JSON straight back through
    // the `signed_events` projection — NEVER publish. Closes the gap
    // where a host needed raw private key bytes to sign a Blossom /
    // feedback auth event, which is impossible for NIP-46 bunker users.
    //
    // The host draft is `{ kind, content, tags, created_at? }` — it
    // carries no `pubkey` (the host does not know which signer will be
    // used) and its `created_at` is advisory. Parse the partial draft
    // and fill `pubkey` from the resolved account + re-stamp
    // `created_at` from the kernel clock (D7 — the host never owns
    // wall-clock time).
    let signer_pubkey = if account_pubkey.is_empty() {
        ctx.identity.active_pubkey()
    } else {
        Some(account_pubkey.clone())
    };
    let Some(signer_pubkey) = signer_pubkey else {
        ctx.kernel.record_signed_event_return(
            &correlation_id,
            Err("no active account — sign in first".to_string()),
        );
        maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
        return Some(Vec::new());
    };
    let unsigned = match build_unsigned_for_return(
        &unsigned_json,
        &signer_pubkey,
        ctx.kernel.now_secs(),
    ) {
        Ok(unsigned) => unsigned,
        Err(reason) => {
            ctx.kernel.record_signed_event_return(
                &correlation_id,
                Err(format!("invalid unsigned_json: {reason}")),
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            return Some(Vec::new());
        }
    };
    // Non-blocking sign (D8): a local key resolves on the spot; a
    // NIP-46 bunker returns `Pending` and is parked below.
    let sign_result = if account_pubkey.is_empty() {
        commands::sign_active_nonblocking(ctx.identity, &unsigned)
    } else {
        commands::sign_with_account_nonblocking(ctx.identity, &signer_pubkey, &unsigned)
    };
    match sign_result {
        Err(reason) => {
            ctx.kernel
                .record_signed_event_return(&correlation_id, Err(reason));
        }
        Ok(mut op) => match op.poll() {
            Some(Ok(signed)) => {
                ctx.kernel.record_signed_event_return(
                    &correlation_id,
                    Ok(signed_event_to_json(&signed)),
                );
            }
            Some(Err(e)) => {
                ctx.kernel
                    .record_signed_event_return(&correlation_id, Err(e.to_string()));
            }
            None => {
                // Remote signer parked → `signed_events` projection. Use
                // the SIGNING account's per-op deadline (ADR-0050 D4): a
                // named 90s NIP-55 key must not inherit the active
                // account's (e.g. 5s) budget. `""` = active (`None`).
                let named = (!account_pubkey.is_empty()).then_some(account_pubkey.as_str());
                let deadline = ctx.identity.sign_deadline_for(named);
                ctx.parked_ops.push(ParkedOp::signed_events_projection(
                    op,
                    correlation_id.clone(),
                    deadline,
                ));
            }
        },
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::AddSigner`.
pub(super) fn add_signer(
    source: crate::actor::SignerSource,
    make_active: bool,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    use crate::actor::SignerSource;
    let is_bunker_handshake = matches!(source, SignerSource::BunkerUri(_));
    let is_app_managed_local = matches!(source, SignerSource::AppManagedLocalNsec(_));
    let remote_persistence = match &source {
        SignerSource::RemoteHandle(handle) => {
            Some((handle.pubkey_hex(), handle.persistence_payload_json()))
        }
        _ => None,
    };
    let outbound = commands::add_signer(
        ctx.identity,
        ctx.kernel,
        source,
        make_active,
        ctx.relays_ready,
    );
    if !is_bunker_handshake {
        if let Some((remote_identity_id, Some(payload_json))) = &remote_persistence {
            session_persistence::enqueue_persist_remote_signer_payload(
                remote_identity_id,
                payload_json,
                ctx.capability_work_tx,
            );
        }
        if is_app_managed_local {
            session_persistence::enqueue_persist_app_managed_local_signers(
                ctx.identity,
                ctx.capability_work_tx,
            );
        }
        update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
        session_persistence::enqueue_persist_current_active_session(
            ctx.identity,
            ctx.capability_work_tx,
        );
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::CreateAccount`.
pub(super) fn create_account(
    profile: std::collections::HashMap<String, String>,
    relays: Vec<(String, String)>,
    initial_follows: Vec<String>,
    mls: bool,
    make_active: bool,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    let outbound = commands::create_account(
        ctx.identity,
        ctx.kernel,
        ctx.relays_ready,
        &profile,
        &relays,
        &initial_follows,
        mls,
        make_active,
    );
    update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
    // ADR-0040 §3 — enqueue the Keychain write off-actor (D8).
    session_persistence::enqueue_persist_current_active_session(
        ctx.identity,
        ctx.capability_work_tx,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::SwitchActive`.
pub(super) fn switch_active(
    identity_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    let outbound =
        commands::switch_active(ctx.identity, ctx.kernel, &identity_id, ctx.relays_ready);
    update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
    // ADR-0040 §3 — enqueue the Keychain write off-actor (D8).
    session_persistence::enqueue_persist_current_active_session(
        ctx.identity,
        ctx.capability_work_tx,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::RemoveAccount`.
pub(super) fn remove_account(
    identity_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    let outbound = commands::remove_account(ctx.identity, ctx.kernel, &identity_id);
    update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
    // ADR-0040 §3 — enqueue the Keychain forget + active-pointer
    // persist off-actor (D8). FIFO ordering ensures forget(acct-X)
    // executes before any subsequent persist for the new active
    // account — the single worker drains in enqueue order.
    session_persistence::enqueue_forget_account(&identity_id, ctx.capability_work_tx);
    session_persistence::enqueue_persist_app_managed_local_signers(
        ctx.identity,
        ctx.capability_work_tx,
    );
    session_persistence::enqueue_persist_current_active_session(
        ctx.identity,
        ctx.capability_work_tx,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::BunkerHandshakeProgress`.
pub(super) fn bunker_handshake_progress(
    stage: String,
    code: Option<String>,
    message: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    commands::bunker_handshake_progress(ctx.identity, ctx.kernel, stage, code, message);
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::BunkerConnectionStateChanged`.
pub(super) fn bunker_connection_state_changed(
    state: String,
    reason: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    commands::bunker_connection_state_changed(ctx.identity, ctx.kernel, state, reason);
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::Nip55SignerStateChanged`.
pub(super) fn nip55_signer_state_changed(
    state: String,
    reason: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    commands::nip55_signer_state_changed(ctx.identity, ctx.kernel, state, reason);
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::CapabilityResultReady`.
#[cfg(feature = "native")]
pub(super) fn capability_result_ready(
    account_id: String,
    result_json: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    if !ctx.identity.contains_account(&account_id) {
        tracing::trace!(
            "CapabilityResultReady: dropped result for removed account {account_id}"
        );
        return Some(Vec::new());
    }
    // Decode the outer CapabilityEnvelope and check the inner
    // KeyringResult status. An error result surfaces a D6 toast so
    // the user sees "keychain write failed" rather than a silent
    // secret-not-persisted bug. Success results are no-ops (the
    // write is already done on the Keychain).
    let decoded =
        serde_json::from_str::<crate::substrate::CapabilityEnvelope>(&result_json)
            .ok()
            .map(|env| crate::substrate::KeyringIdentityWiring::decode_result(&env));
    if let Some(result) = decoded {
        use crate::substrate::KeyringStatus;
        match result.status {
            KeyringStatus::Ok => {
                // Write succeeded — no observable actor-state change needed.
            }
            KeyringStatus::NotFound | KeyringStatus::Error => {
                // D6 — surface as a toast so the user can see the
                // Keychain write failed (session may not persist).
                ctx.kernel.set_last_error_token(
                    &crate::ui_token::UiToken::error(
                        crate::ui_token::codes::KEYRING_WRITE_FAILED,
                        format!(
                            "keyring write failed for account {account_id}: {:?}",
                            result.status
                        ),
                    )
                    .with_subject(account_id.to_string())
                    .with_detail(format!("{:?}", result.status)),
                );
                maybe_emit_after_dispatch(
                    ctx.kernel,
                    *ctx.running,
                    ctx.update_tx,
                    ctx.last_emit,
                );
            }
        }
    }
    Some(Vec::new())
}
