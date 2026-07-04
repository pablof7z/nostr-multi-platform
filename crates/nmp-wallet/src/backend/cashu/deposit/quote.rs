//! [`CashuDepositQuoteCommand`] — phase one of the two-phase Cashu deposit
//! flow: request a NUT-04 mint quote (a bolt11 invoice). Moves no value; see
//! the module-level docs on [`super`] for the split rationale.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{
    build_record_action_success, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_nip60::cashu::MintClient;

use crate::journal::{WalletOperationId, WalletOperationState};

use super::super::state::{lock_state, CashuWalletState, PendingDeposit};
use super::super::ui_codes;
use super::fail;

pub(in crate::backend::cashu) struct CashuDepositQuoteCommand {
    pub(in crate::backend::cashu) state: Arc<Mutex<CashuWalletState>>,
    pub(in crate::backend::cashu) operation_id: WalletOperationId,
    pub(in crate::backend::cashu) mint: String,
    pub(in crate::backend::cashu) amount_sats: u64,
    pub(in crate::backend::cashu) correlation_id: Option<String>,
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
                                minted_proofs: None,
                                signed_token: None,
                                chain_started_at: None,
                            },
                        );
                        let _ = guard.transition(&operation_id, WalletOperationState::MintSettled);
                        // Durable WAL payload write (PR-2 of #2910): persist the
                        // quote-created deposit (no proofs/token yet) so a
                        // process restart after the invoice is paid but before
                        // completion rebuilds `pending_deposits` and unbreaks
                        // `start_complete_deposit`'s lookup. This alone fixes the
                        // everyday "paid, then restarted" case.
                        super::super::wal_payload::persist_deposit_payload(&guard, &quote_id);
                    }
                    // The action-result channel (NOT the bounded projection,
                    // NOT a log line — see module docs) is how the caller
                    // learns the invoice to pay and the quote_id to name back
                    // to `CompleteDepositCashu`.
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
                    fail(
                        &worker_tx,
                        correlation_id,
                        ui_codes::MINT_QUOTE_FAILED,
                        format!("mint quote request failed: {e}"),
                    );
                }
            }
        });
        Ok(())
    }
}
