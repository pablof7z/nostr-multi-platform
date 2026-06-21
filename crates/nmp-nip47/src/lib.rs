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
//! The single entry point is [`register_wallet`], which performs all wiring
//! during the app's config phase (before the kernel starts). It:
//!
//! * constructs a [`WalletRuntimeHandle`] (`Arc<Mutex<Option<WalletRuntime>>>`)
//!   and seeds it with a fresh [`WalletRuntime`];
//! * registers the three wallet `ActionModule` values
//!   ([`WalletConnectModule`] / [`WalletDisconnectModule`] /
//!   [`WalletPayInvoiceModule`]) — each owns an `Arc` clone of the handle
//!   (ADR-0052 rung 5.2: register-by-value, no process-global);
//! * installs a `WalletInterceptor` (implementing the substrate-generic
//!   `RelayTextInterceptor` trait) via `app.add_relay_text_interceptor` — the
//!   interceptor holds its own `Arc` clone of the handle and drives inbound
//!   kind:23195 decoding plus the V-79 heartbeat / TTL sweeps on every actor
//!   idle tick;
//! * registers the generic `"wallet"` snapshot-projection closure and the
//!   typed `"wallet"` sidecar via [`wallet_typed_projection`].
//!
//! Two `NmpApp` instances in one process therefore drive fully independent
//! wallet runtimes — the deleted `ACTIVE_WALLET_RUNTIME` process-global is
//! gone (ADR-0052 rung 5.2).
//!
//! D0: the kernel never names "wallet" / "NWC" / "kind:23194" — those nouns
//! live entirely here.

pub mod action;
mod crypto;
pub mod payment_port;
pub mod payment_store;
pub mod protocol;
mod reconcile;
pub mod register;
pub mod runtime;
pub mod status;
pub mod ui_codes;
pub mod wire;

pub use register::{register_wallet, wallet_typed_projection};

pub use action::{
    WalletAction, WalletConnectAction, WalletConnectModule, WalletDisconnectAction,
    WalletDisconnectModule, WalletPayInvoiceModule,
    INFLIGHT_BOLT11_TTL,
};
pub use payment_port::{wallet_payment_port, WalletPaymentPort};
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
    new_wallet_status_slot, NwcConnectionState, WalletStatus, WalletStatusSlot,
};
pub use wire::typed_fb::{
    decode_wallet_status, encode_wallet_status, FILE_IDENTIFIER as WALLET_STATUS_FILE_IDENTIFIER,
    SCHEMA_ID as WALLET_STATUS_SCHEMA_ID, SCHEMA_VERSION as WALLET_STATUS_SCHEMA_VERSION,
};
