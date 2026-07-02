//! V-51 phase 1 — bounded ring-buffer projection of recent routing decisions.
//!
//! The substrate seam ([`crate::substrate::RoutingTraceObserver`]) fires on
//! every successful `route_publish` / `route_subscription` call. This module
//! ships the projection that consumes those callbacks: two bounded
//! [`VecDeque`]s (one for publish traces, one for subscription traces) with
//! oldest-drop semantics when full.
//!
//! See GitHub issue #968 for the V-51 rollout. Phase 2 wires this
//! projection's [`RoutingTraceProjection::snapshot_publishes`] /
//! `snapshot_subscriptions` outputs to the FFI/wasm snapshot surface so
//! Chirp (phase 3) and the validation CLI (phase 4) can read them.
//!
//! ## Doctrine
//!
//! - **D5** — both ring buffers are hard-bounded by [`Self::capacity`].
//!   Oldest entries are dropped on overflow; the projection never grows
//!   unboundedly with session length.
//! - **D6** — `RwLock` writers panic only on poison; the trait methods catch
//!   poison and degrade to a no-op (the projection's only consumer is
//!   diagnostic, so losing a trace is acceptable; corrupting the kernel
//!   state by propagating a poisoned-lock panic across the FFI boundary is
//!   not).
//! - **D8** — entries hold `Arc`'d strings (`RelayUrl` is already a
//!   reference-counted `String`); the `routed.relays.clone()` is the
//!   only per-trace allocation, scoped to entry size (typically a handful
//!   of URLs per route call). The observer fan-out itself is gated on
//!   `Option::is_some` in the router so the no-projection-installed path
//!   stays zero-alloc.

use crate::time::UNIX_EPOCH;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, RwLock};

use super::{clock::SystemClock, Clock};
use crate::substrate::{
    PublishTrace, RoutedRelaySet, RoutingRelayUrl as RelayUrl, RoutingSource, RoutingTraceObserver,
    SubscriptionTrace,
};

/// Default ring-buffer capacity per stream (publishes / subscriptions). Sized
/// to hold a few minutes of routing activity on an active session — well
/// above an inspector UI's working set (~one screenful of recent rows) and
/// well below any memory concern (one entry ≈ 200 bytes, total cap ≈ 25 KB).
pub const DEFAULT_ROUTING_TRACE_CAPACITY: usize = 64;

/// One captured `route_publish` call.
// `allow(dead_code)`: struct fields are written by the router observer;
// the phase-2 FFI snapshot tick that reads them lands in a follow-up task.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PublishTraceEntry {
    /// Wall-clock ms since Unix epoch at observation time.
    pub at_ms: u64,
    /// The log-safe summary the router constructed.
    pub trace: PublishTrace,
    /// Per-URL resolution attribution, copied off `RoutedRelaySet::relays` at
    /// observation time so the entry is fully owned (no borrow back into the
    /// router's transient call state).
    pub urls: Vec<(RelayUrl, BTreeSet<RoutingSource>)>,
}

/// One captured `route_subscription` call.
// `allow(dead_code)`: same as `PublishTraceEntry` — phase-2 FFI read lands later.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SubscriptionTraceEntry {
    pub at_ms: u64,
    pub trace: SubscriptionTrace,
    pub urls: Vec<(RelayUrl, BTreeSet<RoutingSource>)>,
}

/// Bounded ring-buffer of recent routing decisions. Held by the kernel as
/// `Arc<RoutingTraceProjection>` so a host / FFI snapshot tick (phase 2)
/// and the router observer fan-out share one allocation.
///
/// The `snapshot_*` and `*_len` accessors are public so phase 2's FFI
/// snapshot tick can read them through the [`crate::kernel::Kernel::routing_trace`]
/// accessor; the `#[allow(dead_code)]` keeps the build clean until that
/// consumer lands.
// `allow(dead_code)`: the struct fields and impl block are consumed by phase-2
// FFI; the router observer already writes them — the read side lands later.
#[allow(dead_code)]
pub struct RoutingTraceProjection {
    publishes: RwLock<VecDeque<PublishTraceEntry>>,
    subscriptions: RwLock<VecDeque<SubscriptionTraceEntry>>,
    clock: RwLock<Arc<dyn Clock>>,
    capacity: usize,
}

// `allow(dead_code)`: all public accessors — consumed by phase-2 FFI snapshot
// tick and by tests; the impl-block allow silences the unused-method lint.
#[allow(dead_code)]
impl RoutingTraceProjection {
    /// Construct a projection with [`DEFAULT_ROUTING_TRACE_CAPACITY`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    /// Construct a projection with the default capacity and an injected
    /// kernel clock.
    #[must_use]
    pub(crate) fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self::with_capacity_and_clock(DEFAULT_ROUTING_TRACE_CAPACITY, clock)
    }

    /// Construct a projection with the given per-stream capacity. `capacity`
    /// of `0` silently clamps to `1` — a degenerate value that would
    /// otherwise make every record immediately evict itself.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_and_clock(capacity, Arc::new(SystemClock))
    }

    #[must_use]
    pub(crate) fn with_capacity_and_clock(capacity: usize, clock: Arc<dyn Clock>) -> Self {
        let capacity = capacity.max(1);
        Self {
            publishes: RwLock::new(VecDeque::with_capacity(capacity)),
            subscriptions: RwLock::new(VecDeque::with_capacity(capacity)),
            clock: RwLock::new(clock),
            capacity,
        }
    }

    pub(crate) fn set_clock(&self, clock: Arc<dyn Clock>) {
        if let Ok(mut slot) = self.clock.write() {
            *slot = clock;
        }
    }

    /// Per-stream capacity. `publishes.len() <= capacity()` and
    /// `subscriptions.len() <= capacity()` are invariants.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Snapshot the publish ring (oldest first). Cheap: O(n) clone of the
    /// underlying `VecDeque` into a `Vec`. Returns an empty vec on poisoned
    /// lock (D6).
    #[must_use]
    pub fn snapshot_publishes(&self) -> Vec<PublishTraceEntry> {
        self.publishes
            .read()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Snapshot the subscription ring (oldest first). Same semantics as
    /// [`Self::snapshot_publishes`].
    #[must_use]
    pub fn snapshot_subscriptions(&self) -> Vec<SubscriptionTraceEntry> {
        self.subscriptions
            .read()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Current publish-ring length. Diagnostic / test helper.
    #[must_use]
    pub fn publishes_len(&self) -> usize {
        self.publishes.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Current subscription-ring length.
    #[must_use]
    pub fn subscriptions_len(&self) -> usize {
        self.subscriptions.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Copy `routed.relays` into the owned `Vec<(_, _)>` shape the entries
    /// retain. Single allocation per entry.
    fn copy_urls(routed: &RoutedRelaySet) -> Vec<(RelayUrl, BTreeSet<RoutingSource>)> {
        routed
            .relays
            .iter()
            .map(|(u, s)| (u.clone(), s.clone()))
            .collect()
    }

    /// Current wall-clock ms since Unix epoch through the injected kernel
    /// clock, or `0` if the clock lock is poisoned or pre-epoch.
    fn now_ms(&self) -> u64 {
        let Ok(clock) = self.clock.read() else {
            return 0;
        };
        clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

impl Default for RoutingTraceProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutingTraceObserver for RoutingTraceProjection {
    fn on_publish(&self, summary: PublishTrace, routed: &RoutedRelaySet) {
        let entry = PublishTraceEntry {
            at_ms: self.now_ms(),
            trace: summary,
            urls: Self::copy_urls(routed),
        };
        // D6: drop the entry on poisoned lock rather than propagate a panic.
        if let Ok(mut q) = self.publishes.write() {
            push_bounded(&mut q, entry, self.capacity);
        }
    }

    fn on_subscription(&self, summary: SubscriptionTrace, routed: &RoutedRelaySet) {
        let entry = SubscriptionTraceEntry {
            at_ms: self.now_ms(),
            trace: summary,
            urls: Self::copy_urls(routed),
        };
        if let Ok(mut q) = self.subscriptions.write() {
            push_bounded(&mut q, entry, self.capacity);
        }
    }
}

/// Push `entry` onto `q`, evicting the oldest if at `capacity`. Hard cap.
fn push_bounded<T>(q: &mut VecDeque<T>, entry: T, capacity: usize) {
    while q.len() >= capacity {
        q.pop_front();
    }
    q.push_back(entry);
}

#[cfg(test)]
#[path = "routing_trace_tests.rs"]
mod tests;
