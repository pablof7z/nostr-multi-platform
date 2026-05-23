//! NIP-47 Nostr Wallet Connect FFI wrappers.
//!
//! Connection lifecycle (`nmp_app_wallet_connect` / `nmp_app_wallet_disconnect`)
//! is fire-and-forget bespoke FFI per the Theme A discriminator
//! ([`crate::substrate::action`] module docs): these are connection-oriented
//! protocol glue, not user-authored content actions. They send
//! [`crate::actor::ActorCommand::WalletConnect`] / `WalletDisconnect` directly
//! because they address an in-process connection lifecycle, not a dispatchable
//! intent.
//!
//! [`nmp_app_wallet_pay_invoice`] is the user-initiated intent surface and
//! routes through the [`crate::ffi::action::nmp_app_dispatch_action`] seam
//! (closes the V3 bypass — see `wallet/action.rs` module docs). The
//! `WalletPayInvoiceModule` registered in
//! [`crate::kernel::action_registry::default_registry`] under namespace
//! `nmp.wallet.pay_invoice` is the sole entry point that constructs
//! [`crate::actor::ActorCommand::WalletPayInvoice`].
//!
//! Outcomes surface via subsequent snapshots: the wallet state under
//! `projections["wallet"]` (D0: NIP-47 NWC is an app noun, surfaced through
//! the snapshot-projection seam, not a typed `KernelSnapshot` field) and any
//! error under `last_error_toast`. Dispatched `pay_invoice` calls also reach
//! `projections["action_stages"]` via the registry-minted correlation_id so
//! a host spinner can close on the matching kind:23195 response.

use super::action::dispatch_action_json;
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
/// # V3 — `dispatch_action` is the sole user-write seam
///
/// This symbol is the thin C-ABI wrapper that translates its arguments into
/// a [`crate::wallet::WalletAction::PayInvoice`] payload and routes the call
/// through the [`crate::ffi::action::nmp_app_dispatch_action`] seam (the
/// `nmp.wallet.pay_invoice` namespace registered in
/// [`crate::kernel::action_registry::default_registry`]). The
/// `ActionRegistry` executor is the sole constructor of
/// [`crate::actor::ActorCommand::WalletPayInvoice`] from FFI — this body
/// never sends an `ActorCommand` directly (D4: every user-initiated write
/// enters through `dispatch_action`).
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

    // Translate the call into a `WalletAction::PayInvoice` payload and route
    // through `dispatch_action_json`. The registry's `WalletPayInvoiceModule`
    // executor is the sole constructor of `ActorCommand::WalletPayInvoice`
    // from FFI (V3 — `dispatch_action` is the sole user-write seam).
    //
    // `serde_json::to_string` cannot fail for this `WalletAction` shape
    // (the fields are a `String` and `Option<u64>`, both always-serialisable),
    // but D6 mandates "failures are data, never panics": a hypothetical
    // serialisation failure collapses to a silent drop, exactly the same
    // observable shape as a poisoned-mutex degradation above.
    let action = crate::wallet::WalletAction::PayInvoice { bolt11, amount_msats };
    let Ok(action_json) = serde_json::to_string(&action) else {
        return;
    };
    // The return string carries the minted correlation_id or an error
    // envelope — the C-ABI symbol is fire-and-forget (matches the existing
    // contract), so the result is dropped. The action-stages lifecycle
    // (registered via `is_async_completing = true` on the module) still
    // reaches the host through `projections["action_stages"]` keyed on the
    // same correlation_id; a host that needs the id at call time can call
    // `nmp_app_dispatch_action("nmp.wallet.pay_invoice", ...)` directly.
    let _ = dispatch_action_json(Some(app), "nmp.wallet.pay_invoice", &action_json);
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
    /// `bolt11` must result in exactly one dispatch — the second call is the
    /// UI double-tap and must be silently dropped by the wallet-specific
    /// bolt11 guard BEFORE it reaches `dispatch_action_json`.
    ///
    /// Witness: the `inflight_bolt11` set has exactly one entry after the
    /// pair (a rejected re-tap inserts nothing). Asserting the generic
    /// `inflight_dispatches` set size (also expected to be 1) is an
    /// independent witness covered by
    /// `fire_and_forget_wrapper_routes_through_dispatch_action`.
    #[test]
    fn same_bolt11_twice_enqueues_exactly_once() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11 = CString::new("lnbc100n1p0fakefakefakebolt11invoicestring").unwrap();

            // First call: passes both the bolt11 guard and dispatch_action.
            // Second call: short-circuits at the bolt11 guard — the generic
            // dispatch guard never sees it.
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
                // would be before the platform's `Instant` epoch — but on
                // every platform `nmp-core` targets, the monotonic clock
                // counts from boot (not process start), so subtracting 61s
                // from `Instant::now()` is always representable. Use
                // `.expect()` so a future hypothetical platform whose
                // `Instant` epoch sits inside the TTL window fails this
                // test loudly instead of silently passing for the wrong
                // reason (a fall-back to "now" would leave the entry fresh,
                // the sweep would skip it, and the second call would
                // short-circuit on the still-present key).
                let backdated = Instant::now()
                    .checked_sub(INFLIGHT_BOLT11_TTL + Duration::from_secs(1))
                    .expect("Instant::checked_sub(61s) must succeed on every supported platform");
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

    /// V3 contract — the C-ABI wrapper routes through `dispatch_action`.
    ///
    /// A successful `nmp_app_wallet_pay_invoice` call MUST land an entry in
    /// the generic `inflight_dispatches` map (the dedup keyed on
    /// `hash(namespace, action_json)` that ALL `nmp_app_dispatch_action`
    /// calls populate on accepted execution). If the wrapper were still
    /// constructing `ActorCommand::WalletPayInvoice` directly (the V3
    /// bypass), no `inflight_dispatches` entry would appear — only the
    /// wallet-specific `inflight_bolt11` entry would. Both must appear
    /// today; this test fails closed if a future refactor accidentally
    /// reverts the body to a direct `send_cmd`.
    ///
    /// Pairs with `default_registry_has_wallet_pay_invoice_module_under_feature`
    /// in `kernel/action_registry.rs`: that test proves the module is
    /// registered, this test proves the FFI symbol uses the registered
    /// module instead of bypassing it.
    #[test]
    fn fire_and_forget_wrapper_routes_through_dispatch_action() {
        with_app(|app| {
            let app_ptr = app as *const _ as *mut NmpApp;
            let bolt11 = CString::new("lnbc100n1p0v3contracttest").unwrap();

            // Pre-state: neither inflight map carries an entry.
            assert_eq!(
                app.inflight_bolt11.lock().unwrap().len(),
                0,
                "preconditions: inflight_bolt11 must start empty"
            );
            assert_eq!(
                app.inflight_dispatches.lock().unwrap().len(),
                0,
                "preconditions: inflight_dispatches must start empty"
            );

            nmp_app_wallet_pay_invoice(app_ptr, bolt11.as_ptr(), std::ptr::null());

            // Post-state: BOTH the wallet-specific bolt11 guard AND the
            // generic dispatch dedup guard carry an entry — proving the
            // call went through `dispatch_action_json` (which populates
            // `inflight_dispatches`) instead of a direct `send_cmd` (which
            // does not).
            assert_eq!(
                app.inflight_bolt11.lock().unwrap().len(),
                1,
                "the wallet-specific bolt11 guard must record the accepted call"
            );
            assert_eq!(
                app.inflight_dispatches.lock().unwrap().len(),
                1,
                "the generic dispatch_action guard must also record the accepted call — \
                 a missing entry here means the V3 bypass has been reintroduced"
            );
        });
    }
}
