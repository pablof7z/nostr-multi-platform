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
//! Host code (an `nmp-app-*` crate) constructs the [`WalletStatusSlot`] and a
//! [`WalletRuntimeHandle`] (`Arc<Mutex<Option<WalletRuntime>>>`) ONCE, then:
//!
//! * registers [`WalletConnectModule`] / [`WalletDisconnectModule`] /
//!   [`WalletPayInvoiceModule`] on its `ActionRegistry`, each constructed via
//!   `Module::new(Arc::clone(&handle))` so the module owns its handle by value
//!   (ADR-0052 D1/D2 — no process-global);
//! * captures the SAME `Arc::clone(&handle)` in the relay-text interceptor;
//! * captures one `Arc` clone of [`WalletStatusSlot`] in the `"wallet"`
//!   snapshot-projection closure registered via
//!   [`nmp_core::NmpApp::register_snapshot_projection`].
//!
//! Each `NmpApp` instance owns its own handle, so two apps in one process have
//! two independent wallet runtimes (the K2 rung 5.2 no-crosstalk invariant).
//!
//! D0: the kernel never names "wallet" / "NWC" / "kind:23194" — those nouns
//! live entirely here.

pub mod action;
mod crypto;
pub mod payment_store;
pub mod protocol;
mod reconcile;
pub mod runtime;
pub mod status;
pub mod wire;

pub use action::{
    WalletAction, WalletConnectAction, WalletConnectModule, WalletDisconnectAction,
    WalletDisconnectModule, WalletPayInvoiceModule,
};
pub use payment_store::{FsPaymentStore, PaymentRecord, PaymentState, PaymentStoreError};
pub use protocol::{
    dispatch_nwc_relay_text, WalletConnectCommand, WalletDisconnectCommand,
    WalletPayInvoiceCommand,
};
pub use runtime::{
    new_wallet_runtime_handle, HeartbeatOutbound, WalletRuntime, WalletRuntimeHandle,
    HEARTBEAT_CADENCE_SECS, HEARTBEAT_MAX_FAILURES, HEARTBEAT_PROBE_TIMEOUT_SECS,
    PENDING_PAYMENT_TTL_SECS,
};
pub use status::{
    format_sats_display, new_wallet_status_slot, NwcConnectionState, WalletStatus, WalletStatusSlot,
};
pub use wire::typed_fb::{
    decode_wallet_status, encode_wallet_status, FILE_IDENTIFIER as WALLET_STATUS_FILE_IDENTIFIER,
    SCHEMA_ID as WALLET_STATUS_SCHEMA_ID, SCHEMA_VERSION as WALLET_STATUS_SCHEMA_VERSION,
};
