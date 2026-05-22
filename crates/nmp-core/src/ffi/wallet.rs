//! NIP-47 Nostr Wallet Connect FFI wrappers.
//!
//! All functions are fire-and-forget (D6 — no return values, no exceptions
//! across the FFI boundary). Outcomes surface via subsequent snapshots: the
//! wallet state under `projections["wallet"]` (D0: NIP-47 NWC is an app noun,
//! surfaced through the snapshot-projection seam, not a typed `KernelSnapshot`
//! field) and any error under `last_error_toast`.

use super::{app_ref, c_optional_string_argument, c_string_argument, NmpApp};
use crate::actor::ActorCommand;
use std::ffi::c_char;

/// Connect a NIP-47 wallet using a `nostr+walletconnect://` URI.
///
/// Parses the URI, subscribes for kind:23195 responses on the NWC relay,
/// and sends initial `get_info` + `get_balance` requests.
/// Replaces any existing wallet connection.
#[no_mangle]
pub extern "C" fn nmp_app_wallet_connect(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    app.send_cmd(ActorCommand::WalletConnect { uri });
}

/// Disconnect the current NIP-47 wallet.
///
/// Sends a CLOSE to the NWC relay and clears wallet state. The snapshot's
/// `projections["wallet"].status` will reflect `"disconnected"` on the next
/// emit.
#[no_mangle]
pub extern "C" fn nmp_app_wallet_disconnect(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::WalletDisconnect);
}

/// Pay a Lightning invoice via the connected NIP-47 wallet.
///
/// `bolt11`: BOLT-11 invoice string.
/// `amount_msats_or_null`: pointer to optional payment amount in msats (pass
/// `nil` to use the invoice's embedded amount).
///
/// `correlation_id` is left `None` on this C-ABI path: the iOS shell calls
/// `nmp_app_wallet_pay_invoice` directly (no ActionModule executor exists
/// yet for `nmp.zap`), so the kind:23195 response does not need to drain a
/// dispatched-action promise. A future `ZapAction` executor will construct
/// the same `ActorCommand::WalletPayInvoice` with `Some(correlation_id)` and
/// the wallet runtime's `pending_payments` map closes the round-trip into
/// `action_results` on the matching response.
#[no_mangle]
pub extern "C" fn nmp_app_wallet_pay_invoice(
    app: *mut NmpApp,
    bolt11: *const c_char,
    amount_msats_json: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(bolt11) = c_string_argument(bolt11) else {
        return;
    };
    let amount_msats = c_optional_string_argument(amount_msats_json)
        .and_then(|s| s.parse::<u64>().ok());
    app.send_cmd(ActorCommand::WalletPayInvoice {
        bolt11,
        amount_msats,
        correlation_id: None,
    });
}
