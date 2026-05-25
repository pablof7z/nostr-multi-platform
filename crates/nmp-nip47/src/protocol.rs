//! NIP-47 [`ProtocolCommand`] impls — the substrate-generic replacement for
//! the pre-V-38 `ActorCommand::Wallet{Connect,Disconnect,PayInvoice}`
//! variants.
//!
//! Each command captures its arguments + a clone of the
//! [`WalletRuntimeHandle`] (the shared slot the actor installed at host
//! init). The `run` body locks the slot, calls the runtime helper, and
//! pushes outbound frames through
//! [`ProtocolCommandContext::push_outbound`].
//!
//! D6: a missing runtime (slot never installed) sets a `last_error_toast`
//! and returns `Ok(())` — never panics.

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};

use crate::runtime::{
    handle_nwc_text as runtime_handle_nwc_text, wallet_connect as runtime_wallet_connect,
    wallet_disconnect as runtime_wallet_disconnect, wallet_pay_invoice as runtime_wallet_pay_invoice,
    WalletRuntimeHandle,
};

/// V-38 replacement for `ActorCommand::WalletConnect`.
#[derive(Debug)]
pub struct WalletConnectCommand {
    pub uri: String,
    pub runtime: WalletRuntimeHandle,
}

/// V-38 replacement for `ActorCommand::WalletDisconnect`.
#[derive(Debug)]
pub struct WalletDisconnectCommand {
    pub runtime: WalletRuntimeHandle,
}

/// V-38 replacement for `ActorCommand::WalletPayInvoice`.
#[derive(Debug)]
pub struct WalletPayInvoiceCommand {
    pub bolt11: String,
    pub amount_msats: Option<u64>,
    /// Registry-minted action id when this command originates from
    /// `nmp_app_dispatch_action` under `nmp.wallet.pay_invoice`.
    /// `None` is reserved for actor-internal auto-dispatched payments
    /// (e.g. the LNURL → pay_invoice chain).
    pub correlation_id: Option<String>,
    pub runtime: WalletRuntimeHandle,
}

fn with_runtime_and_kernel<F: FnOnce(&mut crate::runtime::WalletRuntime, &mut nmp_core::Kernel) -> Vec<nmp_core::OutboundMessage>>(
    handle: &WalletRuntimeHandle,
    ctx: &mut ProtocolCommandContext<'_>,
    op_label: &'static str,
    f: F,
) -> Result<(), ProtocolCommandError> {
    let Some(kernel) = ctx.kernel_mut() else {
        return Err(ProtocolCommandError::new(format!(
            "{op_label}: no kernel handle attached"
        )));
    };
    let mut guard = handle.lock().map_err(|_| {
        ProtocolCommandError::new(format!("{op_label}: wallet runtime mutex poisoned"))
    })?;
    let Some(runtime) = guard.as_mut() else {
        kernel.set_last_error_toast(Some(format!(
            "{op_label}: wallet runtime not installed"
        )));
        return Ok(());
    };
    let outbound = f(runtime, kernel);
    drop(guard);
    ctx.push_outbound(outbound);
    Ok(())
}

impl ProtocolCommand for WalletConnectCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self { uri, runtime } = *self;
        with_runtime_and_kernel(&runtime, ctx, "wallet_connect", |rt, k| {
            runtime_wallet_connect(rt, k, &uri)
        })
    }
}

impl ProtocolCommand for WalletDisconnectCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self { runtime } = *self;
        with_runtime_and_kernel(&runtime, ctx, "wallet_disconnect", |rt, k| {
            runtime_wallet_disconnect(rt, k)
        })
    }
}

impl ProtocolCommand for WalletPayInvoiceCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            bolt11,
            amount_msats,
            correlation_id,
            runtime,
        } = *self;
        with_runtime_and_kernel(&runtime, ctx, "wallet_pay_invoice", |rt, k| {
            runtime_wallet_pay_invoice(rt, k, &bolt11, amount_msats, correlation_id.clone())
        })
    }
}

/// Convenience wrapper the actor uses on every relay text frame to give
/// the wallet runtime a chance to decode kind:23195 responses. Returns the
/// outbound frames the runtime wants to enqueue (typically empty — the
/// response side is read-only against the kernel state).
///
/// `None` runtime (slot not installed) is a silent no-op (D6).
#[must_use]
pub fn dispatch_nwc_relay_text(
    runtime: &WalletRuntimeHandle,
    kernel: &mut nmp_core::Kernel,
    relay_url: &str,
    text: &str,
) -> Vec<nmp_core::OutboundMessage> {
    let Ok(mut guard) = runtime.lock() else {
        return Vec::new();
    };
    let Some(rt) = guard.as_mut() else {
        return Vec::new();
    };
    if !rt.is_nwc_relay(relay_url) {
        return Vec::new();
    }
    runtime_handle_nwc_text(rt, text, kernel)
}
