//! [`ZapPayer`] — the payment-backend enum captured by value at zap registration
//! time (ADR-0052 D1/D2). Extracted from `action.rs` to keep the parent module
//! under the 500-LOC ceiling.

/// The payment backend a zap's fetched bolt11 is handed to (ADR-0052 D1/D2).
///
/// The zap chain (`ZapAction` → `FetchLnurlInvoiceCommand`'s off-actor worker)
/// must reach the wallet runtime WITHOUT a process-global. So the payer is
/// captured by value at registration time and carried command-to-worker:
///
/// * `Unavailable` — no wallet wired (the generic `nmp-defaults` zap default,
///   which has zero wallet knowledge): a fetched invoice records a `Failed`
///   terminal with "no wallet connected".
/// * `Nwc(handle)` — a NIP-47 wallet was composed; the app registers
///   `ZapAction::new(ZapPayer::Nwc(handle.clone()))`, overriding the default,
///   so the worker dispatches `WalletPayInvoiceCommand` against THIS app's
///   own runtime handle.
///
/// Cloned into each `FetchLnurlInvoiceCommand` at `execute` time; the worker
/// thread owns its own clone. No global, instance-scoped per `NmpApp`.
#[derive(Clone, Default)]
pub enum ZapPayer {
    /// No wallet backend wired — a fetched invoice fails closed.
    #[default]
    Unavailable,
    /// NIP-47 NWC wallet, sourced from the app's own runtime handle.
    #[cfg(feature = "native")]
    Nwc(nmp_nip47::WalletRuntimeHandle),
}

impl std::fmt::Debug for ZapPayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZapPayer::Unavailable => f.write_str("ZapPayer::Unavailable"),
            #[cfg(feature = "native")]
            ZapPayer::Nwc(_) => f.write_str("ZapPayer::Nwc(<handle>)"),
        }
    }
}
