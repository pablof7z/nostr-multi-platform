//! `nmp-nip60` — Cashu wallet + NutZap + mint discovery for NMP apps.
//!
//! Provides a [WalletBackend] trait that is completely transparent to apps:
//! they don't care whether the user is running a NIP-60 ecash wallet or a
//! NWC-connected Lightning wallet. Both expose the same interface.
//!
//! # Supported NIPs
//!
//! - **NIP-60** — Cashu wallet events (kind:17375 wallet config, kind:7375
//!   unspent proofs, kind:7376 spending history, kind:7374 deposit quote).
//! - **NIP-61** — NutZap send/receive (kind:9321 nutzap event, kind:10019
//!   NutZap info event).
//! - **NIP-88** — Cashu mint announcement (kind:38172) for mint discovery.
//!
//! # Relay selection (NIP-60 §relay-tags)
//!
//! When loading or subscribing to wallet-related events:
//! - If the wallet's kind:17375 includes `relay` tags → use ONLY those relays.
//! - Otherwise → fall back to the user's NIP-65 relays (caller supplies these).
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
//!     vec!["wss://relay.damus.io".into()],
//! ).expect("wallet creation");
//!
//! // Initiate a deposit.
//! let deposit = wallet.initiate_deposit(64).expect("initiate deposit");
//! println!("Pay this invoice: {}", deposit.bolt11);
//! // (testnut auto-pays — skip actual payment)
//! let minted = wallet.complete_deposit(&deposit).expect("complete deposit");
//! println!("Minted {minted} sat");
//! ```

pub mod backend;
pub mod cashu;
pub mod error;
pub mod history_event;
pub mod kinds;
pub mod mint_announce;
pub mod nip60_wallet;
pub mod nutzap;
pub mod relay;
pub mod token_event;
pub mod wallet_event;

pub use backend::{PayResult, WalletBackend, WalletError};
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
