//! `CashuWalletBackend::on_wallet_event`'s per-kind handlers — split out of
//! `mod.rs` (AGENTS.md file-size discipline), mirroring how
//! `nutzap_dispatch.rs` already splits `start_intent`'s publish_info/send/
//! redeem handlers into their own `impl CashuWalletBackend` extension block.
//!
//! [`CashuWalletBackend::on_nutzap_event`] (kind:9321) is unchanged from
//! before #2965, moved here verbatim. [`CashuWalletBackend::on_self_authored_wallet_event`]
//! (kind:17375/7375) is new: wallet recovery (#2965) — see `ingest.rs`'s
//! module docs for the decode/fold logic and confluence guarantees this
//! delegates to.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::KernelEvent;

use crate::journal::{WalletOperationKind, WalletOperationState};

use super::ingest::build_passive_ingest_command;
use super::nutzap_dispatch::redeem_operation_id;
use super::redeem::RedeemNutzapCommand;
use super::state::lock_state;
use super::{CashuWalletBackend, WalletBackendContext};

impl CashuWalletBackend {
    /// #2917 (W9) — a received nutzap. Keyed by the nutzap's own event id
    /// (not a correlation id — this path has none): re-observing the same
    /// kind:9321 (a relay resend, or an explicit `nmp.wallet.nutzap.redeem`
    /// retry for one the observer already started) hits `DuplicateOperation`
    /// here and is silently skipped rather than double-dispatched — a
    /// natural at-most-once guard shared with `nutzap_dispatch.rs`'s
    /// `start_redeem_nutzap`.
    pub(super) fn on_nutzap_event(
        &self,
        ctx: WalletBackendContext<'_>,
        event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return Vec::new();
        };
        let operation_id = redeem_operation_id(&event.id);
        {
            let mut state = lock_state(&self.state);
            if state
                .begin_operation_at(
                    operation_id.clone(),
                    WalletOperationKind::RedeemNutzap,
                    ctx.now_secs,
                )
                .is_err()
            {
                return Vec::new();
            }
        }
        vec![ActorCommand::Protocol(Box::new(RedeemNutzapCommand {
            state: Arc::clone(&self.state),
            operation_id,
            account_pubkey,
            event_id: event.id.clone(),
            correlation_id: None,
        }))]
    }

    /// #2965 — the account's own self-authored kind:17375 (wallet config) or
    /// kind:7375 (token proofs), observed via `interests.rs`'s
    /// `wallet_self_authored_shape` (cold-start replay + live tail —
    /// `runtime.rs`). Decrypts and folds into state so a returning user's
    /// already-published wallet is loaded rather than orphaned behind a
    /// fresh `CreateCashuWallet`.
    pub(super) fn on_self_authored_wallet_event(
        &self,
        ctx: WalletBackendContext<'_>,
        event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey else {
            return Vec::new();
        };
        // Defensive re-check even though `wallet_self_authored_shape` already
        // filters `authors = self` at the relay — never trust relay-side
        // filtering alone (the same principle `RedeemNutzapCommand::run`'s
        // own `p`-tag re-check documents).
        if event.author != account_pubkey {
            return Vec::new();
        }
        // Settle rule (PR-2 of #2910): our own kind:7375 coming back from a
        // relay is the publish-ACK the deposit flow never otherwise had. Fire
        // BEFORE the (async, decrypt-gated) ingest below — the match keys on the
        // event id alone, no decryption needed — so a landed deposit is retired
        // from the durable WAL the moment its token event is re-observed.
        if event.kind == nmp_nip60::kinds::KIND_NIP60_TOKEN {
            self.settle_deposit_on_ingested_token(&event.id);
        }
        vec![build_passive_ingest_command(
            Arc::clone(&self.state),
            account_pubkey.to_string(),
            event.kind,
            event.id.clone(),
            event.content.clone(),
            event.relay_provenance.first().cloned().unwrap_or_default(),
        )]
    }

    /// Retire a deposit whose kind:7375 token event has been re-observed from a
    /// relay (PR-2 of #2910 settle rule). When `event_id` matches a pending
    /// deposit's cached `signed_token.id`, the deposit is durably landed:
    /// transition its operation `PublishPending`/`Unknown` -> `Settled` (which,
    /// per PR-1's terminal write-through, deletes the saga row AND the WAL
    /// payload) and drop the `pending_deposits` entry. This closes the
    /// previously-accepted "unbounded `pending_deposits` map" tradeoff — the
    /// map no longer grows by one retained entry per completed deposit for the
    /// life of the process. A no-op when no deposit matches (any other
    /// self-authored kind:7375 — e.g. a send/redeem replacement token).
    fn settle_deposit_on_ingested_token(&self, event_id: &str) {
        let mut state = lock_state(&self.state);
        let matched = state
            .pending_deposits
            .iter()
            .find(|(_, pending)| {
                pending
                    .signed_token
                    .as_ref()
                    .is_some_and(|signed| signed.id == event_id)
            })
            .map(|(quote_id, pending)| (quote_id.clone(), pending.operation_id.clone()));
        let Some((quote_id, operation_id)) = matched else {
            return;
        };
        let _ = state.transition(&operation_id, WalletOperationState::Settled);
        state.pending_deposits.remove(&quote_id);
    }
}
