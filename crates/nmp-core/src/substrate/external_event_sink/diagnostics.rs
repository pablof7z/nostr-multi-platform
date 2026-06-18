//! Diagnostic counters for the external event sink dispatcher.
//!
//! Doctrine D6: failures are data, not stderr writes. Instead of silently
//! dropping, the worker increments these atomic counters so a host can observe
//! degradation (a panicking policy, an overflowing inbound channel).

use std::sync::atomic::{AtomicU64, Ordering};

/// Atomic diagnostic counters, shared between the worker thread and the
/// dispatcher handle.
#[derive(Debug, Default)]
pub struct ExternalEventSinkDiagnostics {
    /// Frames dropped because the inbound bounded channel was full.
    channel_overflow_drops: AtomicU64,
    /// Policy `destinations()` calls that panicked and were isolated.
    policy_panics: AtomicU64,
}

/// Immutable snapshot of the counters at one instant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticsSnapshot {
    pub channel_overflow_drops: u64,
    pub policy_panics: u64,
}

impl ExternalEventSinkDiagnostics {
    pub(super) fn inc_channel_overflow_drops(&self) {
        self.channel_overflow_drops.fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn inc_policy_panics(&self) {
        self.policy_panics.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub(super) fn snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            channel_overflow_drops: self.channel_overflow_drops.load(Ordering::Relaxed),
            policy_panics: self.policy_panics.load(Ordering::Relaxed),
        }
    }
}
