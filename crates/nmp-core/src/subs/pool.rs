//! Connection pool — uniform send-path shared by REQ emission (M2), NIP-77
//! reconciliation (M4), NIP-42 auth (M5), and event publishing (M7).
//!
//! ## D7 — pool reports; actor decides
//!
//! The pool exposes "send to this relay" and "is this relay connected?"; it
//! does NOT decide reconnect policy, does NOT spawn workers, does NOT retry
//! on its own. The actor (or the existing `relay_worker`) owns those calls.
//!
//! ## Defer-on-disconnect discipline
//!
//! Sending to a disconnected relay returns [`PoolSendOutcome::Deferred`] and
//! the frame is held in a per-relay deferred queue. On reconnect, the actor
//! calls [`ConnectionPool::drain_deferred`] to flush those frames in FIFO
//! order. This matches the existing `Kernel::defer_outbound` semantics and
//! M5/M7 will use the same code path.

use std::collections::{HashMap, VecDeque};

/// Outcome of a send attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolSendOutcome {
    /// Frame was handed to the underlying transport.
    Sent,
    /// Relay is not connected; frame was queued in the deferred buffer.
    Deferred,
    /// Frame was dropped because the deferred buffer would have exceeded its
    /// per-relay cap. Returned only when the cap is finite and exhausted.
    DroppedOverflow,
}

/// Trait the actor speaks to. Production implementation wraps the existing
/// `relay_worker` channels; the in-memory implementation below drives tests.
pub trait ConnectionPool {
    /// Send a single frame to `relay_url`.
    fn send(&mut self, relay_url: &str, frame: String) -> PoolSendOutcome;

    /// Number of frames currently held in the deferred buffer for the relay.
    fn deferred_count(&self, relay_url: &str) -> usize;

    /// Drain the deferred buffer; returns frames in FIFO order. Caller is
    /// responsible for re-sending them (no implicit retry — D7).
    fn drain_deferred(&mut self, relay_url: &str) -> Vec<String>;

    /// Mark a relay as connected. Frames sent to a connected relay return
    /// `Sent` instead of `Deferred`. Idempotent.
    fn mark_connected(&mut self, relay_url: &str);

    /// Mark a relay as disconnected. Future sends defer. Idempotent.
    fn mark_disconnected(&mut self, relay_url: &str);

    /// Whether a given relay is currently considered connected.
    fn is_connected(&self, relay_url: &str) -> bool;
}

// ─── InMemoryPool — test-support implementation ─────────────────────────────

/// Per-relay deferred-buffer cap. The cap matches the existing kernel
/// `defer_outbound` cap of 64 frames — see `kernel/requests/mod.rs`.
const DEFAULT_DEFERRED_CAP: usize = 64;

/// In-memory connection pool. Frames sent to a connected relay are recorded
/// in `sent_log`; frames sent to a disconnected relay are queued in
/// `deferred`.
///
/// Used by integration tests and by the M11 reactivity bench. Production
/// code wraps the `relay_worker` channel set.
pub struct InMemoryPool {
    /// Per-relay deferred buffers (bounded, FIFO).
    deferred: HashMap<String, VecDeque<String>>,
    /// Per-relay connection state.
    connected: HashMap<String, bool>,
    /// Per-relay sent log (test assertion sink).
    sent_log: HashMap<String, Vec<String>>,
    /// Per-relay deferred cap.
    cap: usize,
}

impl Default for InMemoryPool {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryPool {
    #[must_use]
    pub fn new() -> Self {
        Self::with_cap(DEFAULT_DEFERRED_CAP)
    }

    #[must_use]
    pub fn with_cap(cap: usize) -> Self {
        Self {
            deferred: HashMap::new(),
            connected: HashMap::new(),
            sent_log: HashMap::new(),
            cap,
        }
    }

    /// Read the sent log for a relay. Test-only; preserves order.
    #[must_use]
    pub fn sent_log(&self, relay_url: &str) -> Vec<String> {
        self.sent_log.get(relay_url).cloned().unwrap_or_default()
    }
}

impl ConnectionPool for InMemoryPool {
    fn send(&mut self, relay_url: &str, frame: String) -> PoolSendOutcome {
        if self.is_connected(relay_url) {
            self.sent_log
                .entry(relay_url.to_string())
                .or_default()
                .push(frame);
            PoolSendOutcome::Sent
        } else {
            let q = self.deferred.entry(relay_url.to_string()).or_default();
            if q.len() >= self.cap {
                return PoolSendOutcome::DroppedOverflow;
            }
            q.push_back(frame);
            PoolSendOutcome::Deferred
        }
    }

    fn deferred_count(&self, relay_url: &str) -> usize {
        self.deferred
            .get(relay_url)
            .map_or(0, std::collections::VecDeque::len)
    }

    fn drain_deferred(&mut self, relay_url: &str) -> Vec<String> {
        self.deferred
            .remove(relay_url)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default()
    }

    fn mark_connected(&mut self, relay_url: &str) {
        self.connected.insert(relay_url.to_string(), true);
    }

    fn mark_disconnected(&mut self, relay_url: &str) {
        self.connected.insert(relay_url.to_string(), false);
    }

    fn is_connected(&self, relay_url: &str) -> bool {
        self.connected.get(relay_url).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_to_disconnected_defers() {
        let mut pool = InMemoryPool::new();
        let outcome = pool.send("wss://r", "frame-1".to_string());
        assert_eq!(outcome, PoolSendOutcome::Deferred);
        assert_eq!(pool.deferred_count("wss://r"), 1);
    }

    #[test]
    fn send_to_connected_succeeds() {
        let mut pool = InMemoryPool::new();
        pool.mark_connected("wss://r");
        let outcome = pool.send("wss://r", "frame-1".to_string());
        assert_eq!(outcome, PoolSendOutcome::Sent);
        assert_eq!(pool.sent_log("wss://r").len(), 1);
    }

    #[test]
    fn drain_deferred_returns_fifo() {
        let mut pool = InMemoryPool::new();
        pool.send("wss://r", "a".to_string());
        pool.send("wss://r", "b".to_string());
        pool.send("wss://r", "c".to_string());
        let drained = pool.drain_deferred("wss://r");
        assert_eq!(drained, vec!["a", "b", "c"]);
        assert_eq!(pool.deferred_count("wss://r"), 0);
    }

    #[test]
    fn cap_drops_overflow() {
        let mut pool = InMemoryPool::with_cap(2);
        assert_eq!(
            pool.send("wss://r", "a".to_string()),
            PoolSendOutcome::Deferred
        );
        assert_eq!(
            pool.send("wss://r", "b".to_string()),
            PoolSendOutcome::Deferred
        );
        assert_eq!(
            pool.send("wss://r", "c".to_string()),
            PoolSendOutcome::DroppedOverflow
        );
        assert_eq!(pool.deferred_count("wss://r"), 2);
    }
}
