//! `CreateCashuWallet` intent -> a kind:17375 NIP-44 *self*-encrypted wallet
//! event, driven entirely through the signer-transparent NIP-44 + sign ports
//! (never raw Nostr key material — D13; see `chain.rs`'s module docs for why).

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::{WalletConfig, KIND_NIP60_WALLET};

use crate::journal::{WalletOperationId, WalletOperationState};

use super::chain::launch_self_encrypted_publish;
use super::state::{lock_state, CashuWalletState};

pub(super) struct CreateCashuWalletCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) mint: String,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for CreateCashuWalletCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateCashuWalletCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish()
    }
}

impl ProtocolCommand for CreateCashuWalletCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            mint,
            correlation_id,
        } = *self;

        // A fresh Cashu-domain keypair — NOT the Nostr identity key (NIP-61
        // P2PK receiving key). Pure secp256k1 generation, no signer
        // involvement; reuses `nmp_nip60::WalletConfig::generate`, the same
        // call the raw-key `Nip60WalletHandle::create_new` path makes.
        let config = WalletConfig::generate(vec![mint.clone()]);
        let Ok(cashu_pubkey_hex) = config.pubkey_hex() else {
            super::report_pre_dispatch_failure(
                ctx,
                &correlation_id,
                super::ui_codes::OPERATION_FAILED,
                "cashu pubkey derivation failed".to_string(),
            );
            return Ok(());
        };
        // #2917 — parsed once up front so a malformed `privkey_hex` (should
        // never happen: `WalletConfig::generate` just produced it) fails
        // closed here rather than leaving `RedeemNutzap` to discover a bad
        // key later.
        let Ok(cashu_sk) = <nostr::secp256k1::SecretKey as std::str::FromStr>::from_str(
            &config.privkey_hex,
        ) else {
            super::report_pre_dispatch_failure(
                ctx,
                &correlation_id,
                super::ui_codes::OPERATION_FAILED,
                "cashu privkey parse failed".to_string(),
            );
            return Ok(());
        };
        let plaintext = wallet_config_plaintext(&config);

        // V-07-style outbox resolution, resolved up front while `ctx` is
        // still available — the chain below only holds an owned
        // `CommandSender`, mirroring `nmp_nip17::dm_send`'s up-front
        // `required_dm_relays` resolution.
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
                    state.created = true;
                    state.mints = vec![mint];
                    state.cashu_pubkey_hex = Some(cashu_pubkey_hex);
                    state.cashu_privkey = Some(super::state::CashuP2pkSecret(cashu_sk));
                    // Best-effort: a journal-invariant violation here would mean
                    // this operation was already terminal, which can't happen on
                    // this single-shot path — but never panic on it (D6).
                    let _ = state.transition(&on_signed_op, WalletOperationState::PublishPending);
                    state.mints.clone()
                };
                // #3030 PR2 of 2 — a brand-new wallet's mint is trivially
                // "the accepted-mint set changed"; refresh its cached NUT-06/
                // NUT-02 info off this (already off-actor-thread) continuation.
                // The guard above drops the lock before this call, which
                // itself locks `on_signed_state` again.
                super::mint_info::spawn_mint_info_refresh(
                    Arc::clone(&on_signed_state),
                    mints_for_refresh,
                );
            },
        );

        Ok(())
    }
}

/// Mirrors `nmp_nip60::wallet_event::build_wallet_event`'s plaintext content
/// shape exactly (a NIP-44-encrypted JSON array of `[key, value]` pairs) so a
/// kind:17375 this command builds decodes unchanged with
/// `nmp_nip60::decode_wallet_event`. Duplicated rather than reused because
/// that builder bakes in a raw-`Keys` NIP-44 encrypt; this command routes
/// encryption through the signer-transparent port instead (fail-closed if the
/// signer cannot NIP-44 — the whole point of #2895 W2's design requirement).
fn wallet_config_plaintext(config: &WalletConfig) -> String {
    let mut pairs: Vec<Vec<String>> = vec![vec!["privkey".to_string(), config.privkey_hex.clone()]];
    for mint in &config.mints {
        pairs.push(vec!["mint".to_string(), mint.clone()]);
    }
    serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".to_string())
}
