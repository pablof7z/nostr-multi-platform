//! High-level NIP-60 wallet — ties together event encoding, the Cashu mint
//! HTTP client, and the `WalletBackend` trait implementation.
//!
//! # Relay I/O is the kernel's job
//!
//! This crate owns ZERO relay I/O. It never opens a socket and never hardcodes
//! a relay URL. Instead it follows NMP's single-chokepoint doctrine:
//!
//! - **Reads** — the kernel fetches the wallet's events through its
//!   `EventStore` / interest pipeline and hands them to this handle via the
//!   `ingest_*` methods ([`Nip60WalletHandle::from_wallet_event`],
//!   [`Nip60WalletHandle::ingest_token_events`],
//!   [`Nip60WalletHandle::ingest_history_events`]).
//! - **Writes** — every operation that needs to publish builds and signs the
//!   event, then queues it in the handle's outbox. The kernel drains the
//!   outbox via [`Nip60WalletHandle::take_outbox`] and publishes each event
//!   through its `ActorCommand::Publish*` chokepoint.
//!
//! The `relays` field is wallet *metadata* (the relay URLs listed in the
//! kind:17375 config) the kernel uses to scope its interests and publishes —
//! it is not a connection handle.
//!
//! # Typical usage
//!
//! ```rust,no_run
//! use nmp_nip60::Nip60WalletHandle;
//! use nostr::Keys;
//!
//! let keys = Keys::generate();
//! // Create a new wallet. The signed kind:17375 wallet event lands in the
//! // outbox for the kernel to publish.
//! let wallet = Nip60WalletHandle::create_new(
//!     &keys,
//!     "https://testnut.cashu.space",
//!     Vec::new(),
//! ).expect("wallet creation");
//!
//! let to_publish = wallet.take_outbox();
//! println!("balance: {} sat", wallet.balance_sats());
//! println!("{} event(s) queued for the kernel to publish", to_publish.len());
//! ```

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use nostr::{Event, EventId, Keys, PublicKey};
use tracing::info;

use crate::backend::{PayResult, WalletBackend, WalletError};
use crate::cashu::client::{split_amount, MintClient, self as cashu_client};
use crate::cashu::types::Proof;
use crate::error::Nip60Error;
use crate::history_event::{build_history_event, redeemed_nutzap_ids, HistoryRecord};
use crate::nutzap::{
    build_nutzap_info_event, p2pk_secret, NutZapInfo, NutZapProof,
};
use crate::token_event::{build_token_event, decode_token_event, TokenRecord};
use crate::wallet_event::{build_wallet_event, decode_wallet_event, WalletConfig};

/// The in-memory NIP-60 wallet handle.
///
/// Thread-safe via interior `Mutex`. All mutating operations lock briefly.
/// Relay I/O is never performed here — see the module docs.
#[derive(Clone)]
pub struct Nip60WalletHandle {
    keys: Keys,
    config: Arc<Mutex<WalletConfig>>,
    tokens: Arc<Mutex<Vec<TokenRecord>>>,
    redeemed: Arc<Mutex<HashSet<EventId>>>,
    relays: Vec<String>,
    /// Signed events awaiting publication by the kernel. Drained via
    /// [`Self::take_outbox`].
    outbox: Arc<Mutex<Vec<Event>>>,
}

impl Nip60WalletHandle {
    // ── Construction ───────────────────────────────────────────────────────

    /// Create a brand-new NIP-60 wallet. Builds and signs the kind:17375
    /// wallet event and queues it in the outbox for the kernel to publish,
    /// then returns the wallet handle.
    pub fn create_new(
        keys: &Keys,
        mint_url: &str,
        relays: Vec<String>,
    ) -> Result<Self, Nip60Error> {
        let config = WalletConfig::generate(vec![mint_url.to_string()], relays.clone());
        let wallet_builder = build_wallet_event(&config, keys)?;
        let wallet_event = wallet_builder
            .sign_with_keys(keys)
            .map_err(|e| Nip60Error::Event(format!("sign wallet event: {e}")))?;

        let handle = Self {
            keys: keys.clone(),
            config: Arc::new(Mutex::new(config)),
            tokens: Arc::new(Mutex::new(Vec::new())),
            redeemed: Arc::new(Mutex::new(HashSet::new())),
            relays,
            outbox: Arc::new(Mutex::new(Vec::new())),
        };
        handle.enqueue(wallet_event);
        Ok(handle)
    }

    /// Build a wallet handle from a kind:17375 wallet event the kernel already
    /// fetched from its store.
    ///
    /// The wallet's relay list comes exclusively from the event's own `relay`
    /// tags. After construction, feed the wallet's token and history events
    /// through [`Self::ingest_token_events`] and
    /// [`Self::ingest_history_events`] to populate balance and redemption
    /// state.
    pub fn from_wallet_event(keys: &Keys, wallet_event: &Event) -> Result<Self, Nip60Error> {
        let config = decode_wallet_event(wallet_event, keys.secret_key(), &keys.public_key())?;
        let relays = config.relays.clone();
        Ok(Self {
            keys: keys.clone(),
            config: Arc::new(Mutex::new(config)),
            tokens: Arc::new(Mutex::new(Vec::new())),
            redeemed: Arc::new(Mutex::new(HashSet::new())),
            relays,
            outbox: Arc::new(Mutex::new(Vec::new())),
        })
    }

    // ── Outbox (kernel publishes these) ─────────────────────────────────────

    /// Queue a signed event for the kernel to publish through its
    /// `ActorCommand::Publish*` chokepoint. This crate never opens a socket.
    fn enqueue(&self, event: Event) {
        self.outbox.lock().unwrap().push(event);
    }

    /// Drain every event awaiting publication. The kernel calls this after an
    /// operation and dispatches each event through its publish chokepoint.
    pub fn take_outbox(&self) -> Vec<Event> {
        std::mem::take(&mut *self.outbox.lock().unwrap())
    }

    // ── Balance ────────────────────────────────────────────────────────────

    pub fn balance_sats(&self) -> u64 {
        self.tokens
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.balance())
            .sum()
    }

    // ── Ingest (kernel-fetched events) ──────────────────────────────────────

    /// Replace the in-memory token set from kind:7375 token events the kernel
    /// fetched through its store/interest pipeline.
    pub fn ingest_token_events(&self, events: &[Event]) -> Result<(), Nip60Error> {
        // Deduplicate by event id.
        let mut all_events: Vec<&Event> = events.iter().collect();
        all_events.sort_by_key(|e| e.id);
        all_events.dedup_by_key(|e| e.id);

        let mut records = Vec::new();
        for evt in all_events {
            match decode_token_event(evt, self.keys.secret_key(), &self.keys.public_key()) {
                Ok(r) => records.push(r),
                Err(e) => tracing::warn!("skip malformed token event {}: {e}", evt.id),
            }
        }
        *self.tokens.lock().unwrap() = records;
        Ok(())
    }

    /// Learn which nutzaps have already been redeemed from kind:7376 history
    /// events the kernel fetched.
    pub fn ingest_history_events(&self, events: &[Event]) {
        let mut redeemed = HashSet::new();
        for event in events {
            redeemed.extend(redeemed_nutzap_ids(event));
        }
        *self.redeemed.lock().unwrap() = redeemed;
    }

    /// Event IDs of nutzaps already redeemed by this wallet.
    pub fn redeemed_nutzap_ids(&self) -> Vec<EventId> {
        let mut ids: Vec<EventId> = self.redeemed.lock().unwrap().iter().copied().collect();
        ids.sort();
        ids
    }

    /// Whether this wallet already redeemed the given nutzap event.
    pub fn has_redeemed_nutzap(&self, event_id: EventId) -> bool {
        self.redeemed.lock().unwrap().contains(&event_id)
    }

    fn mark_redeemed_nutzap(&self, event_id: EventId) {
        self.redeemed.lock().unwrap().insert(event_id);
    }

    // ── Deposit ────────────────────────────────────────────────────────────

    /// Initiate a deposit (mint tokens from a Lightning invoice).
    ///
    /// Returns the bolt11 invoice to pay. Call [`Self::complete_deposit`]
    /// with the returned `quote_id` once the invoice has been paid.
    pub fn initiate_deposit(
        &self,
        amount_sats: u64,
    ) -> Result<DepositRequest, Nip60Error> {
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
    pub fn complete_deposit(
        &self,
        deposit: &DepositRequest,
    ) -> Result<u64, Nip60Error> {
        let client = MintClient::new(&deposit.mint_url);
        let status = client.get_mint_quote_status(&deposit.quote_id)?;
        if status.state != "PAID" && !status.paid {
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

    // ── NutZap send ────────────────────────────────────────────────────────

    /// Send a NutZap to a recipient.
    ///
    /// The caller supplies the recipient's kind:10019 NutZap info (which the
    /// kernel fetched from its store). This method swaps proofs for
    /// P2PK-locked proofs at the mint, then queues the kind:9321 nutzap event
    /// in the outbox for the kernel to publish.
    ///
    /// Returns the queued nutzap event id.
    pub fn send_nutzap(
        &self,
        amount_sats: u64,
        recipient_pubkey: &PublicKey,
        nutzap_info: &NutZapInfo,
        comment: Option<&str>,
        zapped_event_id: Option<&EventId>,
    ) -> Result<EventId, Nip60Error> {
        let recipient_mint = nutzap_info
            .mints
            .first()
            .ok_or_else(|| {
                Nip60Error::MintDiscovery("recipient has no accepted mints in NutZap info".into())
            })?
            .clone();
        let recipient_cashu_pubkey = nutzap_info
            .cashu_pubkey
            .clone()
            .unwrap_or_else(|| recipient_pubkey.to_hex());

        // Get P2PK locked proofs.
        let proofs = self.create_p2pk_proofs(amount_sats, &recipient_cashu_pubkey, &recipient_mint)?;
        let nutzap_proofs: Vec<NutZapProof> = proofs.into_iter().map(Into::into).collect();

        // Build and queue nutzap event.
        let nutzap_builder = crate::nutzap::build_nutzap_event(
            nutzap_proofs,
            &recipient_mint,
            recipient_pubkey,
            comment,
            zapped_event_id,
        )?;
        let nutzap_event = nutzap_builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign nutzap: {e}")))?;
        let nutzap_event_id = nutzap_event.id;
        self.enqueue(nutzap_event);

        info!(
            "queued nutzap: {} sat to {}",
            amount_sats,
            recipient_pubkey.to_hex()
        );
        Ok(nutzap_event_id)
    }

    // ── NutZap info (kind:10019) ───────────────────────────────────────────

    /// Build the user's kind:10019 NutZap info event and queue it in the
    /// outbox for the kernel to publish. Returns the queued event id.
    pub fn publish_nutzap_info(&self) -> Result<EventId, Nip60Error> {
        let config = self.config.lock().unwrap().clone();
        let cashu_pubkey = config.pubkey_hex()?;
        let info = NutZapInfo {
            relays: self.relays.clone(),
            mints: config.mints.clone(),
            cashu_pubkey: Some(cashu_pubkey),
        };
        let builder = build_nutzap_info_event(&info, &self.keys)?;
        let event = builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign nutzap info: {e}")))?;
        let event_id = event.id;
        self.enqueue(event);
        Ok(event_id)
    }

    // ── Helpers ────────────────────────────────────────────────────────────

    fn primary_mint_url(&self) -> Result<String, Nip60Error> {
        self.config
            .lock()
            .unwrap()
            .mints
            .first()
            .cloned()
            .ok_or(Nip60Error::NotInitialised)
    }

    fn create_p2pk_proofs(
        &self,
        amount_sats: u64,
        recipient_cashu_pubkey: &str,
        mint_url: &str,
    ) -> Result<Vec<Proof>, Nip60Error> {
        // Select proofs from our wallet to cover the amount.
        let (selected, _change_amount) = self.select_proofs(amount_sats, mint_url)?;
        let selected_total: u64 = selected.iter().map(|p| p.amount).sum();

        let client = MintClient::new(mint_url);
        let keyset = client.get_sat_keyset()?;

        // Build P2PK locked secrets for the output denominations.
        let output_amounts = split_amount(amount_sats);
        let p2pk_secrets: Vec<String> = output_amounts
            .iter()
            .map(|_| p2pk_secret(recipient_cashu_pubkey))
            .collect();

        // Compute mint fee and deduct from change.
        let fee = cashu_client::MintClient::compute_fee(selected.len() as u64, keyset.input_fee_ppk);
        let gross_change = selected_total - amount_sats;
        if gross_change < fee {
            return Err(Nip60Error::InsufficientBalance {
                have: selected_total,
                need: amount_sats + fee,
            });
        }
        let change_amount = gross_change - fee;

        let mut all_output_amounts = output_amounts.clone();
        let mut all_secrets: Vec<String> = p2pk_secrets.clone();
        if change_amount > 0 {
            let change_denoms = split_amount(change_amount);
            all_output_amounts.extend_from_slice(&change_denoms);
            for _ in &change_denoms {
                all_secrets.push(hex::encode(crate::cashu::crypto::random_secret()));
            }
        }

        let new_proofs = client.swap(
            selected.clone(),
            all_output_amounts.clone(),
            Some(all_secrets),
            &keyset,
        )?;

        // Split into nutzap proofs and change proofs.
        let nutzap_count = output_amounts.len();
        let nutzap_proofs: Vec<Proof> = new_proofs[..nutzap_count].to_vec();
        let change_proofs: Vec<Proof> = new_proofs[nutzap_count..].to_vec();

        // Delete spent token events and save change.
        self.update_tokens_after_spend(&selected, change_proofs, mint_url)?;

        Ok(nutzap_proofs)
    }

    /// Select proofs summing to at least `amount_sats` from `mint_url`.
    fn select_proofs(
        &self,
        amount_sats: u64,
        mint_url: &str,
    ) -> Result<(Vec<Proof>, u64), Nip60Error> {
        let tokens = self.tokens.lock().unwrap();
        let mut selected = Vec::new();
        let mut total = 0u64;

        for record in tokens.iter().filter(|r| r.mint_url == mint_url) {
            for proof in &record.proofs {
                if total >= amount_sats {
                    break;
                }
                selected.push(proof.clone());
                total += proof.amount;
            }
            if total >= amount_sats {
                break;
            }
        }
        drop(tokens);

        if total < amount_sats {
            return Err(Nip60Error::InsufficientBalance {
                have: total,
                need: amount_sats,
            });
        }
        Ok((selected, total - amount_sats))
    }

    /// Update token events after spending: queue a NIP-09 deletion for each
    /// fully-spent token event, queue a change token event, and refresh
    /// in-memory state. Queued events are published by the kernel.
    fn update_tokens_after_spend(
        &self,
        spent_proofs: &[Proof],
        change_proofs: Vec<Proof>,
        mint_url: &str,
    ) -> Result<(), Nip60Error> {
        let spent_secrets: std::collections::HashSet<&str> =
            spent_proofs.iter().map(|p| p.secret.as_str()).collect();

        let mut tokens = self.tokens.lock().unwrap();
        let mut destroyed_event_ids: Vec<EventId> = Vec::new();
        let mut new_tokens: Vec<TokenRecord> = Vec::new();
        let mut queued: Vec<Event> = Vec::new();

        for record in tokens.drain(..) {
            let remaining: Vec<Proof> = record
                .proofs
                .into_iter()
                .filter(|p| !spent_secrets.contains(p.secret.as_str()))
                .collect();
            if remaining.is_empty() {
                if let Some(id) = record.event_id {
                    destroyed_event_ids.push(id);
                    // Queue deletion event (NIP-09 — kind:5).
                    if let Ok(del_builder) = self.build_delete_event(id) {
                        if let Ok(del_event) = del_builder.sign_with_keys(&self.keys) {
                            queued.push(del_event);
                        }
                    }
                }
            } else {
                new_tokens.push(TokenRecord {
                    mint_url: record.mint_url,
                    proofs: remaining,
                    del: record.del,
                    event_id: record.event_id,
                });
            }
        }

        // Queue change token event.
        if !change_proofs.is_empty() {
            let change_record = TokenRecord::new(mint_url.to_string(), change_proofs.clone());
            let builder = build_token_event(&change_record, &self.keys)?;
            let event = builder
                .sign_with_keys(&self.keys)
                .map_err(|e| Nip60Error::Event(format!("sign change token: {e}")))?;
            let id = event.id;
            queued.push(event);
            new_tokens.push(TokenRecord {
                mint_url: mint_url.to_string(),
                proofs: change_proofs,
                del: destroyed_event_ids.iter().map(|id| id.to_hex()).collect(),
                event_id: Some(id),
            });
        }

        *tokens = new_tokens;
        drop(tokens);

        for event in queued {
            self.enqueue(event);
        }
        Ok(())
    }

    fn build_delete_event(&self, event_id: EventId) -> Result<nostr::EventBuilder, Nip60Error> {
        use nostr::{EventBuilder, Kind, Tag};
        Ok(EventBuilder::new(Kind::EventDeletion, "spent")
            .tag(Tag::event(event_id)))
    }

    // ── NutZap receive ─────────────────────────────────────────────────────

    /// Redeem a received nutzap: swap the P2PK proofs for fresh proofs and
    /// queue a kind:7375 token event plus a kind:7376 history event marking it
    /// redeemed. Queued events are published by the kernel.
    pub fn redeem_nutzap(
        &self,
        nutzap: &crate::nutzap::ReceivedNutZap,
    ) -> Result<u64, Nip60Error> {
        if self.has_redeemed_nutzap(nutzap.event_id) {
            return Err(Nip60Error::AlreadyRedeemed(nutzap.event_id));
        }

        // Sign P2PK proofs with our Cashu private key.
        let config = self.config.lock().unwrap().clone();
        let cashu_sk_bytes = hex::decode(&config.privkey_hex)
            .map_err(|e| Nip60Error::Crypto(format!("cashu privkey: {e}")))?;
        let cashu_sk = nostr::secp256k1::SecretKey::from_slice(&cashu_sk_bytes)
            .map_err(|e| Nip60Error::Crypto(format!("cashu privkey parse: {e}")))?;

        let input_proofs: Vec<Proof> = nutzap
            .proofs
            .iter()
            .map(|np| {
                let proof = Proof {
                    amount: np.amount,
                    id: np.id.clone(),
                    secret: np.secret.clone(),
                    c: np.c.clone(),
                    dleq: np.dleq.clone(),
                    witness: None,
                };
                crate::nutzap::sign_p2pk_proof(&proof, &cashu_sk)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let total: u64 = input_proofs.iter().map(|p| p.amount).sum();

        let client = MintClient::new(&nutzap.mint_url);
        let keyset = client.get_sat_keyset()?;
        let fee = cashu_client::MintClient::compute_fee(input_proofs.len() as u64, keyset.input_fee_ppk);
        let net_total = total.saturating_sub(fee);
        let output_amounts = split_amount(net_total);

        let fresh_proofs = client.swap(input_proofs, output_amounts, None, &keyset)?;

        // Save fresh proofs as a new token event.
        let record = TokenRecord::new(nutzap.mint_url.clone(), fresh_proofs);
        let builder = build_token_event(&record, &self.keys)?;
        let event = builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign redeemed token: {e}")))?;
        let new_event_id = event.id;
        self.enqueue(event);

        // Update in-memory state.
        let mut record_with_id = record;
        record_with_id.event_id = Some(new_event_id);
        self.tokens.lock().unwrap().push(record_with_id);

        // Queue history event.
        let mut history = HistoryRecord::new_in(total);
        history.created.push(new_event_id);
        history.redeemed.push(nutzap.event_id);
        if let Ok(h_builder) = build_history_event(&history, &self.keys) {
            if let Ok(h_event) = h_builder.sign_with_keys(&self.keys) {
                self.enqueue(h_event);
            }
        }
        self.mark_redeemed_nutzap(nutzap.event_id);

        info!("redeemed nutzap: {total} sat from {}", nutzap.sender_pubkey.to_hex());
        Ok(total)
    }

    pub fn relays(&self) -> &[String] {
        &self.relays
    }

    pub fn pubkey(&self) -> PublicKey {
        self.keys.public_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nutzap::ReceivedNutZap;

    fn empty_wallet() -> Nip60WalletHandle {
        Nip60WalletHandle::create_new(&Keys::generate(), "https://mint.example", Vec::new())
            .expect("wallet")
    }

    #[test]
    fn create_new_queues_wallet_event_in_outbox() {
        let wallet = empty_wallet();
        let queued = wallet.take_outbox();
        assert_eq!(queued.len(), 1, "kind:17375 wallet event should be queued");
        // Outbox is drained on take.
        assert!(wallet.take_outbox().is_empty());
    }

    #[test]
    fn redeemed_nutzap_ids_are_queryable() {
        let wallet = empty_wallet();
        let event_id = EventId::from_byte_array([3u8; 32]);

        wallet.mark_redeemed_nutzap(event_id);

        assert!(wallet.has_redeemed_nutzap(event_id));
        assert_eq!(wallet.redeemed_nutzap_ids(), vec![event_id]);
    }

    #[test]
    fn redeem_nutzap_short_circuits_before_mint_for_known_event() {
        let wallet = empty_wallet();
        let event_id = EventId::from_byte_array([5u8; 32]);
        wallet.mark_redeemed_nutzap(event_id);

        let nutzap = ReceivedNutZap {
            event_id,
            sender_pubkey: Keys::generate().public_key(),
            proofs: Vec::new(),
            mint_url: "http://127.0.0.1:1".to_string(),
            amount_sats: 0,
            comment: String::new(),
            zapped_event_id: None,
        };

        let err = wallet.redeem_nutzap(&nutzap).expect_err("already redeemed");

        assert!(matches!(
            err,
            Nip60Error::AlreadyRedeemed(already_redeemed) if already_redeemed == event_id
        ));
    }
}

// ─── WalletBackend impl ────────────────────────────────────────────────────

impl WalletBackend for Nip60WalletHandle {
    fn balance_sats(&self) -> u64 {
        self.balance_sats()
    }

    fn pay_invoice(&self, _bolt11: &str) -> Result<PayResult, WalletError> {
        // TODO: implement melt (NUT-05) for paying Lightning invoices via ecash.
        Err(WalletError::Unsupported)
    }

    fn create_nutzap_proofs(
        &self,
        amount_sats: u64,
        recipient_cashu_pubkey: &str,
        mint_url: &str,
    ) -> Result<Vec<NutZapProof>, WalletError> {
        let proofs = self.create_p2pk_proofs(amount_sats, recipient_cashu_pubkey, mint_url)?;
        Ok(proofs.into_iter().map(Into::into).collect())
    }

    fn backend_type(&self) -> &'static str {
        "nip60"
    }
}

/// A pending deposit request (bolt11 invoice + quote id).
#[derive(Debug, Clone)]
pub struct DepositRequest {
    /// The bolt11 Lightning invoice to pay.
    pub bolt11: String,
    /// Mint quote id — pass to [`Nip60WalletHandle::complete_deposit`] once paid.
    pub quote_id: String,
    /// Mint URL this deposit is for.
    pub mint_url: String,
    /// Requested amount in sats.
    pub amount_sats: u64,
}
