//! `RedeemNutzap` intent -> verify a kind:9321 nutzap, swap its proofs into
//! fresh wallet-owned proofs BEFORE marking it redeemed, publish kind:7375 +
//! kind:7376 (#2917, epic #2864 W9).
//!
//! Reached two ways, both converging on the SAME command: an explicit
//! `nmp.wallet.nutzap.redeem` action (`event_id` supplied by the caller) and
//! the observer path (`CashuWalletBackend::on_wallet_event` sees a matching
//! kind:9321 and dispatches this same command with that event's id) — see
//! `mod.rs`'s `on_wallet_event`. Either way this command resolves the FULL
//! event through [`ProtocolCommandContext::event_by_id`] rather than trusting
//! a caller-supplied event body, so the verification below always runs
//! against a value this backend's own cache actually holds.
//!
//! # Verification (independent of the relay-side filter)
//!
//! `interests.rs`'s `nutzap_receipts_shape` already filters on `#p = self` at
//! the relay, but this command re-checks every invariant in Rust regardless
//! of what a relay claims to have filtered — NIP-61 "Receiving":
//! - `p`-tagged to this account;
//! - not already redeemed (`WalletDerivedState::is_nutzap_redeemed`);
//! - `u` mint is one this wallet accepts;
//! - every proof's P2PK secret locks to THIS wallet's Cashu pubkey;
//! - this session holds the Cashu private key to produce the P2PK witness;
//! - DLEQ (NUT-12) valid — checked LAST (#2933): it is now a hard fail-closed
//!   gate (a missing DLEQ is rejected, not skipped — see
//!   `nmp_nip60::nutzap::verify_nutzap_dleq_against_keyset`'s doc comment)
//!   and the only one of these that round-trips to the mint over HTTP, so
//!   every cheaper, local check runs first.
//!
//! Any failure is `INVALID_NUTZAP`/`NO_TRUSTED_MINT`/`NO_CASHU_WALLET` —
//! rejected/ignored, never counted as value.
//!
//! # Swap-before-redeem ordering
//!
//! See `redeem_worker.rs`'s module docs for the pre-record/swap/publish
//! ordering once this command hands off to the worker thread.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::cashu::canonicalize_mint_url;
use nmp_nip60::kinds::{KIND_NIP60_TOKEN, KIND_NIP61_NUTZAP};
use nmp_nip60::nutzap::{decode_nutzap_fields, p2pk_secret_pubkey, ReceivedNutZap};

use crate::journal::{WalletConsumedInput, WalletEventId, WalletOperationId, WalletOperationState};

use super::redeem_worker::{run_redeem_worker, RedeemWorkerArgs};
use super::state::{lock_state, CashuWalletState};
use super::ui_codes;

// Re-exported so callers (and this module's own tests, via `redeem::`) reach
// the worker-thread half without needing to know it lives in a sibling file
// split for size — `redeem.rs` is still the one public seam for this intent.
#[cfg(test)]
pub(super) use super::redeem_worker::{finish_redeem, FinishRedeemArgs};

pub(super) struct RedeemNutzapCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) event_id: String,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for RedeemNutzapCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedeemNutzapCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for RedeemNutzapCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            event_id,
            correlation_id,
        } = *self;

        let Some(event) = ctx.event_by_id(&event_id) else {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                None,
                correlation_id,
                ui_codes::INVALID_NUTZAP,
                "nutzap event not found in this backend's cache".to_string(),
            );
        };
        // #2966 — record the sender as soon as the event resolves, before
        // any of the checks below can fail this operation: a nutzap feed's
        // "from <pubkey>" needs this on a rejected/unverifiable receive row
        // exactly as much as on a settled one (see `snapshot.rs`'s
        // `receive_rows`/`history_row`), and `event.author` is already this
        // wallet's own verified copy of who authored the kind:9321.
        let _ = lock_state(&state)
            .journal
            .record_sender(&operation_id, event.author.clone());
        if event.kind != KIND_NIP61_NUTZAP {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                None,
                correlation_id,
                ui_codes::INVALID_NUTZAP,
                "event is not a kind:9321 nutzap".to_string(),
            );
        }
        let p_tagged_self = event.tags.iter().any(|row| {
            row.first().map(String::as_str) == Some("p")
                && row.get(1).map(String::as_str) == Some(account_pubkey.as_str())
        });
        if !p_tagged_self {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                None,
                correlation_id,
                ui_codes::INVALID_NUTZAP,
                "nutzap is not p-tagged to this account".to_string(),
            );
        }

        let nutzap: ReceivedNutZap = match decode_nutzap_fields(
            &event.id,
            &event.author,
            &event.tags,
            &event.content,
        ) {
            Ok(n) => n,
            Err(e) => {
                return fail(
                    ctx,
                    &state,
                    &operation_id,
                    &event_id,
                    None,
                    correlation_id,
                    ui_codes::INVALID_NUTZAP,
                    format!("nutzap decode failed: {e}"),
                );
            }
        };

        let nutzap_wallet_event = WalletEventId::new(event_id.clone());
        if lock_state(&state)
            .ledger
            .state()
            .is_nutzap_redeemed(&nutzap_wallet_event)
        {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                Some(&nutzap),
                correlation_id,
                ui_codes::ALREADY_REDEEMED,
                "nutzap already redeemed".to_string(),
            );
        }

        let (our_mints, our_cashu_pubkey, cashu_sk) = {
            let s = lock_state(&state);
            (
                s.mints.clone(),
                s.cashu_pubkey_hex.clone(),
                s.cashu_privkey.as_ref().map(|k| k.0),
            )
        };
        // #2972 — canonical comparison: the sender's `u`-tag mint string need
        // not be byte-identical to this wallet's own accepted-mint string to
        // name the same real mint (trailing slash, scheme/host case).
        if !our_mints
            .iter()
            .any(|m| canonicalize_mint_url(m) == canonicalize_mint_url(&nutzap.mint_url))
        {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                Some(&nutzap),
                correlation_id,
                ui_codes::NO_TRUSTED_MINT,
                "nutzap's mint is not accepted by this wallet".to_string(),
            );
        }
        let Some(our_cashu_pubkey) = our_cashu_pubkey else {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                Some(&nutzap),
                correlation_id,
                ui_codes::NO_CASHU_WALLET,
                "no Cashu wallet created yet".to_string(),
            );
        };
        for proof in &nutzap.proofs {
            match p2pk_secret_pubkey(&proof.secret) {
                Some(locked_to) if locked_to == our_cashu_pubkey => {}
                _ => {
                    return fail(
                        ctx,
                        &state,
                        &operation_id,
                        &event_id,
                        Some(&nutzap),
                        correlation_id,
                        ui_codes::INVALID_NUTZAP,
                        "a proof is not P2PK-locked to this wallet's Cashu pubkey".to_string(),
                    );
                }
            }
        }
        // Cold-start recovery of the Cashu privkey from kind:17375 is a
        // documented, separate deferral (see `state.rs`'s `CashuP2pkSecret`
        // doc comment) — without it in live state, redemption fails closed
        // rather than silently skipping the P2PK witness. Checked BEFORE
        // `verify_nutzap_dleq` (a cheap, local, in-memory check ahead of an
        // HTTP round-trip to the mint) — no point spending that round-trip
        // when this session cannot complete the redemption regardless.
        let Some(cashu_sk) = cashu_sk else {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                Some(&nutzap),
                correlation_id,
                ui_codes::NO_CASHU_WALLET,
                "no Cashu private key available in this session".to_string(),
            );
        };
        if let Err(e) = nmp_nip60::nutzap::verify_nutzap_dleq(&nutzap) {
            return fail(
                ctx,
                &state,
                &operation_id,
                &event_id,
                Some(&nutzap),
                correlation_id,
                ui_codes::INVALID_NUTZAP,
                format!("DLEQ verification failed: {e}"),
            );
        }

        {
            let mut s = lock_state(&state);
            let _ = s.record_consumed_input(
                &operation_id,
                WalletConsumedInput {
                    event_id: event_id.clone(),
                    mint: nutzap.mint_url.clone(),
                    unit: "sat".to_string(),
                    amount: nutzap.amount_sats,
                },
            );
            if let Err(e) = s.transition(&operation_id, WalletOperationState::MintPending) {
                // `return EXPR;` evaluates `fail(..)` BEFORE unwinding this
                // block and dropping `s`, and `fail` re-locks the same
                // non-reentrant mutex — so the guard must be released
                // explicitly first or this branch self-deadlocks (#2953). This
                // branch is reachable when a concurrent `reset()` wipes the
                // journal entry out from under this in-flight redeem, making
                // the transition return `MissingOperation`.
                drop(s);
                return fail(
                    ctx,
                    &state,
                    &operation_id,
                    &event_id,
                    Some(&nutzap),
                    correlation_id,
                    ui_codes::JOURNAL_ERROR,
                    format!("{e:?}"),
                );
            }
        }

        let relays = ctx.recipient_publish_relays(&account_pubkey, KIND_NIP60_TOKEN);
        // #2931 — durable pre-swap record: now that the nutzap/mint/relays are
        // all known (and the consumed input is journalled above), persist a
        // resumable redeem record (`fresh_proofs: None`) BEFORE the swap HTTP
        // call goes out — mirroring the deposit "record as soon as you have
        // enough to resume" pattern. A crash mid-swap that leaves this operation
        // `Unknown` is what `restore_from_wal` reconciles against on restart
        // (see `wal_redeem.rs`).
        {
            let s = lock_state(&state);
            super::wal_redeem::persist_redeem_payload(
                &s,
                &operation_id,
                &nutzap,
                &nutzap_wallet_event,
                &relays,
                None,
            );
        }
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            run_redeem_worker(RedeemWorkerArgs {
                worker_tx,
                state,
                operation_id,
                account_pubkey,
                nutzap,
                nutzap_wallet_event,
                cashu_sk,
                relays,
                created_at,
                correlation_id,
            });
        });
        Ok(())
    }
}

/// `event_id`/`nutzap` are best-effort receive-candidate info (#2949):
/// `nutzap` is `Some` once `decode_nutzap_fields` has succeeded (every fail
/// site from `already_redeemed` onward), `None` for the earlier fail sites
/// where the event itself is still unproven (not found, wrong kind, not
/// p-tagged, undecodable). Recording it here — never overwriting an already-
/// recorded consumed input, since the happy path records the real one just
/// before the `MintPending` transition this fn is never called after except
/// on that very transition's own failure — is what lets a rejected/
/// unverifiable nutzap still surface in `WalletProjection::receive_rows`
/// (see `snapshot.rs`) instead of vanishing once this operation goes
/// terminal, matching the design doc's "Observer counting: unverifiable
/// nutzaps may be shown as rejected... not counted as value".
#[allow(clippy::too_many_arguments)]
pub(super) fn fail(
    ctx: &ProtocolCommandContext<'_>,
    state: &Arc<Mutex<CashuWalletState>>,
    operation_id: &WalletOperationId,
    event_id: &str,
    nutzap: Option<&ReceivedNutZap>,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) -> Result<(), ProtocolCommandError> {
    let mut s = lock_state(state);
    let already_recorded = s
        .journal
        .get(operation_id)
        .is_some_and(|op| !op.consumed_inputs.is_empty());
    if !already_recorded {
        let _ = s.record_consumed_input(
            operation_id,
            WalletConsumedInput {
                event_id: event_id.to_string(),
                mint: nutzap.map(|n| n.mint_url.clone()).unwrap_or_default(),
                unit: "sat".to_string(),
                amount: nutzap.map(|n| n.amount_sats).unwrap_or(0),
            },
        );
    }
    let _ = s.transition(operation_id, WalletOperationState::Failed);
    drop(s);
    super::report_pre_dispatch_failure(ctx, &correlation_id, code, reason);
    Ok(())
}
