//! Deposit flow — mint kind:7375 token events from a paid Lightning invoice
//! (Cashu NUT-04 mint-quote / NUT-23 BOLT11).
//!
//! Requires the `native` feature — every operation here round-trips to the
//! mint over HTTP via [`crate::cashu::client`].

use crate::cashu::client::MintClient;
use crate::cashu::types::MintQuoteState;
use crate::error::Nip60Error;
use crate::history_event::{build_history_event, HistoryRecord};
use crate::token_event::{build_token_event, TokenRecord};

use super::Nip60WalletHandle;

impl Nip60WalletHandle {
    /// Initiate a deposit (mint tokens from a Lightning invoice).
    ///
    /// Returns the bolt11 invoice to pay. Call [`Self::complete_deposit`]
    /// with the returned `quote_id` once the invoice has been paid.
    pub fn initiate_deposit(&self, amount_sats: u64) -> Result<DepositRequest, Nip60Error> {
        let mint_url = self.primary_mint_url()?;
        let client = MintClient::new(&mint_url);
        let quote = client.create_mint_quote(amount_sats)?;
        Ok(DepositRequest {
            bolt11: quote.request,
            quote_id: quote.quote,
            mint_url,
            amount_sats,
        })
    }

    /// Check a mint quote and mint tokens if the invoice has been paid.
    ///
    /// This performs exactly one mint-status HTTP read and returns; it owns no
    /// sleep+check loop, so it stays D8-clean (no polling in library code).
    /// Returns `Err(Nip60Error::QuoteNotPaid)` if the invoice has not been paid
    /// yet — the caller decides when to re-check.
    ///
    /// # Why the caller re-checks (protocol constraint, not debt)
    ///
    /// The Cashu mint-quote flow (NUT-04 / NUT-23 BOLT11) is request/response:
    /// a quote advances `UNPAID → PAID → ISSUED` and the wallet learns of the
    /// transition only by re-reading `GET /v1/mint/quote/{method}/{quote_id}`.
    /// The base spec defines **no** push primitive (no webhook / callback / WS
    /// notification) for "invoice paid". So a single short library sleep loop
    /// here would just be hidden polling. Instead this returns `QuoteNotPaid`
    /// and leaves the *when-to-re-check* policy to the kernel, which can drive
    /// it from a wall-clock-gated observer rather than a busy-wait. For testnut,
    /// invoices are auto-paid within milliseconds; a single re-check suffices.
    ///
    /// The new kind:7375 token event and kind:7376 history event are queued in
    /// the outbox for the kernel to publish.
    pub fn complete_deposit(&self, deposit: &DepositRequest) -> Result<u64, Nip60Error> {
        let client = MintClient::new(&deposit.mint_url);
        let status = client.get_mint_quote_status(&deposit.quote_id)?;
        if status.state != MintQuoteState::Paid {
            return Err(Nip60Error::QuoteNotPaid);
        }

        // Mint tokens.
        let keyset = client.get_sat_keyset()?;
        let proofs = client.mint_tokens(&deposit.quote_id, deposit.amount_sats, &keyset)?;
        let total: u64 = proofs.iter().map(|p| p.amount).sum();

        // Save proofs as a new token event.
        let record = TokenRecord::new(deposit.mint_url.clone(), proofs);
        let event_builder = build_token_event(&record, &self.keys)?;
        let event = event_builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign token event: {e}")))?;
        let token_event_id = event.id;
        self.enqueue(event);

        // Update in-memory state.
        let mut record_with_id = record;
        record_with_id.event_id = Some(token_event_id);
        self.tokens.lock().unwrap().push(record_with_id);

        // Queue history event (direction: in).
        let mut history = HistoryRecord::new_in(total);
        history.created.push(token_event_id);
        if let Ok(h_builder) = build_history_event(&history, &self.keys) {
            if let Ok(h_event) = h_builder.sign_with_keys(&self.keys) {
                self.enqueue(h_event);
            }
        }

        Ok(total)
    }

    fn primary_mint_url(&self) -> Result<String, Nip60Error> {
        self.config
            .lock()
            .unwrap()
            .mints
            .first()
            .cloned()
            .ok_or(Nip60Error::NotInitialised)
    }
}

/// A pending deposit request (bolt11 invoice + quote id).
#[derive(Clone)]
pub struct DepositRequest {
    /// The bolt11 Lightning invoice to pay. Secret-adjacent: identifies a
    /// specific payment; never printed in `Debug`.
    pub bolt11: String,
    /// Mint quote id — pass to [`Nip60WalletHandle::complete_deposit`] once
    /// paid. Secret-adjacent: never printed in `Debug`.
    pub quote_id: String,
    /// Mint URL this deposit is for.
    pub mint_url: String,
    /// Requested amount in sats.
    pub amount_sats: u64,
}

impl std::fmt::Debug for DepositRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepositRequest")
            .field("bolt11", &"<redacted>")
            .field("quote_id", &"<redacted>")
            .field("mint_url", &self.mint_url)
            .field("amount_sats", &self.amount_sats)
            .finish()
    }
}
