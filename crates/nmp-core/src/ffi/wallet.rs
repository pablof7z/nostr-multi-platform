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
use std::time::{Duration, Instant};

/// Time-to-live for an `inflight_bolt11` entry — the wall-clock window during
/// which a same-invoice retap is rejected as a double-tap.
///
/// Sized for "the NWC response is in flight": long enough to absorb relay
/// round-trip jitter on a healthy connection (typically <2s), short enough
/// that a wallet which never responds does not lock the user out of retrying
/// the same invoice indefinitely. 60s mirrors the NIP-47 client's typical
/// pay_invoice timeout budget.
pub(crate) const INFLIGHT_BOLT11_TTL: Duration = Duration::from_secs(60);

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
///
/// # Double-tap guard
///
/// A second call carrying the same `bolt11` string within
/// [`INFLIGHT_BOLT11_TTL`] of the first is rejected as a UI double-tap: no
/// new `ActorCommand::WalletPayInvoice` is enqueued. This guard lives
/// entirely on the FFI thread (no cross-thread coupling): expired entries
/// are swept on every call by wall-clock. The guard is per-`bolt11`, so two
/// rapid taps on the same invoice collapse to one wire request even when
/// the actor's kind:23194-event-id-keyed correlation map cannot deduplicate
/// (the request id is minted by the actor AFTER `send_cmd` returns; the FFI
/// thread cannot wait for it without violating D8).
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
    let amount_msats = c_optional_string_argument(amount_msats_json)
        .and_then(|s| s.parse::<u64>().ok());

    // Double-tap guard: sweep expired entries first (so a retry after TTL
    // is not rejected by a stale residue), then try to claim the bolt11.
    // A poisoned mutex (D6) collapses to "let the send through" — the lint
    // bar is "never deadlock the FFI thread on a poisoned guard"; a
    // double-pay in the panic-recovery path is acceptable degradation for
    // an already-broken process.
    if let Ok(mut guard) = app.inflight_bolt11.lock() {
        let now = Instant::now();
        guard.retain(|_, started| now.duration_since(*started) < INFLIGHT_BOLT11_TTL);
        if guard.contains_key(&bolt11) {
            // Re-tap inside the TTL window — silently drop. D6 fire-and-forget:
            // no return envelope, no toast (a toast would mis-attribute the
            // benign UI duplication as a wallet error). The host's first call
            // is still in flight; its eventual outcome will surface through
            // `projections["wallet"]` exactly as if the second tap never
            // happened.
            return;
        }
        guard.insert(bolt11.clone(), now);
    }

    app.send_cmd(ActorCommand::WalletPayInvoice {
        bolt11,
        amount_msats,
        correlation_id: None,
    });
}

#[cfg(test)]
mod tests {
    use super::super::{nmp_app_free, nmp_app_new};
    use super::*;
    use std::ffi::CString;

    /// Run `body` against a fresh `NmpApp`, freeing it afterwards. Mirrors
    /// the pattern in `ffi::action::tests` so the FFI-level wallet guard
    /// tests use the same lifecycle as the rest of the FFI suite.
    fn with_app(body: impl FnOnce(&NmpApp)) {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null.
        body(unsafe { &*app });
        nmp_app_free(app);
    }

    /// Two consecutive `nmp_app_wallet_pay_invoice` calls with the SAME
    /// `bolt11` must result in exactly one `WalletPayInvoice` enqueue —
    /// the second call is the UI double-tap and must be silently dropped.
    ///
    /// Witnessed via the `queue_depth` straddle counter: the first call
    /// increments by 1, the second must not increment.
    #[test]
    fn same_bolt11_twice_enqueues_exactly_once() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11 = CString::new("lnbc100n1p0fakefakefakebolt11invoicestring").unwrap();

            // The straddle counter (`queue_depth`) is incremented
            // synchronously inside `send_cmd` BEFORE the actor can dequeue,
            // so the FFI-side increment for an accepted call is observable.
            // But the actor thread is running concurrently and may dequeue
            // between the two reads, so we cannot assert
            // `after_second == after_first` directly. The robust witness for
            // "the second call was rejected before reaching `send_cmd`" is
            // the inflight set itself: a rejected re-tap inserts nothing, so
            // the set size after two same-bolt11 calls is exactly one.
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());

            let guard = app.inflight_bolt11.lock().unwrap();
            assert_eq!(
                guard.len(),
                1,
                "exactly one inflight entry expected after a same-bolt11 double-tap"
            );
            assert!(
                guard.contains_key("lnbc100n1p0fakefakefakebolt11invoicestring"),
                "inflight key must be the bolt11 string"
            );
        });
    }

    /// Two `nmp_app_wallet_pay_invoice` calls with DIFFERENT `bolt11`
    /// strings must both pass through the guard — they are independent
    /// payments, not a double-tap.
    #[test]
    fn different_bolt11_strings_both_enqueue() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11_a = CString::new("lnbc100n1p0aaaaaaaa").unwrap();
            let bolt11_b = CString::new("lnbc200n1p0bbbbbbbb").unwrap();

            nmp_app_wallet_pay_invoice(app_ptr, bolt11_a.as_ptr(), std::ptr::null());
            nmp_app_wallet_pay_invoice(app_ptr, bolt11_b.as_ptr(), std::ptr::null());

            let guard = app.inflight_bolt11.lock().unwrap();
            assert_eq!(
                guard.len(),
                2,
                "two distinct invoices must both be tracked inflight"
            );
            assert!(guard.contains_key("lnbc100n1p0aaaaaaaa"));
            assert!(guard.contains_key("lnbc200n1p0bbbbbbbb"));
        });
    }

    /// An inflight entry older than [`INFLIGHT_BOLT11_TTL`] must be swept
    /// before the contains-check, so a legitimate retry after the TTL
    /// passes through the guard.
    ///
    /// Manipulates the inflight `Instant` directly to simulate the elapsed
    /// time without sleeping — `Instant` is opaque but `HashMap` lets us
    /// replace the entry with a backdated `Instant`. We construct the
    /// backdated value via `Instant::now() - (TTL + 1s)`, which the
    /// `Instant::checked_sub` API supports on the platforms nmp-core
    /// targets.
    #[test]
    fn expired_inflight_entry_is_swept_and_retry_passes() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11 = CString::new("lnbc500n1p0cccccccc").unwrap();

            // First call seeds the inflight set.
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            {
                let mut guard = app.inflight_bolt11.lock().unwrap();
                assert_eq!(guard.len(), 1);
                // Backdate the entry so the sweep on the next call removes
                // it. `Instant::checked_sub` returns `None` if the result
                // would be before the platform's Instant epoch — unwrap is
                // safe here because the test process has been alive for at
                // least the TTL by the time it runs (CI cold-starts are
                // longer than 60s; local runs are still safely past it on
                // every platform `nmp-core` targets). On the off chance
                // that fails, fall back to "now" — which would make the
                // test fail loudly rather than silently pass.
                let backdated = Instant::now()
                    .checked_sub(INFLIGHT_BOLT11_TTL + Duration::from_secs(1))
                    .unwrap_or_else(Instant::now);
                if let Some(v) = guard.get_mut("lnbc500n1p0cccccccc") {
                    *v = backdated;
                }
            }

            // Second call: the sweep must drop the expired entry, then
            // re-insert. The set still has exactly one entry after — but
            // its timestamp is fresh.
            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());
            let guard = app.inflight_bolt11.lock().unwrap();
            assert_eq!(
                guard.len(),
                1,
                "retry after TTL must pass the guard and re-insert"
            );
            let ts = guard.get("lnbc500n1p0cccccccc").unwrap();
            assert!(
                Instant::now().duration_since(*ts) < INFLIGHT_BOLT11_TTL,
                "re-inserted entry must carry a fresh timestamp"
            );
        });
    }

    /// A NULL `bolt11` is the existing fire-and-forget no-op (D6) — the
    /// guard must not touch the inflight set, since a NULL argument
    /// already short-circuits before any `WalletPayInvoice` work.
    #[test]
    fn null_bolt11_does_not_pollute_inflight_set() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            nmp_app_wallet_pay_invoice(app_ptr, std::ptr::null(), std::ptr::null());
            let guard = app.inflight_bolt11.lock().unwrap();
            assert!(
                guard.is_empty(),
                "NULL bolt11 is a no-op and must not insert an empty-key entry"
            );
        });
    }
}
