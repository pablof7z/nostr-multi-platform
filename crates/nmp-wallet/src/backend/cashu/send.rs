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
//!
//! # Balance-aware mutual-mint selection (#2997)
//!
//! `run()` no longer stops at the FIRST mint the recipient and this wallet
//! both accept: it walks every mutually-trusted mint in the recipient's
//! listed order and picks the first one THIS WALLET ALSO HAS ENOUGH BALANCE
//! AT. Before this fix, an underfunded first mutual mint failed the entire
//! send with `INSUFFICIENT_BALANCE` even when a later mutual mint could have
//! covered it. `NO_TRUSTED_MINT` is reserved for the case where no mint is
//! mutual at all; `INSUFFICIENT_BALANCE` now means "at least one mutual mint
//! exists, but none of them has enough balance".

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO;
use nmp_nip60::nutzap::decode_nutzap_info_fields;

use crate::journal::{WalletConsumedInput, WalletOperationId, WalletOperationState};

use super::send_worker::{run_send_worker, SendWorkerArgs};
use super::state::{canonicalize_mint_url, lock_state, CashuWalletState};
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

        // #2997 — pick the first mint that is BOTH mutually trusted (the
        // recipient lists it AND this wallet also accepts it, matched by
        // canonical mint identity per #2972 — never raw string equality) AND
        // actually has enough balance to cover `amount_sats`. Before this
        // fix, the FIRST mutual mint was picked unconditionally and an
        // underfunded first mint failed the whole send with
        // `INSUFFICIENT_BALANCE` even when a LATER mutual mint could have
        // covered it. The exact `u`-tag URL always comes from the
        // recipient's own list, never rewritten — design doc "Relay
        // Acquisition"/"NIP-61 Event Rules".
        let our_mints = lock_state(&state).mints.clone();
        let mut any_mutual_mint = false;
        let mut chosen_mint: Option<String> = None;
        for candidate in &recipient_info.mints {
            let target = canonicalize_mint_url(candidate);
            if !our_mints.iter().any(|o| canonicalize_mint_url(o) == target) {
                continue;
            }
            any_mutual_mint = true;
            if lock_state(&state)
                .select_proofs(candidate, amount_sats)
                .is_some()
            {
                chosen_mint = Some(candidate.clone());
                break;
            }
        }
        let Some(mint) = chosen_mint else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                if any_mutual_mint {
                    ui_codes::INSUFFICIENT_BALANCE
                } else {
                    ui_codes::NO_TRUSTED_MINT
                },
                if any_mutual_mint {
                    "insufficient balance at every mutually-trusted mint".to_string()
                } else {
                    "no mint the recipient accepts is also accepted by this wallet".to_string()
                },
            );
        };

        // Re-select under the SAME lock discipline as before (`select_proofs`
        // is a pure read; nothing mutates `state.proofs` between the loop
        // above and here on this single-threaded actor) — this is what
        // actually reserves the exact proof set spent below.
        let Some((selected, selected_total)) = lock_state(&state).select_proofs(&mint, amount_sats)
        else {
            // Unreachable given the check above already proved this mint has
            // enough balance; fail closed rather than unwrap/panic if it
            // somehow no longer does.
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
                let _ = s.record_consumed_input(
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
                // `return EXPR;` evaluates `fail(..)` BEFORE unwinding this
                // block and dropping `s`, and `fail` re-locks the same
                // non-reentrant mutex — so the guard must be released
                // explicitly first or this branch self-deadlocks (#2953),
                // matching the same fix in `redeem.rs`. Reachable when a
                // concurrent `reset()` wipes this operation's journal entry
                // mid-flight, turning the transition into `MissingOperation`.
                drop(s);
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
            // #2960 — durable pre-publish record, in the SAME lock scope as the
            // reservation: the inputs are now reserved (removed from the
            // inventory) but the mint swap hasn't happened yet (`swapped:
            // None`). Persisting here means a crash before the swap leaves a
            // resumable record `restore_from_wal` reconciles against on restart
            // (see `wal_send.rs`).
            super::wal_send::persist_send_payload(
                &s,
                &operation_id,
                &mint,
                &recipient_pubkey,
                &recipient_cashu_pubkey,
                target_event_id.as_deref(),
                &recipient_info.relays,
                &selected,
                None,
            );
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
