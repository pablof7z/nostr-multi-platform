//! `nmp-nip60` — NIP-60 Cashu wallet + NIP-61 NutZap mechanics for NMP apps.
//!
//! Owns reusable NIP-60/NIP-61 protocol mechanics only: event codecs, Cashu
//! proof/DLEQ/P2PK/rollover types, and pure shape validation. It does not own
//! backend selection, a unified wallet interface, product policy, or a
//! private operation queue — those belong to the `nmp-wallet` composition
//! crate (see `docs/architecture/nip60-nip61-wallet-design.md`), which
//! consumes this crate as its Cashu backend adapter.
//!
//! # Supported NIPs
//!
//! - **NIP-60** — Cashu wallet events (kind:17375 wallet config, kind:7375
//!   unspent proofs, kind:7376 spending history, kind:7374 deposit quote).
//! - **NIP-61** — NutZap send/receive (kind:9321 nutzap event, kind:10019
//!   NutZap info event).
//! - **NIP-88** — Cashu mint announcement (kind:38172) for mint discovery.
//!
//! # Relay I/O lives in the kernel, not here
//!
//! This crate performs ZERO relay I/O — no sockets, no hardcoded relay URLs.
//! The kernel fetches the wallet's events through its `EventStore` / interest
//! pipeline and feeds them in via `Nip60WalletHandle::ingest_*`; events to
//! publish are queued in the handle's outbox and drained by the kernel through
//! its `ActorCommand::Publish*` chokepoint. The legacy `relay` tags on the
//! kind:17375 config are surfaced only as a non-authoritative compatibility
//! hint (`Nip60WalletHandle::legacy_relay_hint`) — they must never be treated
//! as the relay-selection source of truth. The authoritative relay set is the
//! active user's kind:10019 `relay` tags, with NIP-65 fallback; that
//! resolution policy is owned by `nmp-wallet`, not this crate.
//!
//! # Cashu cryptography
//!
//! All blind signature math (DHKE, NUT-00) and DLEQ proof verification
//! (NUT-12) are implemented in [`cashu::crypto`] using secp256k1 via the
//! `nostr` crate's re-exported crypto primitives.  No external CDK dependency
//! is required.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use nmp_nip60::Nip60WalletHandle;
//! use nostr::Keys;
//!
//! let keys = Keys::generate();
//! // Create a new wallet backed by the testnut mint.
//! let wallet = Nip60WalletHandle::create_new(
//!     &keys,
//!     "https://testnut.cashu.space",
//!     Vec::new(),
//! ).expect("wallet creation");
//!
//! // Initiate a deposit.
//! let deposit = wallet.initiate_deposit(64).expect("initiate deposit");
//! println!("Pay this invoice: {}", deposit.bolt11);
//! // (testnut auto-pays — skip actual payment)
//! let minted = wallet.complete_deposit(&deposit).expect("complete deposit");
//! println!("Minted {minted} sat");
//! ```

pub mod cashu;
pub mod error;
pub mod history_event;
pub mod kinds;
pub mod mint_announce;
pub mod nip60_wallet;
pub mod nutzap;
pub mod ownership;
pub mod token_event;
pub mod wallet_event;

pub use error::Nip60Error;
pub use kinds::*;
pub use mint_announce::{
    decode_mint_announce_event, mint_announce_filter, MintAnnouncement,
};
pub use nip60_wallet::{DepositRequest, Nip60WalletHandle};
pub use nutzap::{
    build_nutzap_event, build_nutzap_info_event, decode_nutzap_event, decode_nutzap_info_event,
    p2pk_secret, verify_nutzap_dleq, NutZapInfo, NutZapProof, ReceivedNutZap,
};
pub use token_event::{build_token_event, decode_token_event, TokenRecord};
pub use wallet_event::{build_wallet_event, decode_wallet_event, WalletConfig};
