//! NIP-47 implementation of the substrate [`PaymentPort`] seam.
//!
//! NIP-57 (zaps) emits a typed [`PaymentIntent`] through an injected
//! `Arc<dyn PaymentPort>`; this module supplies the concrete NIP-47 adapter
//! that turns that intent into a [`WalletPayInvoiceCommand`] paid through the
//! per-app [`WalletRuntimeHandle`]. The dependency direction is
//! `nmp-nip47 → nmp-core` (the trait owner) — NIP-57 no longer depends on
//! `nmp-nip47`. Composition (`explicit composition` / app roots) wires the adapter
//! into the zap chain via `nmp_nip57::Config::with_payment_port`.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{PaymentIntent, PaymentPort};

use crate::protocol::WalletPayInvoiceCommand;
use crate::runtime::WalletRuntimeHandle;

/// [`PaymentPort`] backed by a per-app NIP-47 [`WalletRuntimeHandle`].
///
/// Holds the same handle the wallet `ActionModule`s mutate (ADR-0072 rung 5.2
/// — per-app, not a process-global), so a zap pays through the same wallet
/// runtime the app installed.
pub struct WalletPaymentPort {
    runtime: WalletRuntimeHandle,
}

impl WalletPaymentPort {
    /// Wrap a per-app wallet runtime handle as a payment port.
    #[must_use]
    pub fn new(runtime: WalletRuntimeHandle) -> Self {
        Self { runtime }
    }
}

// The handle is `Arc<Mutex<Option<WalletRuntime>>>`; its inner runtime is not
// `Debug`, and the `PaymentPort` bound only needs an opaque tag, so format a
// type name rather than the runtime contents.
impl std::fmt::Debug for WalletPaymentPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WalletPaymentPort")
    }
}

impl PaymentPort for WalletPaymentPort {
    fn pay_invoice(&self, intent: PaymentIntent) -> ActorCommand {
        ActorCommand::Protocol(Box::new(WalletPayInvoiceCommand {
            bolt11: intent.bolt11,
            amount_msats: intent.amount_msats,
            correlation_id: intent.correlation_id,
            runtime: self.runtime.clone(),
        }))
    }
}

/// Convenience: build an `Arc<dyn PaymentPort>` from a per-app wallet runtime
/// handle, ready to inject into NIP-57's zap chain.
#[must_use]
pub fn wallet_payment_port(runtime: WalletRuntimeHandle) -> Arc<dyn PaymentPort> {
    Arc::new(WalletPaymentPort::new(runtime))
}
