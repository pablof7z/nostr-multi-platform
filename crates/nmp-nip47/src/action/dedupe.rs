//! UI-layer bolt11 double-tap dedup guard for `nmp.wallet.pay_invoice`.
//!
//! Extracted from `action.rs` to keep `action/mod.rs` under the 500-LOC cap.
//! The guard is per-[`WalletPayInvoiceModule`](super::WalletPayInvoiceModule)
//! instance (ADR-0052 rung 5.2: no process-global), so two `NmpApp` instances
//! in one process dedup independently.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Time-to-live for an `inflight_bolt11` entry — the wall-clock window during
/// which a same-invoice retap is rejected as a UI double-tap before it is
/// dispatched through the action seam.
///
/// 60 s is sized for "the NWC response is in flight": long enough to absorb
/// relay round-trip jitter, short enough that a wallet that never responds
/// does not lock the user out of retrying. The `WalletRuntime` owns a separate
/// `PENDING_PAYMENT_TTL_SECS` (90 s) guard for the on-wire dedup window.
pub const INFLIGHT_BOLT11_TTL: Duration = Duration::from_secs(60);

/// Per-module-instance guard that rejects duplicate bolt11 taps within
/// [`INFLIGHT_BOLT11_TTL`].
///
/// Entries are swept lazily on each [`Self::is_duplicate_tap`] call (D8 —
/// no sleep/loop). D6: a poisoned mutex collapses to "let the send through"
/// (no user-visible lockout on poison).
pub(super) struct InflightBolt11Guard {
    inner: Mutex<HashMap<String, Instant>>,
}

impl InflightBolt11Guard {
    /// Construct a fresh, empty guard.
    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `true` if `bolt11` is currently in-flight and the call should
    /// be treated as a duplicate tap. Sweeps expired entries on every call
    /// (D8 — no sleep/loop). D6: a poisoned mutex is treated as "not a
    /// duplicate" so the user is never locked out by a poisoned guard.
    pub(super) fn is_duplicate_tap(&self, bolt11: &str) -> bool {
        let Ok(mut guard) = self.inner.lock() else {
            return false; // D6: poisoned mutex → let through
        };
        let now = Instant::now();
        guard.retain(|_, started| now.duration_since(*started) < INFLIGHT_BOLT11_TTL);
        if guard.contains_key(bolt11) {
            return true;
        }
        guard.insert(bolt11.to_string(), now);
        false
    }

    /// Expose the inner map mutably for test backdating only.
    #[cfg(test)]
    pub(super) fn get_inner_mut(&self) -> std::sync::MutexGuard<'_, HashMap<String, Instant>> {
        self.inner.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bolt11_twice_second_is_duplicate() {
        let guard = InflightBolt11Guard::new();
        assert!(!guard.is_duplicate_tap("lnbc100n1p0dup"), "first tap must not be a duplicate");
        assert!(guard.is_duplicate_tap("lnbc100n1p0dup"), "second tap within TTL must be a duplicate");
    }

    #[test]
    fn different_bolt11_strings_are_independent() {
        let guard = InflightBolt11Guard::new();
        assert!(!guard.is_duplicate_tap("lnbc100n1p0aaaa"));
        assert!(!guard.is_duplicate_tap("lnbc200n1p0bbbb"));
    }

    #[test]
    fn expired_entry_is_not_duplicate() {
        let guard = InflightBolt11Guard::new();
        let bolt11 = "lnbc500n1p0expired";

        assert!(!guard.is_duplicate_tap(bolt11), "first tap must not be a duplicate");

        // Backdate the entry so it appears expired.
        {
            let mut map = guard.get_inner_mut();
            let backdated = Instant::now()
                .checked_sub(INFLIGHT_BOLT11_TTL + Duration::from_secs(1))
                .expect("Instant::checked_sub(61s) must succeed");
            if let Some(v) = map.get_mut(bolt11) {
                *v = backdated;
            }
        }

        assert!(!guard.is_duplicate_tap(bolt11), "retry after TTL must not be a duplicate");
    }
}
