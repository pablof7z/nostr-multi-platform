//! Cashu backend-owned resume payloads for the durable pre-publish WAL
//! (PR-2 of #2910 — the deposit-side payload writes + restore/re-drive).
//!
//! PR-1 (the journal-durability spine) made the backend-agnostic saga row
//! durable, but a saga row deliberately carries no Cashu noun — it only knows
//! ids/mints/units/amounts (`journal::saga`). The value-moving secret this
//! deposit flow crashes around — the freshly-minted [`Proof`]s, and the signed
//! kind:7375 [`SignedEvent`] built from them — must therefore ride a SEPARATE
//! opaque byte blob, keyed by the same operation id, through
//! [`WalletWalStore::upsert_payload`]/[`WalletWalStore::load_payload`]. This
//! module owns the Cashu-specific serialization of that blob, keeping the
//! durable spine in the backend-agnostic `journal` module (which never learns
//! a `Proof`) while the Cashu backend owns the payload shape.
//!
//! # Why this closes #2910
//!
//! A Cashu mint marks a quote `ISSUED` the instant it hands out proofs and
//! permanently refuses to re-mint it. Before PR-2, `PendingDeposit.minted_proofs`
//! lived only in memory: a hard crash between `mint_tokens` returning `Ok` and
//! the kind:7375 publishing lost those already-real proofs for good
//! (`start_complete_deposit` returned `UNKNOWN_QUOTE`, the mint refused to
//! re-issue). Persisting the minted proofs here — and rebuilding
//! `pending_deposits` from them on restore (see [`restore_deposits`]) — means a
//! restarted process resumes the encrypt/sign/publish chain from the same
//! proofs instead of stranding real sats.

use serde::{Deserialize, Serialize};

use nmp_nip60::cashu::types::Proof;
use nmp_signer_iface::SignedEvent;

use crate::journal::{WalletOperationId, WalletOperationKind, WalletWalStore};

use super::state::{CashuWalletState, PendingDeposit};

/// The Cashu backend's opaque WAL resume payload, keyed under the operation id
/// (the saga row's own key) via [`WalletWalStore::upsert_payload`].
///
/// One variant per operation family that has secret-bearing resume state. Only
/// [`Self::Deposit`] exists today — the Send/Redeem variants that persist
/// consumed proofs + signed replacement tokens are PR-3's job (the send+redeem
/// WAL wave) and land WITH their write points, not as a stub here.
#[derive(Serialize, Deserialize)]
pub(super) enum CashuWalPayload {
    /// A two-phase Cashu deposit. `minted_proofs`/`signed_token` are `None`
    /// until the flow reaches that step, mirroring [`PendingDeposit`]'s own
    /// field lifecycle exactly (quote created -> minted -> signed).
    Deposit {
        quote_id: String,
        mint: String,
        amount_sats: u64,
        minted_proofs: Option<Vec<Proof>>,
        signed_token: Option<SignedEvent>,
    },
    // PR-3 (send+redeem WAL wave, #2910 follow-up): `Send`/`Redeem` variants
    // carrying consumed proofs + the signed replacement token event land here
    // alongside their own write points. Intentionally not stubbed.
}

impl CashuWalPayload {
    /// Serialize to the opaque bytes the WAL store persists. `None` on a
    /// (practically unreachable) serde failure — the caller treats a failed
    /// encode as "no payload written", the same fail-open posture the saga
    /// write-through takes for a transient disk error (D6).
    fn encode(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(self).ok()
    }

    /// Decode a persisted payload; `None` on a corrupt/foreign blob, so a
    /// single bad payload skips its deposit rather than bricking restore
    /// (same corrupt-skip discipline as `restore_into_journal`).
    fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Write the current deposit payload for `quote_id` through to the durable WAL,
/// keyed under the pending deposit's operation id. A no-op when no WAL/account
/// is configured (in-memory-only parity) or the quote is unknown. Failures are
/// swallowed — the payload is a durability shadow and must never fail the
/// in-memory mutation that already succeeded (D6), exactly like
/// `CashuWalletState::wal_persist`'s saga write-through.
///
/// Call this at each of the three deposit write points, AFTER mutating the
/// in-memory [`PendingDeposit`] field, so the payload always reflects the same
/// state a resume would read: quote created (proofs/token `None`), mint `Ok`
/// (proofs set — the actual #2910 fix), token signed (token set).
pub(super) fn persist_deposit_payload(state: &CashuWalletState, quote_id: &str) {
    let (Some(store), Some(account)) = (state.wal_store.as_ref(), state.wal_account.as_ref()) else {
        return;
    };
    let Some(pending) = state.pending_deposits.get(quote_id) else {
        return;
    };
    let payload = CashuWalPayload::Deposit {
        quote_id: quote_id.to_string(),
        mint: pending.mint.clone(),
        amount_sats: pending.amount_sats,
        minted_proofs: pending.minted_proofs.clone(),
        signed_token: pending.signed_token.clone(),
    };
    if let Some(bytes) = payload.encode() {
        let _ = store.upsert_payload(account, &pending.operation_id, &bytes);
    }
}

/// A restored deposit that still needs its encrypt/sign/publish chain re-driven
/// — i.e. one whose payload carried `minted_proofs` and/or `signed_token`.
/// A quote-created-only deposit (both `None`) is NOT listed: repopulating its
/// `pending_deposits` entry is enough to unbreak `start_complete_deposit`'s
/// lookup, and re-driving it would prematurely poll the mint before the user
/// has even paid the invoice.
pub(super) struct ResumeDeposit {
    pub(super) operation_id: WalletOperationId,
    pub(super) quote_id: String,
    pub(super) mint: String,
}

/// Rebuild `pending_deposits` from the durable WAL for `account` and report
/// which deposits still need their chain re-driven. Called from
/// `CashuWalletBackend::restore_from_wal` AFTER `restore_into_journal` has
/// rehydrated the saga journal and self-healed terminal rows — so the store
/// here holds only the non-terminal operations, and every deposit op read back
/// is one whose lookup `start_complete_deposit` must be able to satisfy again.
///
/// Deposits whose payload decodes to `minted_proofs`/`signed_token` set are
/// returned as [`ResumeDeposit`]s for the caller to re-enqueue as
/// `ResumeDepositCommand`s (which run the same chain the in-process
/// `DepositResume` retry path does, off the actor thread per D8). A
/// quote-created-only deposit is repopulated but not returned (see
/// [`ResumeDeposit`]).
pub(super) fn restore_deposits(
    state: &mut CashuWalletState,
    store: &dyn WalletWalStore,
    account: &str,
) -> Vec<ResumeDeposit> {
    let mut resumes = Vec::new();
    let Ok(operations) = store.load_operations(account) else {
        return resumes;
    };
    for op in operations {
        if op.kind != WalletOperationKind::DepositCashu || op.state.is_terminal() {
            continue;
        }
        let Ok(Some(bytes)) = store.load_payload(account, &op.id) else {
            continue;
        };
        let Some(CashuWalPayload::Deposit {
            quote_id,
            mint,
            amount_sats,
            minted_proofs,
            signed_token,
        }) = CashuWalPayload::decode(&bytes)
        else {
            continue;
        };
        let needs_redrive = minted_proofs.is_some() || signed_token.is_some();
        state.pending_deposits.insert(
            quote_id.clone(),
            PendingDeposit {
                operation_id: op.id.clone(),
                mint: mint.clone(),
                amount_sats,
                minted_proofs,
                // A lease is a transient, in-flight-attempt token — never
                // persisted, so a restored deposit starts with no attempt in
                // flight (the resume below stamps a fresh one if it re-drives).
                chain_started_at: None,
                signed_token,
            },
        );
        if needs_redrive {
            resumes.push(ResumeDeposit {
                operation_id: op.id,
                quote_id,
                mint,
            });
        }
    }
    resumes
}
