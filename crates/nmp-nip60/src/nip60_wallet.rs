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
//! # Module layout
//!
//! This file owns the wallet's shared state (struct, construction, outbox,
//! balance, ingest) and the [`WalletBackend`] trait adapter. Each concern
//! that operates on that shared state lives in its own submodule:
//!
//! - [`deposit`] — minting tokens from a paid Lightning invoice (NUT-04/23).
//! - [`nutzap_send`] — proof selection, P2PK-locking, and sending NutZaps.
//! - [`nutzap_receive`] — redeeming a received NutZap.
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

mod deposit;
mod nutzap_receive;
mod nutzap_send;

pub use deposit::DepositRequest;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use nostr::{Event, EventId, Keys, PublicKey};

use crate::backend::{PayResult, WalletBackend, WalletError};
use crate::error::Nip60Error;
use crate::history_event::redeemed_nutzap_ids;
use crate::nutzap::NutZapProof;
use crate::token_event::{decode_token_event, TokenRecord};
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

    // ── Accessors ──────────────────────────────────────────────────────────

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
