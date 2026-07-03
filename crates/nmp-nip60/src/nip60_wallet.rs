//! High-level NIP-60 wallet — ties together event encoding, the Cashu mint
//! HTTP client, and the in-memory wallet state that a backend adapter wraps
//! for `nmp-wallet`.
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
//! The `legacy_relay_hint` field is wallet *metadata* (the `relay` tags
//! listed in the kind:17375 config), surfaced verbatim for callers that want
//! it. It is **not** authoritative relay selection: the active user's
//! kind:10019 `relay` tags, with NIP-65 fallback, are the source of truth for
//! wallet relay scoping (see `docs/architecture/nip60-nip61-wallet-design.md`,
//! Relay Acquisition). That resolution policy lives in `nmp-wallet`, not
//! here.
//!
//! # Module layout
//!
//! This file owns the wallet's shared state (struct, construction, outbox,
//! balance, ingest). Each concern that operates on that shared state lives in
//! its own submodule:
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

use crate::error::Nip60Error;
use crate::history_event::redeemed_nutzap_ids;
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
    legacy_relay_hint: Vec<String>,
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
            legacy_relay_hint: relays,
            outbox: Arc::new(Mutex::new(Vec::new())),
        };
        handle.enqueue(wallet_event);
        Ok(handle)
    }

    /// Build a wallet handle from a kind:17375 wallet event the kernel already
    /// fetched from its store.
    ///
    /// The legacy relay hint comes from the event's own `relay` tags, if any
    /// — a non-authoritative compatibility signal only (see
    /// [`Self::legacy_relay_hint`]). After construction, feed the wallet's
    /// token and history events through [`Self::ingest_token_events`] and
    /// [`Self::ingest_history_events`] to populate balance and redemption
    /// state.
    pub fn from_wallet_event(keys: &Keys, wallet_event: &Event) -> Result<Self, Nip60Error> {
        let config = decode_wallet_event(wallet_event, keys.secret_key(), &keys.public_key())?;
        let legacy_relay_hint = config.legacy_relay_hint.clone();
        Ok(Self {
            keys: keys.clone(),
            config: Arc::new(Mutex::new(config)),
            tokens: Arc::new(Mutex::new(Vec::new())),
            redeemed: Arc::new(Mutex::new(HashSet::new())),
            legacy_relay_hint,
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

    /// The `relay` tags from the kind:17375 wallet event, if any.
    ///
    /// This is legacy design residue, not authoritative relay selection: the
    /// active user's kind:10019 `relay` tags, with NIP-65 fallback, decide
    /// where wallet-related events are read from and published to. Callers
    /// must not use this as a relay-scoping source of truth — it exists only
    /// as a compatibility hint for wallets whose kind:17375 predates
    /// kind:10019 adoption.
    pub fn legacy_relay_hint(&self) -> &[String] {
        &self.legacy_relay_hint
    }

    pub fn pubkey(&self) -> PublicKey {
        self.keys.public_key()
    }
}

#[cfg(test)]
mod tests;
