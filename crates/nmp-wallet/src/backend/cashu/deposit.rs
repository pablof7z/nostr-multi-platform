//! `DepositQuote` / `CompleteDeposit` — the two-phase Cashu deposit flow
//! (#2895 W2), split because the two NUT-04 mint HTTP calls happen at
//! different times and only one of them moves value:
//!
//! - [`CashuDepositQuoteCommand`] requests a mint quote (a bolt11 invoice).
//!   Never moves value — no proofs are consumed or minted — so it can
//!   complete before any invoice is paid. Persists the quote to the journal
//!   (`MintPending` -> `MintSettled`) and surfaces `{quote_id, bolt11, mint,
//!   amount_sats}` through the action's `RecordActionSuccess` result JSON —
//!   the one-shot channel a caller keeps to pay the invoice and later name
//!   `quote_id` back to `CompleteDeposit`. Never the bounded
//!   `WalletProjection` or a log line (quote ids are secret-adjacent).
//! - [`CashuCompleteDepositCommand`] checks the quote's paid state (testnut:
//!   already `Paid`; a real mint: `Paid` once the invoice above is settled
//!   externally), then mints tokens — THE value-moving call — and writes the
//!   resulting proofs as a NIP-44 self-encrypted kind:7375 token event via the
//!   same signer-transparent chain `create_wallet.rs` uses.
//!
//! Both HTTP round-trips run on a spawned worker thread (D8 — the actor
//! thread never blocks on mint HTTP); the worker writes results back into the
//! shared [`CashuWalletState`] directly, mirroring how `NwcWalletBackend`'s
//! runtime is the sole writer of its `WalletStatusSlot`.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    build_record_action_failure, build_record_action_success, ProtocolCommand,
    ProtocolCommandContext, ProtocolCommandError,
};
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::{MintQuoteState, Proof};
use nmp_nip60::cashu::MintClient;
use nmp_nip60::KIND_NIP60_TOKEN;
use nmp_signer_iface::SignedEvent;

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, ProofRef, Provenance, WalletEventId, WalletFact,
    WalletOperationId, WalletOperationState, WalletUnit,
};

use super::chain::launch_self_encrypted_publish;
use super::state::{lock_state, CashuWalletState, PendingDeposit};
use super::ui_codes;

// ─── DepositQuote ───────────────────────────────────────────────────────────

pub(super) struct CashuDepositQuoteCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) mint: String,
    pub(super) amount_sats: u64,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for CashuDepositQuoteCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CashuDepositQuoteCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish()
    }
}

impl ProtocolCommand for CashuDepositQuoteCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            mint,
            amount_sats,
            correlation_id,
        } = *self;
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            let client = MintClient::new(&mint);
            match client.create_mint_quote(amount_sats) {
                Ok(quote) => {
                    let quote_id = quote.quote.clone();
                    let bolt11 = quote.request.clone();
                    {
                        let mut guard = lock_state(&state);
                        guard.pending_deposits.insert(
                            quote_id.clone(),
                            PendingDeposit {
                                operation_id: operation_id.clone(),
                                mint: mint.clone(),
                                amount_sats,
                            },
                        );
                        let _ = guard.transition(&operation_id, WalletOperationState::MintSettled);
                    }
                    // The action-result channel (NOT the bounded projection,
                    // NOT a log line — see module docs) is how the caller
                    // learns the invoice to pay and the quote_id to name back
                    // to `CompleteDeposit`.
                    if let Some(id) = correlation_id {
                        let result_json = serde_json::json!({
                            "quote_id": quote_id,
                            "bolt11": bolt11,
                            "mint": mint,
                            "amount_sats": amount_sats,
                        })
                        .to_string();
                        let _ = worker_tx.send(build_record_action_success(id, Some(result_json)));
                    }
                }
                Err(e) => {
                    {
                        let mut guard = lock_state(&state);
                        let _ = guard.transition(&operation_id, WalletOperationState::Failed);
                    }
                    fail(&worker_tx, correlation_id, ui_codes::MINT_QUOTE_FAILED, format!("mint quote request failed: {e}"));
                }
            }
        });
        Ok(())
    }
}

// ─── CompleteDeposit ────────────────────────────────────────────────────────

pub(super) struct CashuCompleteDepositCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) quote_id: String,
    pub(super) mint: String,
    pub(super) amount_sats: u64,
    pub(super) account_pubkey: String,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for CashuCompleteDepositCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CashuCompleteDepositCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish()
    }
}

impl ProtocolCommand for CashuCompleteDepositCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            quote_id,
            mint,
            amount_sats,
            account_pubkey,
            correlation_id,
        } = *self;
        let relays = ctx.recipient_publish_relays(&account_pubkey, KIND_NIP60_TOKEN);
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            let client = MintClient::new(&mint);
            let status = match client.get_mint_quote_status(&quote_id) {
                Ok(s) => s,
                Err(e) => {
                    fail(
                        &worker_tx,
                        correlation_id,
                        ui_codes::MINT_QUOTE_FAILED,
                        format!("mint quote status check failed: {e}"),
                    );
                    return;
                }
            };
            if status.state != MintQuoteState::Paid {
                // Retryable, not a hard failure — the caller pays the
                // invoice (testnut auto-settles almost immediately) and
                // retries `CompleteDeposit`. The operation stays at
                // `MintSettled`; no journal transition here.
                fail(
                    &worker_tx,
                    correlation_id,
                    ui_codes::QUOTE_NOT_PAID,
                    "mint quote not yet paid — pay the invoice and retry".to_string(),
                );
                return;
            }
            let keyset = match client.get_sat_keyset() {
                Ok(k) => k,
                Err(e) => {
                    fail_and_terminate_operation(&state, &operation_id, &worker_tx, correlation_id, format!("mint keyset fetch failed: {e}"));
                    return;
                }
            };
            // THE value-moving call. The operation's `MintSettled` pre-record
            // (written back durably by `CashuDepositQuoteCommand`, before any
            // HTTP happened) already satisfies the at-most-once guarantee —
            // deposits consume zero local proofs (see
            // `WalletOperationKind::requires_consumed_inputs_before_mint_request`),
            // so there is nothing further to pre-record before this call.
            let proofs = match client.mint_tokens(&quote_id, amount_sats, &keyset) {
                Ok(p) => p,
                Err(e) => {
                    fail_and_terminate_operation(&state, &operation_id, &worker_tx, correlation_id, format!("mint-tokens request failed: {e}"));
                    return;
                }
            };
            dispatch_token_event(
                worker_tx,
                state,
                operation_id,
                quote_id,
                mint,
                proofs,
                account_pubkey,
                relays,
                correlation_id,
            );
        });
        Ok(())
    }
}

/// Build the kind:7375 self-encrypted token event for freshly minted `proofs`
/// and launch the encrypt -> sign -> publish chain. Factored out of `run`'s
/// worker closure so it is directly callable with synthetic proofs — no live
/// mint or DHKE math needed to test the ledger/journal wiring this function
/// owns (the DHKE unblind+verify math itself is `nmp-nip60`'s own tested
/// surface; this function only calls it).
#[allow(clippy::too_many_arguments)]
fn dispatch_token_event(
    worker_tx: CommandSender,
    state: Arc<Mutex<CashuWalletState>>,
    operation_id: WalletOperationId,
    quote_id: String,
    mint: String,
    proofs: Vec<Proof>,
    account_pubkey: String,
    relays: Vec<String>,
    correlation_id: Option<String>,
) {
    let plaintext = token_event_plaintext(&mint, &proofs);
    // ProofRef is the proof's public `C` value (never the secret) — matches
    // `ProofRef`'s own doc comment. `amount_msat` here is sats*1000: the
    // ledger's `ProofAtom` field is msat-denominated (future unit-agnostic
    // room); the "sat" unit this deposit flow uses converts at the edge.
    let proof_atoms: Vec<ProofAtom> = proofs
        .iter()
        .map(|p| ProofAtom {
            proof: ProofRef::new(p.c.clone()),
            amount_msat: p.amount.saturating_mul(1000),
        })
        .collect();
    let on_signed_state = Arc::clone(&state);
    let on_signed_op = operation_id.clone();
    let mint_for_fact = mint.clone();
    launch_self_encrypted_publish(
        worker_tx,
        account_pubkey,
        KIND_NIP60_TOKEN,
        plaintext,
        relays,
        correlation_id,
        move |_tx, signed: &SignedEvent| {
            let mut guard = lock_state(&on_signed_state);
            guard.ledger.apply(WalletFact::TokenAdded {
                token_event: WalletEventId::new(signed.id.clone()),
                mint: MintUrl::new(mint_for_fact),
                unit: WalletUnit::new("sat"),
                proofs: proof_atoms,
                via: Provenance::Saga(CorrelationId::new(on_signed_op.as_str())),
            });
            let _ = guard.transition(&on_signed_op, WalletOperationState::PublishPending);
            guard.pending_deposits.remove(&quote_id);
        },
    );
}

fn token_event_plaintext(mint: &str, proofs: &[Proof]) -> String {
    serde_json::json!({
        "mint": mint,
        "proofs": proofs,
        "del": Vec::<String>::new(),
    })
    .to_string()
}

fn fail_and_terminate_operation(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    worker_tx: &CommandSender,
    correlation_id: Option<String>,
    reason: String,
) {
    {
        let mut guard = lock_state(state);
        let _ = guard.transition(operation_id, WalletOperationState::Failed);
    }
    fail(worker_tx, correlation_id, ui_codes::MINT_TOKENS_FAILED, reason);
}

fn fail(
    worker_tx: &CommandSender,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) {
    let token = UiToken::error(code, reason.clone());
    let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
    if let Some(id) = correlation_id {
        let _ = worker_tx.send(build_record_action_failure(id, reason));
    }
}
