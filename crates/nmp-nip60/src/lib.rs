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
//! its `ActorCommand::Publish*` chokepoint. For the kind:17375 legacy relay
//! hint and why it can never become relay-selection truth, see the
//! [`wallet_event`] module docs.
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
//! ).expect("wallet creation");
//!
//! let to_publish = wallet.take_outbox();
//! println!("balance: {} sat", wallet.balance_sats());
//! println!("{} event(s) queued for the kernel to publish", to_publish.len());
//! ```
//!
//! For depositing funds (mint tokens from a paid Lightning invoice) and
//! sending/receiving NutZaps — all `native`-feature-only, since they round-
//! trip to the Cashu mint over HTTP — see [`nip60_wallet::deposit`] and the
//! [`Nip60WalletHandle::send_nutzap`](nip60_wallet::Nip60WalletHandle) /
//! [`Nip60WalletHandle::redeem_nutzap`](nip60_wallet::Nip60WalletHandle) docs.

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
#[cfg(feature = "native")]
pub use nip60_wallet::DepositRequest;
pub use nip60_wallet::Nip60WalletHandle;
pub use nutzap::{
    build_nutzap_event, build_nutzap_info_event, decode_nutzap_event, decode_nutzap_info_event,
    p2pk_secret, NutZapInfo, NutZapProof, ReceivedNutZap,
};
#[cfg(feature = "native")]
pub use nutzap::verify_nutzap_dleq;
pub use token_event::{build_token_event, decode_token_event, TokenRecord};
pub use wallet_event::{build_wallet_event, decode_wallet_event, WalletConfig};
