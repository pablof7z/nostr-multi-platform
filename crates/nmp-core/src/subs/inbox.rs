//! Trigger inbox — FIFO + per-tick coalescing.
//!
//! Recompilation triggers from the actor's various sources (ingest, view
//! registry, relay worker, session machine, signer) fan into this single
//! queue. At each tick boundary, `drain_coalesced` returns all queued
//! triggers; the lifecycle then runs at most one compile pass regardless
//! of cardinality (D8 budget per ADR-0002).
//!
//! Coalescing semantics (recompilation.md §4.3):
//!
//! - Pure-replay triggers (A5 `RelayReconnected`) survive coalescing but do
//!   not increment the per-tick compile count — the lifecycle drains them
//!   separately via `handle_reconnect`.
//! - All other triggers fold into one compile pass per tick.
//! - An empty queue produces an empty drain (zero allocations after warmup).

use std::collections::VecDeque;

use super::trigger::CompileTrigger;

/// FIFO trigger inbox.
#[derive(Default)]
pub struct TriggerInbox {
    queue: VecDeque<CompileTrigger>,
}

impl TriggerInbox {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue one trigger. O(1).
    pub fn enqueue(&mut self, trigger: CompileTrigger) {
        self.queue.push_back(trigger);
    }

    /// Drain everything currently queued; returns the full vector (caller
    /// applies coalescing semantics — `SubscriptionLifecycle::drain_tick`
    /// runs at most one compile across the lot).
    ///
    /// Empty queue → empty vec, no allocation beyond a default `Vec::new()`.
    #[must_use]
    pub fn drain_coalesced(&mut self) -> Vec<CompileTrigger> {
        if self.queue.is_empty() {
            return Vec::new();
        }
        let count = self.queue.len();
        let mut out = Vec::with_capacity(count);
        while let Some(t) = self.queue.pop_front() {
            out.push(t);
        }
        out
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::super::trigger::InvalidateReason;
    use super::*;

    #[test]
    fn drain_returns_in_order() {
        let mut inbox = TriggerInbox::new();
        inbox.enqueue(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::External("first".to_string()),
        });
        inbox.enqueue(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::External("second".to_string()),
        });
        let drained = inbox.drain_coalesced();
        assert_eq!(drained.len(), 2);
        // Order preserved.
        if let CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::External(s),
        } = &drained[0]
        {
            assert_eq!(s, "first");
        } else {
            panic!("expected first")
        }
    }

    #[test]
    fn empty_drain_returns_empty() {
        let mut inbox = TriggerInbox::new();
        assert!(inbox.drain_coalesced().is_empty());
        assert!(inbox.is_empty());
    }

    #[test]
    fn drain_empties_queue() {
        let mut inbox = TriggerInbox::new();
        for _ in 0..10 {
            inbox.enqueue(CompileTrigger::InvalidateCompile {
                reason: InvalidateReason::TestForceRecompile,
            });
        }
        assert_eq!(inbox.len(), 10);
        let _ = inbox.drain_coalesced();
        assert_eq!(inbox.len(), 0);
    }
}
