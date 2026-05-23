//! NIP-47 wallet `ActionModule` surface (gated on `feature = "wallet"`).
//!
//! Parallel to `crates/nmp-core/src/publish/` — this is the *intent* side of
//! the wallet capability (the `ActionModule` trait impls dispatched through
//! `nmp_app_dispatch_action`). The *runtime* side (the actor-thread state
//! machine that owns the NWC connection, the `pending_payments` map, and the
//! kind:23195 response handler) lives in `crates/nmp-core/src/actor/commands/wallet.rs`.
//!
//! # Why both sides exist
//!
//! `ActionModule::execute` is a static method called on the FFI dispatch
//! thread — it must only enqueue an `ActorCommand` (D8: no blocking on the
//! FFI thread, D4: the actor is the sole writer). The split keeps the
//! action-seam validator (`start`) and the executor (`execute`) in this
//! module, and the actor-thread engine code (which holds the NWC connection
//! and processes responses) on the actor side.
//!
//! See [`action`] module docs for the V3 rationale (closing the
//! `WalletPayInvoice` bypass of `dispatch_action`) and the Theme A
//! discriminator that scopes `nmp.wallet.*` to user-initiated intents
//! (`pay_invoice`) while leaving connection lifecycle (`wallet_connect` /
//! `wallet_disconnect`) on dedicated FFI symbols.

mod action;

pub use action::{WalletAction, WalletPayInvoiceModule};
