//! `SetCashuMints` intent -> a kind:17375 NIP-44 *self*-encrypted wallet
//! event that replaces the accepted-mint list WITHOUT rotating the Cashu
//! P2PK receive key (#2997).
//!
//! # Why this is a distinct command from `CreateCashuWalletCommand`
//!
//! `cashu.create`'s `CreateCashuWalletCommand` always calls
//! `nmp_nip60::WalletConfig::generate`, which mints a FRESH Cashu P2PK
//! privkey every time it runs — the only way today to publish a kind:17375
//! at all. That is fine for a first-time wallet, but there is no way to
//! change the accepted-mint list on an EXISTING wallet without also rotating
//! its receive key, which would strand any incoming P2PK-locked proofs
//! (kind:9321 nutzaps, kind:7375 tokens) already locked to the old pubkey —
//! a real-sats loss, not a cosmetic gap.
//!
//! This command instead reads the EXISTING `cashu_privkey` out of
//! [`CashuWalletState`] (never generates one) and publishes a kind:17375
//! whose `privkey` field is byte-identical to the wallet's current one, with
//! only the `mint` entries replaced. `on_signed` updates `state.mints` alone
//! — `cashu_pubkey_hex`/`cashu_privkey` are left completely untouched.
//!
//! Driven entirely through the signer-transparent NIP-44 + sign ports (never
//! raw Nostr key material — D13; see `chain.rs`'s module docs for why).

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::KIND_NIP60_WALLET;

use crate::journal::{WalletOperationId, WalletOperationState};

use super::chain::launch_self_encrypted_publish;
use super::state::{lock_state, CashuWalletState};

pub(super) struct SetCashuMintsCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) mints: Vec<String>,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for SetCashuMintsCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetCashuMintsCommand")
            .field("operation_id", &self.operation_id.as_str())
            .field("mint_count", &self.mints.len())
            .finish()
    }
}

impl ProtocolCommand for SetCashuMintsCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            mints,
            correlation_id,
        } = *self;

        // Read the EXISTING privkey hex — never generate one. The backend's
        // `start_set_mints` gate already required `cashu_privkey.is_some()`
        // before dispatching this command, but re-check here (D6: never
        // trust a synchronous pre-check to still hold by the time this runs)
        // rather than unwrap on a money-adjacent value.
        let sk_hex = {
            let s = lock_state(&state);
            s.cashu_privkey
                .as_ref()
                .map(|k| k.0.display_secret().to_string())
        };
        let Some(sk_hex) = sk_hex else {
            super::report_pre_dispatch_failure(
                ctx,
                &correlation_id,
                super::ui_codes::NO_CASHU_WALLET,
                "no existing Cashu privkey to carry forward".to_string(),
            );
            return Ok(());
        };

        let plaintext = wallet_config_plaintext(&sk_hex, &mints);

        let relays = ctx.recipient_publish_relays(&account_pubkey, KIND_NIP60_WALLET);
        // D7 — the kernel owns the wall clock; re-stamp before the wallet
        // event is built (see `chain.rs`'s `launch_self_encrypted_publish`).
        let created_at = ctx.now_secs();

        let worker_tx = ctx.command_sender_clone();
        let on_signed_state = Arc::clone(&state);
        let on_signed_op = operation_id.clone();
        launch_self_encrypted_publish(
            worker_tx,
            account_pubkey,
            KIND_NIP60_WALLET,
            plaintext,
            Vec::new(),
            relays,
            created_at,
            correlation_id,
            move |_tx, _signed| {
                let mints_for_refresh = {
                    let mut state = lock_state(&on_signed_state);
                    // Only the accepted-mint list changes — `cashu_pubkey_hex`/
                    // `cashu_privkey` are deliberately left untouched (that is
                    // the whole point of this command: key-PRESERVING).
                    state.mints = mints;
                    // Best-effort: a journal-invariant violation here would mean
                    // this operation was already terminal, which can't happen on
                    // this single-shot path — but never panic on it (D6).
                    let _ = state.transition(&on_signed_op, WalletOperationState::PublishPending);
                    state.mints.clone()
                };
                // #3030 PR2 of 2 — the accepted-mint set just changed;
                // refresh cached NUT-06/NUT-02 info for the new list (drops
                // stale entries implicitly: `snapshot.rs`'s `mint_info_rows`
                // only ever surfaces rows for CURRENTLY relevant mints).
                super::mint_info::spawn_mint_info_refresh(
                    Arc::clone(&on_signed_state),
                    mints_for_refresh,
                );
            },
        );

        Ok(())
    }
}

/// Mirrors `create_wallet.rs`'s `wallet_config_plaintext` exactly — the same
/// NIP-44-encrypted JSON array of `[key, value]` pairs
/// `nmp_nip60::wallet_event::build_wallet_event` produces — except the
/// `privkey` is the caller-supplied EXISTING hex, never a freshly generated
/// one.
fn wallet_config_plaintext(existing_privkey_hex: &str, mints: &[String]) -> String {
    let mut pairs: Vec<Vec<String>> = vec![vec![
        "privkey".to_string(),
        existing_privkey_hex.to_string(),
    ]];
    for mint in mints {
        pairs.push(vec!["mint".to_string(), mint.clone()]);
    }
    serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".to_string())
}
