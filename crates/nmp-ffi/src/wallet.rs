//! NIP-47 Nostr Wallet Connect FFI wrappers (V-38 thin shims).
//!
//! Pre-V-38 these symbols constructed `ActorCommand::Wallet{Connect,
//! Disconnect,PayInvoice}` directly — but those variants were deleted when
//! the wallet stack moved to `crates/nmp-nip47`. The symbols stay here on
//! `nmp-ffi::wallet` because:
//!
//! * the iOS / Android shells already link them under these names;
//! * adding a `dispatch_action_json("nmp.wallet.connect", ...)` shim is a
//!   non-breaking caller-side change (the wire shape is hidden inside the
//!   shim body).
//!
//! All three symbols now route through the dispatch-action seam
//! (`crate::action::nmp_app_dispatch_action`). The
//! `WalletConnectModule` / `WalletDisconnectModule` / `WalletPayInvoiceModule`
//! registered in `nmp_core::__ffi_internal::action_registry::default_registry`
//! under namespaces `nmp.wallet.{connect,disconnect,pay_invoice}` are the
//! sole entry points that construct the per-command `ProtocolCommand` in
//! `nmp-nip47::protocol::*`.
//!
//! D0: this file ships zero protocol code — it carries no `nmp-nwc`
//! dependency, no `WalletStatus` type, no kind:23194 builder. Every line
//! below is JSON shaping for the dispatch-action seam. The host MUST have
//! registered `nmp.wallet.connect` / `nmp.wallet.disconnect` /
//! `nmp.wallet.pay_invoice` against its `NmpApp`'s `ActionRegistry`
//! (via `nmp_nip47::{WalletConnectModule, WalletDisconnectModule,
//! WalletPayInvoiceModule}`) and called `nmp_nip47::install_wallet_runtime`
//! before any of these symbols are invoked.

use super::action::dispatch_action_json;
use super::{app_ref, c_optional_string_argument, c_string_argument, NmpApp};
use std::ffi::c_char;
use std::time::{Duration, Instant};

/// Time-to-live for an `inflight_bolt11` entry — the wall-clock window
/// during which a same-invoice retap is rejected as a double-tap.
pub(crate) const INFLIGHT_BOLT11_TTL: Duration = Duration::from_secs(60);

/// Connect a NIP-47 wallet using a `nostr+walletconnect://` URI.
///
/// V-38: thin shim — translates into a `nmp.wallet.connect` dispatch action.
/// The actual wallet runtime lives in `nmp-nip47`.
#[no_mangle]
pub extern "C" fn nmp_app_wallet_connect(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    let body = serde_json::json!({ "Connect": { "uri": uri } });
    let action_json = match serde_json::to_string(&body) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = dispatch_action_json(Some(app), "nmp.wallet.connect", &action_json);
}

/// Disconnect the current NIP-47 wallet.
///
/// V-38: thin shim — translates into a `nmp.wallet.disconnect` dispatch
/// action whose `Disconnect` payload is a unit variant.
#[no_mangle]
pub extern "C" fn nmp_app_wallet_disconnect(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = dispatch_action_json(Some(app), "nmp.wallet.disconnect", "\"Disconnect\"");
}

/// Pay a Lightning invoice via the connected NIP-47 wallet.
///
/// V-38: thin shim — translates into a `nmp.wallet.pay_invoice` dispatch
/// action. The wallet-specific bolt11 double-tap guard runs here (UI-side)
/// before dispatch so the wallet runtime never sees the second tap.
///
/// `bolt11`: BOLT-11 invoice string.
/// `amount_msats_or_null`: pointer to optional payment amount in msats (pass
/// `nil` to use the invoice's embedded amount).
///
/// # V3 — `dispatch_action` is the sole user-write seam
///
/// This symbol is the thin C-ABI wrapper that translates its arguments into
/// a `nmp.wallet.pay_invoice` JSON payload and routes the call through the
/// [`crate::ffi::action::nmp_app_dispatch_action`] seam. The
/// `WalletPayInvoiceModule` in `nmp-nip47` is the sole constructor of the
/// underlying `WalletPayInvoiceCommand` `ProtocolCommand` (V3 — `dispatch_action`
/// is the sole user-write seam).
///
/// The registry-minted correlation_id is consumed internally: the wrapper
/// preserves the existing fire-and-forget C-ABI contract (no return value)
/// so the iOS shell + chirp-tui binary continue to compile unchanged. A
/// caller that needs the correlation_id (to bind a UI spinner) can call
/// `nmp_app_dispatch_action("nmp.wallet.pay_invoice", ...)` directly — both
/// paths reach the same module and produce the same `action_stages`
/// lifecycle. The unack'd `action_stages` entry minted by this wrapper
/// auto-evicts under the kernel's `MAX_TRACKED_CORRELATIONS` bound (no
/// memory leak) — the host that called this fire-and-forget symbol does
/// not need to ACK an id it cannot observe.
///
/// # Double-tap guard
///
/// A second call carrying the same `bolt11` string within
/// [`INFLIGHT_BOLT11_TTL`] of the first is rejected as a UI double-tap: no
/// dispatch is performed. This guard lives entirely on the FFI thread (no
/// cross-thread coupling): expired entries are swept on every call by
/// wall-clock. The guard is per-`bolt11` (independent of `amount_msats`),
/// so two rapid taps on the same invoice with different amounts ALSO
/// collapse to one wire request — the generic `inflight_dispatches`
/// guard keyed on `(namespace, action_json)` would not deduplicate those
/// because the JSON differs. The wallet-specific guard runs FIRST so the
/// generic dispatch guard never sees a same-`bolt11` retap.
///
/// A retry of the same invoice AFTER the TTL passes through — the NWC
/// wallet itself is responsible for deduping a true on-the-wire retry by
/// payment hash. The 60s window is sized so that "the response is in
/// flight" remains the dominant deduplication regime; a wallet that never
/// responds does not lock the user out of retrying.
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
    let amount_msats =
        c_optional_string_argument(amount_msats_json).and_then(|s| s.parse::<u64>().ok());

    // Double-tap guard: sweep expired entries first, then try to claim the
    // bolt11. A poisoned mutex (D6) collapses to "let the send through".
    if let Ok(mut guard) = app.inflight_bolt11.lock() {
        let now = Instant::now();
        guard.retain(|_, started| now.duration_since(*started) < INFLIGHT_BOLT11_TTL);
        if guard.contains_key(&bolt11) {
            return;
        }
        guard.insert(bolt11.clone(), now);
    }

    // Translate the call into a `nmp.wallet.pay_invoice` JSON payload and
    // route through `dispatch_action_json`. The registry's
    // `WalletPayInvoiceModule` (in `nmp-nip47`) is the sole constructor of
    // the underlying `WalletPayInvoiceCommand` `ProtocolCommand` (V3 —
    // `dispatch_action` is the sole user-write seam).
    //
    // `serde_json::to_string` cannot fail for this shape (a `String` and an
    // `Option<u64>`, both always-serialisable), but D6 mandates "failures
    // are data, never panics": a hypothetical serialisation failure
    // collapses to a silent drop, exactly the same observable shape as a
    // poisoned-mutex degradation above.
    let body = serde_json::json!({
        "PayInvoice": {
            "bolt11": bolt11,
            "amount_msats": amount_msats,
        }
    });
    let Ok(action_json) = serde_json::to_string(&body) else {
        return;
    };
    let _ = dispatch_action_json(Some(app), "nmp.wallet.pay_invoice", &action_json);
}

#[cfg(test)]
mod tests {
    use super::super::{nmp_app_free, nmp_app_new};
    use super::*;
    use std::ffi::CString;

    fn with_app(body: impl FnOnce(&NmpApp)) {
        let app = nmp_app_new();
        body(unsafe { &*app });
        nmp_app_free(app);
    }

    /// Two consecutive same-bolt11 calls collapse to one inflight entry.
    #[test]
    fn same_bolt11_twice_enqueues_exactly_once() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11 = CString::new("lnbc100n1p0fakefakefakebolt11invoicestring").unwrap();
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            let guard = app.inflight_bolt11.lock().unwrap();
            assert_eq!(guard.len(), 1);
            assert!(guard.contains_key("lnbc100n1p0fakefakefakebolt11invoicestring"));
        });
    }

    /// Different invoices both enqueue.
    #[test]
    fn different_bolt11_strings_both_enqueue() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let a = CString::new("lnbc100n1p0aaaaaaaa").unwrap();
            let b = CString::new("lnbc200n1p0bbbbbbbb").unwrap();
            nmp_app_wallet_pay_invoice(app_ptr, a.as_ptr(), std::ptr::null());
            nmp_app_wallet_pay_invoice(app_ptr, b.as_ptr(), std::ptr::null());
            let guard = app.inflight_bolt11.lock().unwrap();
            assert_eq!(guard.len(), 2);
        });
    }

    /// Expired inflight entry is swept on next call.
    #[test]
    fn expired_inflight_entry_is_swept_and_retry_passes() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11 = CString::new("lnbc500n1p0cccccccc").unwrap();
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            {
                let mut guard = app.inflight_bolt11.lock().unwrap();
                let backdated = Instant::now()
                    .checked_sub(INFLIGHT_BOLT11_TTL + Duration::from_secs(1))
                    .expect("Instant::checked_sub(61s) must succeed");
                if let Some(v) = guard.get_mut("lnbc500n1p0cccccccc") {
                    *v = backdated;
                }
            }
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            let guard = app.inflight_bolt11.lock().unwrap();
            assert_eq!(guard.len(), 1);
            let ts = guard.get("lnbc500n1p0cccccccc").unwrap();
            assert!(Instant::now().duration_since(*ts) < INFLIGHT_BOLT11_TTL);
        });
    }

    /// NULL bolt11 is a no-op (D6).
    #[test]
    fn null_bolt11_does_not_pollute_inflight_set() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            nmp_app_wallet_pay_invoice(app_ptr, std::ptr::null(), std::ptr::null());
            assert!(app.inflight_bolt11.lock().unwrap().is_empty());
        });
    }
}
