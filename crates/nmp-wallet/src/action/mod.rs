//! W5 (#2908, epic #2864) — the canonical `nmp.wallet.*` `ActionModule`s
//! this crate registers: `select_backend`, the Cashu wallet/deposit family,
//! and the nutzap dispatch points. `nmp.wallet.{connect,disconnect,
//! pay_invoice}` stay registered by `nmp-nip47` unchanged this wave — see
//! `crate::register`'s module docs for why.
//!
//! Every module here holds an `Arc<WalletBackendSelector>` (W4) and
//! translates its typed action payload into a `WalletIntent`, dispatched
//! through the selector rather than to a hardcoded backend — this is what
//! makes the Cashu family route to whichever backend advertises the
//! capability, and what makes the nutzap family (no backend implements these
//! yet) a real "dispatch point" that starts working the moment a future
//! backend advertises the capability, with zero change to these modules.

mod cashu;
mod nutzap;
mod select_backend;

pub use cashu::{
    CashuCompleteDepositAction, CashuCompleteDepositModule, CashuCreateAction, CashuCreateModule,
    CashuDepositQuoteAction, CashuDepositQuoteModule, CashuRecoverAction, CashuRecoverModule,
    CashuSetMintsAction, CashuSetMintsModule,
};
pub use nutzap::{
    NutzapPublishInfoAction, NutzapPublishInfoModule, NutzapRedeemAction, NutzapRedeemModule,
    NutzapSendAction, NutzapSendModule,
};
pub use select_backend::{SelectBackendAction, SelectBackendModule};

/// Wall-clock read for `WalletBackendContext::now_secs` at action-dispatch
/// time.
///
/// No clock capability reaches an `ActionModule::execute()` — only
/// `ProtocolCommandContext::now_secs()` (inside a dispatched
/// `ProtocolCommand::run`) has one, and action modules run one hop before
/// that. This value is consumed today only by
/// `CashuWalletBackend::start_intent`'s correlation-id-absent fallback
/// operation-id label, which production dispatch never reaches (the
/// registry always mints a `correlation_id` before `execute()` runs) — so an
/// approximate wall-clock read here carries no correctness weight, only
/// uniqueness-of-a-label weight in a path that is not exercised in
/// production. Mirrors `nmp-nip57::lnurl::roundtrip`'s own direct
/// `SystemTime::now()` read outside the kernel reducer (this is Layer-4
/// composition code, not kernel/reducer code — D9's determinism requirement
/// does not apply here).
pub(crate) fn wall_clock_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Shared dispatch path every Cashu/nutzap `ActionModule::execute()` uses:
/// build the `WalletBackendContext` from the active-account slot, route
/// `intent` through the selector, and forward every resulting `ActorCommand`
/// through `send` (the registry's `execute` callback takes one command per
/// call, not a `Vec`).
pub(crate) fn dispatch_and_forward(
    selector: &crate::selector::WalletBackendSelector,
    active_pubkey: &nmp_core::slots::ActiveAccountSlot,
    intent: crate::backend::WalletIntent,
    correlation_id: &str,
    send: &dyn Fn(nmp_core::actor::ActorCommand),
) {
    let account_pubkey = active_pubkey.lock().ok().and_then(|slot| slot.clone());
    let preferred = selector.preferred();
    let ctx = crate::backend::WalletBackendContext {
        now_secs: wall_clock_now_secs(),
        selected_backend: preferred.as_ref(),
        account_pubkey: account_pubkey.as_deref(),
    };
    for cmd in selector.dispatch(ctx, intent, Some(correlation_id.to_string())) {
        send(cmd);
    }
}

/// Shared `start()` gate every Cashu/nutzap `ActionModule` (except
/// `cashu.recover` — see `action::cashu`'s module docs) uses: reject before
/// dispatch when zero registered backends could ever satisfy `intent`'s
/// capability. Absent capability is a structured rejection, never a silent
/// no-op reached via `execute()`.
pub(crate) fn require_capable_backend(
    selector: &crate::selector::WalletBackendSelector,
    intent: &crate::backend::WalletIntent,
) -> Result<(), nmp_core::substrate::ActionRejection> {
    let Some(capability) = crate::selector::capability_for(intent) else {
        return Ok(());
    };
    if selector.candidates_for(capability).is_empty() {
        return Err(nmp_core::substrate::ActionRejection::InvalidCoded {
            code: crate::ui_codes::NO_CAPABLE_BACKEND,
            message: "no registered wallet backend supports this operation".to_string(),
        });
    }
    Ok(())
}
