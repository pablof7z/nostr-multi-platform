//! [`CashuWalletBackend`] construction and per-account lifecycle: WAL-store
//! injection, the cross-account `reset`, and the durable-WAL `restore_from_wal`
//! (PR-1 of #2910/#2960/#2931). Split out of `mod.rs` for LOC discipline; the
//! `WalletBackend` trait impl and intent dispatch stay there.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;

use crate::journal::{restore_into_journal, WalletWalStore};

use super::cross_mint_resume::{restore_cross_mint_transfers, ResumeCrossMintTransferCommand};
use super::deposit::ResumeDepositCommand;
use super::state::{lock_state, CashuWalletState};
use super::wal_payload::restore_deposits;
use super::wal_redeem::{restore_redeems, ResumeRedeemCommand};
use super::wal_send::{restore_sends, ResumeSendCommand};
use super::CashuWalletBackend;

impl CashuWalletBackend {
    #[must_use]
    pub fn new() -> Self {
        Self::with_wal_store(None)
    }

    /// Construct the backend with a durable pre-publish WAL store (PR-1). The
    /// composition root (`crate::register::register`) builds an
    /// [`crate::journal::FsWalletWalStore`] when a `storage_path` is configured
    /// and passes it here; `None` keeps the journal in-memory-only.
    #[must_use]
    pub fn with_wal_store(wal_store: Option<Arc<dyn WalletWalStore>>) -> Self {
        let mut state = CashuWalletState::new();
        state.wal_store = wal_store.clone();
        Self {
            state: Arc::new(Mutex::new(state)),
            wal_store,
        }
    }

    /// Discard all in-memory wallet state (created flag, mints, Cashu P2PK
    /// pubkey, ledger, journal, pending deposits) and start fresh.
    ///
    /// Required because this backend, unlike `NwcWalletBackend`'s connection
    /// state, holds NIP-44-encrypted-to-a-specific-identity material
    /// (`kind:17375`'s Cashu private key + accepted mints, `kind:7375`
    /// proofs): it is constructed once per app instance
    /// (`register.rs`), not once per signed-in account, so without this reset
    /// a Nostr account switch within one running app would leave the
    /// PREVIOUS account's balance/mint list/pending deposits visible to
    /// (and, via `complete_deposit`, completable as) the NEWLY active
    /// account — a cross-account data/fund leak. Callers wire this to fire
    /// on every active-account change (`nmp_core::substrate::IdentityChangeRegistrar`),
    /// mirroring how `nmp-nip51`'s `MuteListProjection` resets on the same
    /// signal. Losing in-memory wallet state on account switch is expected
    /// and safe: nothing here is the source of truth — the durable
    /// `kind:17375`/`kind:7375`/`kind:7376` events are, and `created = false`
    /// after this reset is exactly what lets the NEWLY active account's own
    /// wallet get reloaded — `runtime.rs`'s identity-change observer re-syncs
    /// `wallet_self_authored_shape`'s reconciler on every account switch,
    /// whose replay `on_self_authored_wallet_event` (#2965, `events.rs`)
    /// folds back into this fresh state the same way cold start does.
    pub fn reset(&self) {
        let mut state = lock_state(&self.state);
        *state = CashuWalletState::new();
        // Re-thread the app-lifetime WAL store into the fresh state; leave
        // `wal_account` `None` until `restore_from_wal` sets it for the newly
        // active account. Write-through therefore no-ops in the window between
        // reset and restore (no account is active there anyway).
        state.wal_store = self.wal_store.clone();
    }

    /// Rehydrate the in-memory saga journal from the durable WAL for the
    /// now-active `account` (PR-1 of #2910/#2960/#2931). Called after
    /// [`Self::reset`] on every identity change, plus once eagerly at
    /// registration to cover cold start (the account may already be active).
    ///
    /// Sets [`CashuWalletState::wal_account`] so subsequent write-through keys
    /// under this account, then loads persisted non-terminal operations back
    /// into the live journal and deletes terminal rows from disk — see
    /// [`crate::journal::restore_into_journal`] for the #2931 terminal-row
    /// deletion rule (a terminal `Failed` redeem must NOT survive back into the
    /// live journal, or a re-observed kind:9321 would be blocked forever by the
    /// `DuplicateOperation` guard). A no-op when no WAL is configured.
    ///
    /// PR-2 of #2910 additionally rebuilds `pending_deposits` from the durable
    /// Cashu payloads ([`restore_deposits`]) — the deposit-side recovery that
    /// unbreaks `start_complete_deposit`'s `UNKNOWN_QUOTE` lookup after a hard
    /// crash — and RETURNS a `ResumeDepositCommand` per deposit past the mint
    /// (minted or signed) for the caller to enqueue onto the actor. The re-drive
    /// is a returned command, not run inline, because the encrypt/sign/publish
    /// chain must run through a `ProtocolCommand`'s `ctx` (relays + command
    /// sender, off the actor thread per D8) — the same seam the in-process
    /// `DepositResume` retry already uses. The caller
    /// (`register::register`'s identity observer + eager cold-start restore)
    /// forwards the returned commands via `app.actor_sender()`.
    ///
    /// PR-3 of #2960/#2931 extends this to send + redeem: a [`ResumeSendCommand`]
    /// per in-flight send and a [`ResumeRedeemCommand`] per in-flight redeem,
    /// each of which (off the actor thread, same D8 reason) either re-drives its
    /// `finish_*` seam from persisted post-swap proofs or NUT-07 check-state
    /// reconciles a reserved-but-never-swapped operation (see
    /// `wal_send.rs`/`wal_redeem.rs`). The mint-HTTP work all happens inside
    /// those commands' worker threads, never inline here (this method holds the
    /// state lock).
    #[must_use]
    pub fn restore_from_wal(&self, account: &str) -> Vec<ActorCommand> {
        let Some(store) = self.wal_store.clone() else {
            return Vec::new();
        };
        let mut state = lock_state(&self.state);
        state.wal_account = Some(account.to_string());
        let _ = restore_into_journal(store.as_ref(), account, &mut state.journal);
        let deposit_resumes = restore_deposits(&mut state, store.as_ref(), account);
        let send_resumes = restore_sends(store.as_ref(), account);
        let redeem_resumes = restore_redeems(store.as_ref(), account);
        let cross_mint_resumes = restore_cross_mint_transfers(&mut state, store.as_ref(), account);

        let mut commands: Vec<ActorCommand> = Vec::new();
        for resume in deposit_resumes {
            commands.push(ActorCommand::Protocol(Box::new(ResumeDepositCommand {
                state: Arc::clone(&self.state),
                operation_id: resume.operation_id,
                quote_id: resume.quote_id,
                mint: resume.mint,
                account_pubkey: account.to_string(),
            })));
        }
        for resume in send_resumes {
            commands.push(ActorCommand::Protocol(Box::new(ResumeSendCommand {
                state: Arc::clone(&self.state),
                account_pubkey: account.to_string(),
                resume,
            })));
        }
        for resume in redeem_resumes {
            commands.push(ActorCommand::Protocol(Box::new(ResumeRedeemCommand {
                state: Arc::clone(&self.state),
                account_pubkey: account.to_string(),
                resume,
            })));
        }
        for resume in cross_mint_resumes {
            commands.push(ActorCommand::Protocol(Box::new(
                ResumeCrossMintTransferCommand {
                    state: Arc::clone(&self.state),
                    account_pubkey: account.to_string(),
                    target_quote_id: resume.target_quote_id,
                },
            )));
        }
        commands
    }
}
