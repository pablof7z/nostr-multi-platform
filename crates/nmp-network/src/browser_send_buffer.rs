//! Pre-connect outbound send buffer for the browser relay transport (#2765).
//!
//! [`BrowserRelayDriver::send_text`] buffers frames while a relay's
//! `web_sys::WebSocket` is still `CONNECTING` (or between a close and the next
//! reconnect dial). This module is the pure, non-`cfg`-gated bookkeeping that
//! buffer needs — mirroring [`crate::role::RelayRole`]'s "always compiled"
//! shape so the eviction/drop accounting is unit-testable on native targets
//! even though [`crate::browser_driver`] itself is `#[cfg(target_arch =
//! "wasm32")]`-gated.
//!
//! # Never a silent loss (D6)
//!
//! The inbound side of the browser relay transport
//! (`nmp-browser-runtime::relay::inbound::InboundQueue`) bounds its queue and
//! surfaces every overflow-evicted frame through an explicit
//! `BrowserRuntimeEvent::RelayInboundDropped` event. Before this module the
//! outbound pre-connect buffer had no counterpart: `BrowserRelayDriver::send_text`
//! evicted the oldest buffered frame on overflow and returned `Ok(())` — a
//! publish `EVENT` queued behind others to a slow-to-open relay could be lost
//! with the app believing it had published. [`PreConnectSendBuffer`] records
//! every eviction in `dropped` so the caller (`nmp-browser-runtime`) can drain
//! them and surface a `RelayOutboundDropped` event plus, for `EVENT` frames, a
//! terminal failure that un-sticks the publish engine.

use std::collections::VecDeque;

use crate::role::RelayRole;

/// One frame evicted from a [`PreConnectSendBuffer`] on overflow, enough
/// context for the host to classify and surface it (relay identity + frame
/// kind is derivable from `text` by the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedOutboundFrame {
    /// The relay URL the frame was queued for.
    pub url: String,
    /// The transport-lane role the driver reports under.
    pub role: RelayRole,
    /// The raw outbound frame text that was evicted (never sent).
    pub text: String,
}

/// Bounded pre-connect outbound buffer + drop accounting.
///
/// `pending` holds frames enqueued before the socket reaches `OPEN`, bounded
/// by `capacity`; `dropped` records the frames evicted on overflow (D6-honest
/// — never a silent loss), itself bounded by `capacity` so a relay that never
/// connects under sustained publish load cannot grow the drop log unboundedly.
pub struct PreConnectSendBuffer {
    capacity: usize,
    pending: VecDeque<String>,
    dropped: VecDeque<String>,
}

impl PreConnectSendBuffer {
    /// Construct an empty buffer bounded at `capacity` frames.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            pending: VecDeque::new(),
            dropped: VecDeque::new(),
        }
    }

    /// Enqueue `text`. When the buffer is already at capacity the oldest
    /// pending frame is evicted into `dropped` (bounded the same way) before
    /// `text` is pushed — never a silent loss.
    pub fn push(&mut self, text: String) {
        if self.pending.len() >= self.capacity {
            if let Some(evicted) = self.pending.pop_front() {
                if self.dropped.len() >= self.capacity {
                    self.dropped.pop_front();
                }
                self.dropped.push_back(evicted);
            }
        }
        self.pending.push_back(text);
    }

    /// Drain all currently-pending frames (in enqueue order) for on-open
    /// flush. Leaves `dropped` untouched.
    pub fn drain_pending(&mut self) -> Vec<String> {
        self.pending.drain(..).collect()
    }

    /// Drain all recorded drops (oldest first), resetting the drop log.
    /// Called once per pump turn so each drop is surfaced exactly once.
    pub fn take_dropped(&mut self) -> Vec<String> {
        self.dropped.drain(..).collect()
    }

    /// Clear both the pending and dropped logs. Used on host-initiated
    /// teardown (`BrowserRelayDriver::close()`), which intentionally discards
    /// any buffered-but-unsurfaced records — no reconnect is coming.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.dropped.clear();
    }

    /// Number of frames currently pending (diagnostic / test helper).
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True when no frames are pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_buffer(capacity: usize) -> PreConnectSendBuffer {
        let mut buf = PreConnectSendBuffer::new(capacity);
        for i in 0..capacity {
            buf.push(format!("frame-{i}"));
        }
        buf
    }

    #[test]
    fn drop_oldest_when_full() {
        let mut buf = full_buffer(4);
        assert!(buf.take_dropped().is_empty());

        buf.push("new-frame".to_string());
        let dropped = buf.take_dropped();
        assert_eq!(dropped, vec!["frame-0".to_string()], "oldest frame dropped");
        assert_eq!(buf.len(), 4, "buffer stays at capacity");

        let pending = buf.drain_pending();
        assert_eq!(
            pending.last(),
            Some(&"new-frame".to_string()),
            "newest frame must be at the back"
        );
    }

    #[test]
    fn dropped_overflow_records_each_eviction_once() {
        let mut buf = full_buffer(4);
        for i in 4..7 {
            buf.push(format!("frame-{i}"));
        }
        let dropped = buf.take_dropped();
        assert_eq!(
            dropped,
            vec![
                "frame-0".to_string(),
                "frame-1".to_string(),
                "frame-2".to_string(),
            ],
            "three overflow pushes evict the three oldest frames, oldest-first"
        );

        // Second take returns empty: already-surfaced drops are not re-reported.
        assert!(buf.take_dropped().is_empty());
    }

    #[test]
    fn dropped_log_itself_is_bounded() {
        // A relay that never opens under sustained publish load must not grow
        // the drop log unboundedly: capacity+N pushes past a full buffer only
        // ever retain `capacity` drop records.
        let mut buf = full_buffer(2);
        for i in 2..20 {
            buf.push(format!("frame-{i}"));
        }
        let dropped = buf.take_dropped();
        assert_eq!(dropped.len(), 2, "drop log bounded to capacity");
    }

    #[test]
    fn drain_pending_empties_buffer() {
        let mut buf = full_buffer(3);
        let drained = buf.drain_pending();
        assert_eq!(drained.len(), 3);
        assert!(buf.is_empty());
    }

    #[test]
    fn clear_drops_both_logs() {
        let mut buf = full_buffer(2);
        buf.push("evicted".to_string());
        buf.clear();
        assert!(buf.is_empty(), "pending cleared");
        assert!(
            buf.take_dropped().is_empty(),
            "unsurfaced drop records are discarded on host-initiated teardown"
        );
    }
}
