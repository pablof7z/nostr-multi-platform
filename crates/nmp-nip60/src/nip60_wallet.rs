//! High-level NIP-60 wallet — ties together event encoding, Cashu mint HTTP
//! client, relay I/O, and the `WalletBackend` trait implementation.
//!
//! # Typical usage
//!
//! ```rust,no_run
//! use nmp_nip60::Nip60WalletHandle;
//! use nostr::Keys;
//!
//! let keys = Keys::generate();
//! let mut wallet = Nip60WalletHandle::create_new(
//!     &keys,
//!     "https://testnut.cashu.space",
//!     vec!["wss://relay.damus.io".into()],
//! ).expect("wallet creation");
//!
//! println!("balance: {} sat", wallet.balance_sats());
//! ```

use std::sync::{Arc, Mutex};

use nostr::{EventId, Filter, Keys, Kind, PublicKey};
use tracing::{debug, info, warn};

use crate::backend::{PayResult, WalletBackend, WalletError};
use crate::cashu::client::{split_amount, MintClient, self as cashu_client};
use crate::cashu::types::Proof;
use crate::error::Nip60Error;
use crate::history_event::{build_history_event, HistoryRecord};
use crate::kinds::{KIND_NUTZAP_INFO, KIND_TOKEN, KIND_WALLET};
use crate::nutzap::{
    build_nutzap_info_event, p2pk_secret, NutZapInfo, NutZapProof,
};
use crate::relay::{fetch_events, fetch_nip65_relays, publish_event};
use crate::token_event::{build_token_event, decode_token_event, TokenRecord};
use crate::wallet_event::{build_wallet_event, decode_wallet_event, WalletConfig};

/// The in-memory NIP-60 wallet handle.
///
/// Thread-safe via interior `Mutex`. All mutating operations lock briefly.
#[derive(Clone)]
pub struct Nip60WalletHandle {
    keys: Keys,
    config: Arc<Mutex<WalletConfig>>,
    tokens: Arc<Mutex<Vec<TokenRecord>>>,
    relays: Vec<String>,
}

impl Nip60WalletHandle {
    // ── Construction ───────────────────────────────────────────────────────

    /// Create a brand-new NIP-60 wallet, publish the kind:17375 wallet event
    /// and optionally publish a kind:10019 NutZap info event, then return
    /// the wallet handle.
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

        // Publish wallet event to each relay.
        for relay in &relays {
            match publish_event(relay, &wallet_event) {
                Ok(()) => info!("published wallet event to {relay}"),
                Err(e) => warn!("failed to publish wallet event to {relay}: {e}"),
            }
        }

        Ok(Self {
            keys: keys.clone(),
            config: Arc::new(Mutex::new(config)),
            tokens: Arc::new(Mutex::new(Vec::new())),
            relays,
        })
    }

    /// Load an existing NIP-60 wallet from the given relays.
    ///
    /// Fetches the most recent kind:17375 event, decrypts it, and loads all
    /// associated kind:7375 token events.
    pub fn load_from_relays(
        keys: &Keys,
        relays: &[String],
    ) -> Result<Self, Nip60Error> {
        let filter = Filter::new()
            .kind(Kind::from(KIND_WALLET))
            .author(keys.public_key())
            .limit(1);

        let mut wallet_events = Vec::new();
        for relay in relays {
            match fetch_events(relay, filter.clone()) {
                Ok(evts) => wallet_events.extend(evts),
                Err(e) => warn!("wallet fetch from {relay}: {e}"),
            }
        }

        let wallet_event = wallet_events
            .into_iter()
            .max_by_key(|e| e.created_at)
            .ok_or(Nip60Error::NotInitialised)?;
        let config = decode_wallet_event(&wallet_event, keys.secret_key(), &keys.public_key())?;
        let effective_relays = if config.relays.is_empty() {
            relays.to_vec()
        } else {
            config.relays.clone()
        };

        let handle = Self {
            keys: keys.clone(),
            config: Arc::new(Mutex::new(config)),
            tokens: Arc::new(Mutex::new(Vec::new())),
            relays: effective_relays.clone(),
        };
        handle.refresh_tokens()?;
        Ok(handle)
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

    // ── Token management ───────────────────────────────────────────────────

    /// Reload all token events from the wallet's relays.
    pub fn refresh_tokens(&self) -> Result<(), Nip60Error> {
        let filter = Filter::new()
            .kind(Kind::from(KIND_TOKEN))
            .author(self.keys.public_key());

        let mut all_events = Vec::new();
        for relay in &self.relays {
            match fetch_events(relay, filter.clone()) {
                Ok(evts) => all_events.extend(evts),
                Err(e) => warn!("token fetch from {relay}: {e}"),
            }
        }
        // Deduplicate by event id.
        all_events.sort_by_key(|e| e.id);
        all_events.dedup_by_key(|e| e.id);

        let mut records = Vec::new();
        for evt in &all_events {
            match decode_token_event(evt, self.keys.secret_key(), &self.keys.public_key()) {
                Ok(r) => records.push(r),
                Err(e) => warn!("skip malformed token event {}: {e}", evt.id),
            }
        }
        *self.tokens.lock().unwrap() = records;
        Ok(())
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
    /// Returns `Err(Nip60Error::QuoteNotPaid)` if the invoice has not been
    /// paid yet — the caller is responsible for waiting and retrying (sleeping
    /// in library code violates D8). For testnut, invoices are auto-paid
    /// within milliseconds; a single short wait in the caller is sufficient.
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

        for relay in &self.relays {
            match publish_event(relay, &event) {
                Ok(()) => debug!("published token event to {relay}"),
                Err(e) => warn!("token event to {relay}: {e}"),
            }
        }

        // Update in-memory state.
        let mut record_with_id = record;
        record_with_id.event_id = Some(event.id);
        self.tokens.lock().unwrap().push(record_with_id);

        // Publish history event (direction: in).
        let mut history = HistoryRecord::new_in(total);
        history.created.push(event.id);
        if let Ok(h_builder) = build_history_event(&history, &self.keys) {
            if let Ok(h_event) = h_builder.sign_with_keys(&self.keys) {
                for relay in &self.relays {
                    let _ = publish_event(relay, &h_event);
                }
            }
        }

        Ok(total)
    }

    // ── NutZap send ────────────────────────────────────────────────────────

    /// Send a NutZap to a recipient.
    ///
    /// 1. Looks up the recipient's kind:10019 NutZap info from their relays.
    /// 2. Swaps proofs for P2PK-locked proofs at the mint.
    /// 3. Publishes kind:9321 to the recipient's nutzap relays.
    ///
    /// Returns the published nutzap event id.
    pub fn send_nutzap(
        &self,
        amount_sats: u64,
        recipient_pubkey: &PublicKey,
        recipient_relays: &[String],
        comment: Option<&str>,
        zapped_event_id: Option<&EventId>,
    ) -> Result<EventId, Nip60Error> {
        // Look up recipient's NutZap info.
        let nutzap_info = self.fetch_nutzap_info(recipient_pubkey, recipient_relays)?;
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

        // Build and publish nutzap event.
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

        // Publish to recipient's nutzap relays (plus our own).
        let mut publish_relays: Vec<String> = nutzap_info.relays.clone();
        for r in &self.relays {
            if !publish_relays.contains(r) {
                publish_relays.push(r.clone());
            }
        }
        for relay in &publish_relays {
            match publish_event(relay, &nutzap_event) {
                Ok(()) => info!("published nutzap to {relay}"),
                Err(e) => warn!("nutzap to {relay}: {e}"),
            }
        }

        info!(
            "sent nutzap: {} sat to {} ({} relays)",
            amount_sats,
            recipient_pubkey.to_hex(),
            publish_relays.len()
        );
        Ok(nutzap_event.id)
    }

    // ── NutZap info (kind:10019) ───────────────────────────────────────────

    /// Publish the user's kind:10019 NutZap info event.
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
        for relay in &self.relays {
            match publish_event(relay, &event) {
                Ok(()) => info!("published nutzap info to {relay}"),
                Err(e) => warn!("nutzap info to {relay}: {e}"),
            }
        }
        Ok(event.id)
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

    fn fetch_nutzap_info(
        &self,
        recipient_pubkey: &PublicKey,
        hint_relays: &[String],
    ) -> Result<NutZapInfo, Nip60Error> {
        let nutzap_filter = Filter::new()
            .kind(Kind::from(KIND_NUTZAP_INFO))
            .author(*recipient_pubkey)
            .limit(1);

        // 1. Discover the recipient's relay list via purplepag.es (NIP-65 indexer).
        //    This is the correct approach: don't assume the recipient uses our relays.
        let nip65_relays = fetch_nip65_relays(recipient_pubkey);

        // 2. Build the search order: NIP-65 relays first, then any caller-supplied
        //    hints, then our own relays as a last resort.
        let mut search_relays: Vec<String> = nip65_relays;
        for r in hint_relays {
            if !search_relays.contains(r) {
                search_relays.push(r.clone());
            }
        }
        for r in &self.relays {
            if !search_relays.contains(r) {
                search_relays.push(r.clone());
            }
        }

        for relay in &search_relays {
            match fetch_events(relay, nutzap_filter.clone()) {
                Ok(events) => {
                    if let Some(evt) = events.into_iter().max_by_key(|e| e.created_at) {
                        return Ok(crate::nutzap::decode_nutzap_info_event(&evt));
                    }
                }
                Err(e) => warn!("fetch nutzap info from {relay}: {e}"),
            }
        }
        Err(Nip60Error::MintDiscovery(format!(
            "no kind:10019 found for {}",
            recipient_pubkey.to_hex()
        )))
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

    /// Update token events after spending: delete spent events, save change.
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

        for record in tokens.drain(..) {
            let remaining: Vec<Proof> = record
                .proofs
                .into_iter()
                .filter(|p| !spent_secrets.contains(p.secret.as_str()))
                .collect();
            if remaining.is_empty() {
                if let Some(id) = record.event_id {
                    destroyed_event_ids.push(id);
                    // Publish deletion event (NIP-09 style — kind:5).
                    if let Ok(del_builder) = self.build_delete_event(id) {
                        if let Ok(del_event) = del_builder.sign_with_keys(&self.keys) {
                            for relay in &self.relays {
                                let _ = publish_event(relay, &del_event);
                            }
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

        // Publish change token event.
        let _new_event_id = if !change_proofs.is_empty() {
            let change_record = TokenRecord::new(mint_url.to_string(), change_proofs.clone());
            let builder = build_token_event(&change_record, &self.keys)?;
            let event = builder
                .sign_with_keys(&self.keys)
                .map_err(|e| Nip60Error::Event(format!("sign change token: {e}")))?;
            for relay in &self.relays {
                let _ = publish_event(relay, &event);
            }
            let id = event.id;
            new_tokens.push(TokenRecord {
                mint_url: mint_url.to_string(),
                proofs: change_proofs,
                del: destroyed_event_ids.iter().map(|id| id.to_hex()).collect(),
                event_id: Some(id),
            });
            Some(id)
        } else {
            None
        };

        *tokens = new_tokens;
        Ok(())
    }

    fn build_delete_event(&self, event_id: EventId) -> Result<nostr::EventBuilder, Nip60Error> {
        use nostr::{EventBuilder, Kind, Tag};
        Ok(EventBuilder::new(Kind::EventDeletion, "spent")
            .tag(Tag::event(event_id)))
    }

    // ── NutZap receive ─────────────────────────────────────────────────────

    /// Redeem a received nutzap: swap the P2PK proofs for fresh proofs and
    /// publish a kind:7376 history event marking it as redeemed.
    pub fn redeem_nutzap(
        &self,
        nutzap: &crate::nutzap::ReceivedNutZap,
    ) -> Result<u64, Nip60Error> {
        

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
        for relay in &self.relays {
            let _ = publish_event(relay, &event);
        }
        let new_event_id = event.id;

        // Update in-memory state.
        let mut record_with_id = record;
        record_with_id.event_id = Some(new_event_id);
        self.tokens.lock().unwrap().push(record_with_id);

        // Publish history event.
        let mut history = HistoryRecord::new_in(total);
        history.created.push(new_event_id);
        history.redeemed.push(nutzap.event_id);
        if let Ok(h_builder) = build_history_event(&history, &self.keys) {
            if let Ok(h_event) = h_builder.sign_with_keys(&self.keys) {
                for relay in &self.relays {
                    let _ = publish_event(relay, &h_event);
                }
            }
        }

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
