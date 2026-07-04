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

use crate::journal::WalletOperationKind;

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
        vec![build_passive_ingest_command(
            Arc::clone(&self.state),
            account_pubkey.to_string(),
            event.kind,
            event.id.clone(),
            event.content.clone(),
            event.relay_provenance.first().cloned().unwrap_or_default(),
        )]
    }
}
