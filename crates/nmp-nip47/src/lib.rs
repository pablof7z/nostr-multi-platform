//! NIP-47 Nostr Wallet Connect — Layer-4 NIP crate.
//!
//! Owns the actor-side `WalletRuntime` (NWC connection, pending payments,
//! kind:23195 response decoder), the `nmp.wallet.pay_invoice` `ActionModule`,
//! and the three [`ProtocolCommand`](nmp_core::substrate::ProtocolCommand)
//! impls that replace the pre-V-38 `ActorCommand::Wallet{Connect,Disconnect,
//! PayInvoice}` variants.
//!
//! After V-38 lands `nmp-core` no longer depends on `nmp-nwc`; that edge
//! moves here (`nmp-nip47 → nmp-nwc`, `nmp-nip47 → nmp-core`).
//!
//! See `docs/architecture/crate-boundaries.md` §2 (per-crate table row
//! `nmp-nip47`) and §5 step 7 for the canonical responsibility statement +
//! migration brief.
//!
//! # Composition
//!
//! Host code (an `nmp-app-*` crate, or the FFI surface in `nmp-core::ffi::
//! wallet`) constructs the [`WalletStatusSlot`] and a
//! [`WalletRuntimeHandle`] (`Arc<Mutex<Option<WalletRuntime>>>`), then:
//!
//! * registers [`WalletPayInvoiceModule`] on its `ActionRegistry`;
//! * installs the runtime handle into the actor through
//!   [`nmp_core::NmpApp::set_wallet_runtime_handle`];
//! * captures one `Arc` clone of [`WalletStatusSlot`] in the `"wallet"`
//!   snapshot-projection closure registered via
//!   [`nmp_core::NmpApp::register_snapshot_projection`].
//!
//! D0: the kernel never names "wallet" / "NWC" / "kind:23194" — those nouns
//! live entirely here.

pub mod action;
mod crypto;
pub mod protocol;
pub mod runtime;
pub mod status;

pub use action::{
    WalletAction, WalletConnectAction, WalletConnectModule, WalletDisconnectAction,
    WalletDisconnectModule, WalletPayInvoiceModule,
};
pub use protocol::{
    dispatch_nwc_relay_text, WalletConnectCommand, WalletDisconnectCommand,
    WalletPayInvoiceCommand,
};
pub use runtime::{
    active_wallet_runtime, handle_nwc_text, install_wallet_runtime, new_wallet_runtime_handle,
    wallet_connect, wallet_disconnect, wallet_pay_invoice, WalletRuntime, WalletRuntimeHandle,
};
pub use status::{format_sats_display, new_wallet_status_slot, WalletStatus, WalletStatusSlot};
