//! `PublishNutzapInfo` intent -> a kind:10019 PUBLIC nutzap-receive-policy
//! event (#2917, epic #2864 W13) — the prerequisite for anyone to send this
//! wallet a nutzap.
//!
//! kind:10019 is neither self- nor peer-encrypted (NIP-61 requires senders to
//! read it directly), so this uses `chain::launch_plain_publish` — the
//! sign->publish chain without `create_wallet.rs`/`deposit.rs`'s NIP-44 step
//! — through the same signer-transparent sign port (D13).
//!
//! # Relay set (design doc "Relay Acquisition")
//!
//! 1. The active account's own already-cached kind:10019 `relay` tags, if any
//!    (`ctx.latest_author_kind` — a point-in-time cache read, see that
//!    accessor's doc comment).
//! 2. Else the NIP-65 fallback (`ctx.recipient_publish_relays`).
//!
//! Never an app-provided relay list — this mirrors `create_wallet.rs`'s own
//! `ctx.recipient_publish_relays` use for kind:17375, just with the
//! self-kind:10019 cache consulted first.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO;
use nmp_nip60::nutzap::{decode_nutzap_info_fields, nutzap_info_tags, NutZapInfo};

use crate::journal::{WalletOperationId, WalletOperationState};

use super::chain::launch_plain_publish;
use super::state::{lock_state, CashuWalletState};
use super::ui_codes;

pub(super) struct PublishNutzapInfoCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for PublishNutzapInfoCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublishNutzapInfoCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish()
    }
}

impl ProtocolCommand for PublishNutzapInfoCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            correlation_id,
        } = *self;

        let (mints, cashu_pubkey_hex) = {
            let s = lock_state(&state);
            (s.mints.clone(), s.cashu_pubkey_hex.clone())
        };
        let Some(cashu_pubkey_hex) = cashu_pubkey_hex else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_CASHU_WALLET,
                "no Cashu wallet created yet".to_string(),
            );
        };

        let self_cached_relays = ctx
            .latest_author_kind(&account_pubkey, KIND_NIP61_NUTZAP_INFO)
            .map(|event| decode_nutzap_info_fields(&event.tags).relays)
            .filter(|relays| !relays.is_empty());
        let relays = match self_cached_relays {
            Some(relays) => relays,
            None => ctx.recipient_publish_relays(&account_pubkey, KIND_NIP61_NUTZAP_INFO),
        };
        if relays.is_empty() {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_NUTZAP_RELAYS,
                "no relay set resolved to publish nutzap info to".to_string(),
            );
        }

        let info = NutZapInfo {
            relays: relays.clone(),
            mints,
            cashu_pubkey: Some(cashu_pubkey_hex),
        };
        let tags = nutzap_info_tags(&info);
        // D7 — the kernel owns the wall clock.
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        let on_signed_state = Arc::clone(&state);
        let on_signed_op = operation_id;
        launch_plain_publish(
            worker_tx,
            account_pubkey,
            KIND_NIP61_NUTZAP_INFO,
            tags,
            String::new(),
            relays,
            created_at,
            correlation_id,
            move |_tx, _signed| {
                let mut guard = lock_state(&on_signed_state);
                let _ = guard.transition(&on_signed_op, WalletOperationState::PublishPending);
            },
        );
        Ok(())
    }
}

fn fail(
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
