//! `SendNutzap` intent -> P2PK-lock proofs to a recipient's Cashu pubkey at a
//! mutually-trusted mint, publish kind:9321 (#2917, epic #2864 W8).
//!
//! # Recipient resolution — cache-read, warm-on-miss, fail-closed (#2936)
//!
//! This wallet has no read interest for an arbitrary recipient's kind:10019
//! by default (unlike the active account's own self-authored events —
//! `interests.rs`). `ctx.latest_author_kind` is a POINT-IN-TIME cache read
//! (see its doc comment), not a fetch: a recipient this account has never
//! observed a kind:10019 from misses here. On a miss, `run()` opens
//! `interests::recipient_nutzap_info_interest` for that recipient (via the
//! generic `ctx.ensure_interest`, mirroring `nmp-marmot`'s peer-KeyPackage
//! lookup) so the event flows into the kernel event store — then still fails
//! closed on THIS attempt. There is no in-command fetch-then-resume: the
//! caller (action retry, or a future observe-then-retry UX) is responsible
//! for trying again; once the recipient's kind:10019 has arrived through the
//! newly-opened interest, that retry finds it cached and proceeds.
//!
//! # Deferred: republishing kind:7375/kind:5 for spent/change proofs
//!
//! On success this updates the LOCAL proof inventory (removes the spent
//! proofs, adds the change proofs) and the ledger's ref-only facts, so this
//! wallet's own balance is correct immediately. It does NOT publish a
//! replacement kind:7375 token event for the change proofs or a NIP-09
//! deletion of the now-partially-spent token event(s) — mirrors
//! `deposit.rs`'s own documented "kind:7376 not wired" deferral (same class
//! of gap: a session-local wallet has correct state; another NIP-60 client
//! reading this wallet's relays sees a stale kind:7375 until a follow-up
//! wires the republish). Tracked as a fast-follow, not a silent gap.
//!
//! # Worker-thread half
//!
//! `send_worker.rs` (AGENTS.md file-size split) owns everything from the mint
//! HTTP round-trip onward, including the definite-vs-ambiguous-failure
//! restore/no-restore split for the proof reservation this file makes below.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO;
use nmp_nip60::nutzap::decode_nutzap_info_fields;

use crate::journal::{WalletConsumedInput, WalletOperationId, WalletOperationState};

use super::send_worker::{run_send_worker, SendWorkerArgs};
use super::state::{lock_state, CashuWalletState};
use super::ui_codes;

// Re-exported so callers (and this module's own tests, via `send::`) reach
// the worker-thread half without needing to know it lives in a sibling file
// split for size — `send.rs` is still the one public seam for this intent.
#[cfg(test)]
pub(super) use super::send_worker::{finish_send, FinishSendArgs};

pub(super) struct SendNutzapCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) recipient_pubkey: String,
    pub(super) amount_sats: u64,
    pub(super) target_event_id: Option<String>,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for SendNutzapCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendNutzapCommand")
            .field("operation_id", &self.operation_id.as_str())
            .field("amount_sats", &self.amount_sats)
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for SendNutzapCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            recipient_pubkey,
            amount_sats,
            target_event_id,
            correlation_id,
        } = *self;

        let Some(info_event) = ctx.latest_author_kind(&recipient_pubkey, KIND_NIP61_NUTZAP_INFO)
        else {
            // Warm the cache for next time: nothing else subscribes to a
            // third party's kind:10019 by default (#2936). This attempt
            // still fails closed; the caller's existing retry loop is what
            // picks up the recipient's info once this interest's REQ
            // delivers it.
            ctx.ensure_interest(
                crate::interests::recipient_nutzap_info_identity(&recipient_pubkey),
                crate::interests::recipient_nutzap_info_interest(&recipient_pubkey),
            );
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_RECIPIENT_NUTZAP_INFO,
                "recipient has no cached kind:10019 nutzap info".to_string(),
            );
        };
        let recipient_info = decode_nutzap_info_fields(&info_event.tags);
        if recipient_info.relays.is_empty() {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_RECIPIENT_RELAYS,
                "recipient's kind:10019 lists no relay".to_string(),
            );
        }
        let Some(recipient_cashu_pubkey) = recipient_info.cashu_pubkey.clone() else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_RECIPIENT_P2PK,
                "recipient's kind:10019 has no Cashu P2PK pubkey".to_string(),
            );
        };

        // Use ONLY a mint the recipient lists that this wallet also accepts
        // (the exact `u` tag URL comes from the recipient's own list, never
        // rewritten) — design doc "Relay Acquisition"/"NIP-61 Event Rules".
        let our_mints = lock_state(&state).mints.clone();
        let Some(mint) = recipient_info
            .mints
            .iter()
            .find(|m| our_mints.contains(m))
            .cloned()
        else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_TRUSTED_MINT,
                "no mint the recipient accepts is also accepted by this wallet".to_string(),
            );
        };

        let Some((selected, selected_total)) =
            lock_state(&state).select_proofs(&mint, amount_sats)
        else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::INSUFFICIENT_BALANCE,
                "insufficient balance at the trusted mint".to_string(),
            );
        };

        // Pre-effect record — money-safety: these inputs are about to be
        // consumed by a mint swap BEFORE the HTTP call goes out. `remove_proofs`
        // here (not just the journal write) is what actually reserves the
        // selection: `run()` executes synchronously on the single-threaded
        // actor, but the worker thread spawned below does not, so without
        // removing these proofs from the inventory NOW, a second `SendNutzap`
        // dispatched before this operation's worker finishes could select the
        // SAME proofs (a double-tap race) — `select_proofs` is a pure read
        // with no reservation of its own.
        {
            let mut s = lock_state(&state);
            for stored in &selected {
                let _ = s.journal.record_consumed_input(
                    &operation_id,
                    WalletConsumedInput {
                        event_id: stored.token_event.clone().unwrap_or_default(),
                        mint: mint.clone(),
                        unit: "sat".to_string(),
                        amount: stored.proof.amount,
                    },
                );
            }
            if let Err(e) = s.transition(&operation_id, WalletOperationState::MintPending) {
                return fail(
                    ctx,
                    &state,
                    &operation_id,
                    correlation_id,
                    ui_codes::JOURNAL_ERROR,
                    format!("{e:?}"),
                );
            }
            s.remove_proofs(&selected);
        }

        let relays = recipient_info.relays.clone();
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            run_send_worker(SendWorkerArgs {
                worker_tx,
                state,
                operation_id,
                account_pubkey,
                recipient_pubkey,
                mint,
                selected,
                selected_total,
                amount_sats,
                recipient_cashu_pubkey,
                target_event_id,
                relays,
                created_at,
                correlation_id,
            });
        });
        Ok(())
    }
}

pub(super) fn fail(
    ctx: &ProtocolCommandContext<'_>,
    state: &Arc<Mutex<CashuWalletState>>,
    operation_id: &WalletOperationId,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) -> Result<(), ProtocolCommandError> {
    let _ = lock_state(state).transition(operation_id, WalletOperationState::Failed);
    super::report_pre_dispatch_failure(ctx, &correlation_id, code, reason);
    Ok(())
}
