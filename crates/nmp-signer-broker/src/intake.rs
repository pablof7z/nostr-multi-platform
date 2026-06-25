//! Bounded intake queue for relay EVENT frames with overflow diagnostics.
//!
//! A flood of EVENT frames from a noisy/hostile relay must not grow memory
//! unbounded while valid handshake/restore traffic still completes. This module
//! provides a fixed-size intake capacity and admission control.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use serde_json::Value;

/// Max in-flight relay EVENT frames buffered per signer-broker session before
/// overflow. Bounds memory against a noisy/hostile relay (D5/D8). One handshake
/// needs only a handful of frames in flight; this is generous headroom.
pub const SIGNER_BROKER_INTAKE_CAP: usize = 256;

/// Single source of truth for bounded intake: owns the channel and drop counter.
/// Provides non-blocking admission with automatic drop counting.
#[derive(Debug)]
pub struct BoundedIntake {
    sender: Sender<Value>,
    receiver: Receiver<Value>,
    dropped_frames: Arc<AtomicU64>,
}

impl BoundedIntake {
    /// Create a new bounded intake with capacity SIGNER_BROKER_INTAKE_CAP.
    pub fn new() -> Self {
        let (sender, receiver) = bounded::<Value>(SIGNER_BROKER_INTAKE_CAP);
        Self {
            sender,
            receiver,
            dropped_frames: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Attempt to admit a frame. Non-blocking: returns immediately whether the
    /// frame was enqueued or dropped. On Full, increments the drop counter and
    /// returns false. Callers check the return value to decide whether to return
    /// true (frame accepted) or false (frame dropped) to the relay callback.
    pub fn try_admit(&self, event: Value) -> bool {
        match self.sender.try_send(event) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped_frames.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                // Receiver dropped; silently drop without counting (session ended).
                false
            }
        }
    }

    /// Get the current count of frames dropped due to overflow.
    pub fn dropped_count(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// Return a clone of the receiver for use by the transport dispatcher.
    pub fn receiver(&self) -> Receiver<Value> {
        self.receiver.clone()
    }

    /// Return a cloneable handle to the drop counter for diagnostics emission.
    pub fn drop_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.dropped_frames)
    }
}

impl Default for BoundedIntake {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay_client::EventCallback;

    #[test]
    fn test_intake_cap_constant() {
        assert_eq!(SIGNER_BROKER_INTAKE_CAP, 256);
    }

    #[test]
    fn test_bounded_intake_basic() {
        let intake = BoundedIntake::new();
        assert_eq!(intake.dropped_count(), 0);

        let event = serde_json::json!({"kind": 24133});
        let accepted = intake.try_admit(event);
        assert!(accepted, "should accept first frame");

        assert_eq!(intake.dropped_count(), 0);
    }

    #[test]
    fn test_bounded_intake_drop_counter() {
        let intake = Arc::new(BoundedIntake::new());

        // Get a cloned reference to the drop counter.
        let counter = intake.drop_counter();

        // Verify we can read the counter independently.
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 0);

        // Fill the intake and cause drops.
        for i in 0..(SIGNER_BROKER_INTAKE_CAP * 2) {
            let event = serde_json::json!({"kind": 24133, "index": i});
            let _ = intake.try_admit(event);
        }

        // Verify the counter was incremented (accessible via the cloned handle).
        let drops_via_counter = counter.load(std::sync::atomic::Ordering::Relaxed);
        let drops_via_intake = intake.dropped_count();
        assert_eq!(drops_via_counter, drops_via_intake);
        assert!(drops_via_intake > 0);
    }

    #[test]
    fn flood_does_not_grow_unbounded() {
        // Send frames aggressively to exceed the bounded cap.
        // Verify drop counter increments when channel is full.
        let intake = Arc::new(BoundedIntake::new());

        // Send more frames than cap without draining.
        let intake_for_send = Arc::clone(&intake);
        let send_handle = std::thread::spawn(move || {
            // Send 512 items into a 256-item cap (2x cap).
            // All try_admit calls complete immediately (non-blocking).
            for i in 0..(SIGNER_BROKER_INTAKE_CAP * 2) {
                let event = serde_json::json!({"kind": 24133, "index": i});
                let _ = intake_for_send.try_admit(event);
            }
        });

        send_handle.join().unwrap();

        // Verify drops occurred (512 attempts on a 256 cap must drop at least 256).
        let dropped_count = intake.dropped_count();
        assert!(
            dropped_count > 0,
            "expected frames to be dropped due to bounded intake; got {}",
            dropped_count
        );
        // At most 256 items drained (the cap), at least 256 dropped.
        assert!(
            dropped_count >= SIGNER_BROKER_INTAKE_CAP as u64 / 2,
            "expected at least {} drops, got {}",
            SIGNER_BROKER_INTAKE_CAP / 2,
            dropped_count
        );
    }

    #[test]
    fn event_callback_integration_with_flood() {
        // Test the real EventCallback integration path: verify that a flood
        // through the callback increments drop counter while valid interleaved
        // frames are still drained.
        let intake = Arc::new(BoundedIntake::new());
        let intake_for_cb = Arc::clone(&intake);

        // Create an EventCallback that uses BoundedIntake (the production pattern).
        let event_cb: EventCallback = Arc::new(move |event| {
            intake_for_cb.try_admit(event)
        });

        let rx = intake.receiver();

        // Spawn sender that floods with noise and interleaves one special frame.
        let send_handle = std::thread::spawn({
            let cb = Arc::clone(&event_cb);
            move || {
                for i in 0..1000 {
                    if i == 500 {
                        // Special frame.
                        let special = serde_json::json!({"kind": 24133, "special": true});
                        let _ = cb(special);
                    } else {
                        // Noise.
                        let noise = serde_json::json!({"kind": 24133, "index": i});
                        let _ = cb(noise);
                    }
                    if i % 100 == 0 {
                        std::thread::sleep(std::time::Duration::from_micros(100));
                    }
                }
            }
        });

        // Drain until we find the special frame, then stop.
        let mut found_special = false;
        while let Ok(event) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
            if let Some(true) = event.get("special").and_then(|v| v.as_bool()) {
                found_special = true;
                break;
            }
        }

        send_handle.join().unwrap();

        assert!(
            found_special,
            "valid frame should complete even under pressure from noise frames"
        );
        // At least one frame should have been dropped (1000 tries into a 256-cap).
        assert!(
            intake.dropped_count() > 0,
            "expected drops due to flood; got {}",
            intake.dropped_count()
        );
    }

    #[test]
    fn valid_handshake_completes_under_pressure() {
        // Test that valid handshake frame completes even under noise pressure.
        let intake = Arc::new(BoundedIntake::new());
        let intake_for_send = Arc::clone(&intake);
        let rx = intake.receiver();

        let sender = std::thread::spawn(move || {
            for i in 0..1000 {
                if i == 500 {
                    // Send a special marker event.
                    let special = serde_json::json!({"kind": 24133, "special": true, "id": "42"});
                    let _ = intake_for_send.try_admit(special);
                } else {
                    // Send noise.
                    let noise = serde_json::json!({"kind": 24133, "index": i});
                    let _ = intake_for_send.try_admit(noise);
                }
                if i % 100 == 0 {
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        });

        let mut found_special = false;
        while let Ok(event) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
            if let Some(true) = event.get("special").and_then(|v| v.as_bool()) {
                found_special = true;
                break;
            }
        }

        sender.join().unwrap();

        assert!(
            found_special,
            "valid handshake frame should complete even under pressure from noise frames"
        );
    }
}
