//! `PaymentPort` — substrate seam for paying a Lightning (BOLT-11) invoice.
//!
//! NIP-57 (zaps) needs to *pay* an invoice once the LNURL-pay round-trip has
//! produced a BOLT-11, but it must not know *how* the payment happens. The
//! concrete payment mechanism in this workspace is NIP-47 Nostr Wallet Connect
//! (`nmp-nip47`), but that is a runtime-policy detail: a different app could
//! pay through a custom wallet, an on-device LN node, or a test stub.
//!
//! Before this seam existed, `nmp-nip57` captured the concrete
//! `nmp_nip47::WalletRuntimeHandle` and built a concrete
//! `nmp_nip47::WalletPayInvoiceCommand` itself — a Layer-4 → Layer-4
//! sibling dependency (`nmp-nip57 → nmp-nip47`) that hard-wired one wallet
//! runtime into the zap chain. This trait inverts that edge: NIP-57 emits a
//! typed [`PaymentIntent`] through an injected `Arc<dyn PaymentPort>` and never
//! names a wallet runtime. NIP-47 supplies the implementation, and the
//! composition root (`nmp-defaults`) wires the two together.
//!
//! The port returns an [`ActorCommand`] rather than performing I/O directly:
//! payment is an actor-thread effect (it enqueues a `ProtocolCommand`), so the
//! caller (the off-actor LNURL worker) sends the returned command back into the
//! actor loop exactly as it would any other follow-up command. This keeps the
//! port pure data-in / command-out and replay-safe (AGENTS.md effects rule).

use crate::ActorCommand;

/// A request to pay a single BOLT-11 invoice.
///
/// Produced by NIP-57 after the LNURL-pay round-trip mints the invoice. Carries
/// only protocol-neutral fields — no wallet runtime, no NIP-47 nouns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentIntent {
    /// The BOLT-11 invoice string to pay.
    pub bolt11: String,
    /// Optional amount override in millisatoshis. `None` means the amount is
    /// carried by the invoice itself (the common zap case).
    pub amount_msats: Option<u64>,
    /// Registry-minted correlation id when the payment originates from a
    /// `dispatch_action` flow, so terminal stages close the originating
    /// action's spinner. `None` for actor-internal payments with no spinner.
    pub correlation_id: Option<String>,
}

/// Substrate seam: turn a [`PaymentIntent`] into the [`ActorCommand`] that pays
/// it through the app's configured wallet.
///
/// Implementations are injected at composition time and held by NIP-57 as
/// `Option<Arc<dyn PaymentPort>>` (`None` = no wallet wired, which NIP-57
/// surfaces as a "no wallet connected" failure). The bound includes
/// [`std::fmt::Debug`] so commands embedding an `Arc<dyn PaymentPort>` (e.g.
/// NIP-57's `FetchLnurlInvoiceCommand`) keep their derived `Debug`.
pub trait PaymentPort: Send + Sync + std::fmt::Debug {
    /// Produce the [`ActorCommand`] that pays `intent` through this wallet.
    /// The caller sends the returned command into the actor loop.
    fn pay_invoice(&self, intent: PaymentIntent) -> ActorCommand;
}
