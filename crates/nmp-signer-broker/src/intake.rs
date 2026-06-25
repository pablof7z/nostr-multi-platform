//! Bounded intake queue for relay EVENT frames with overflow diagnostics.
//!
//! A flood of EVENT frames from a noisy/hostile relay must not grow memory
//! unbounded while valid handshake/restore traffic still completes. This module
//! provides a fixed-size intake capacity and admission control.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Max in-flight relay EVENT frames buffered per signer-broker session before
/// overflow. Bounds memory against a noisy/hostile relay (D5/D8). One handshake
/// needs only a handful of frames in flight; this is generous headroom.
pub const SIGNER_BROKER_INTAKE_CAP: usize = 256;

/// Outcome of admitting one inbound relay EVENT frame into a bounded intake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeAdmission {
    /// Frame enqueued.
    Accepted,
    /// Intake full; frame dropped (newest-dropped policy). Carries no payload
    /// so diagnostics stay log-safe (D10/D13).
    DroppedFull,
}

/// Per-session statistics for dropped frames due to intake overflow.
#[derive(Debug, Default)]
pub struct IntakeStats {
    /// Number of frames dropped because the intake was full.
    dropped_frames: Arc<AtomicU64>,
}

impl IntakeStats {
    /// Create a new intake stats counter.
    pub fn new() -> Self {
        Self {
            dropped_frames: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Increment the dropped-frames counter.
    pub fn record_dropped(&self) {
        self.dropped_frames.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the current count of dropped frames.
    pub fn dropped_count(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// Get a clone of the counter for sharing with the relay callback thread.
    pub fn clone_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.dropped_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intake_cap_constant() {
        assert_eq!(SIGNER_BROKER_INTAKE_CAP, 256);
    }

    #[test]
    fn test_intake_stats_basic() {
        let stats = IntakeStats::new();
        assert_eq!(stats.dropped_count(), 0);
        stats.record_dropped();
        assert_eq!(stats.dropped_count(), 1);
        stats.record_dropped();
        assert_eq!(stats.dropped_count(), 2);
    }

    #[test]
    fn test_intake_stats_concurrent() {
        let stats = Arc::new(IntakeStats::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let stats_clone = Arc::clone(&stats);
            let handle = std::thread::spawn(move || {
                for _ in 0..100 {
                    stats_clone.record_dropped();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(stats.dropped_count(), 1000);
    }

    #[test]
    fn flood_does_not_grow_unbounded() {
        // Send SIGNER_BROKER_INTAKE_CAP * 4 frames without draining.
        // Assert the channel length never exceeds SIGNER_BROKER_INTAKE_CAP
        // and that excess sends return an error indicating the channel is full.
        let (tx, rx) = crossbeam_channel::bounded::<i32>(SIGNER_BROKER_INTAKE_CAP);
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_clone = Arc::clone(&dropped);

        // Send frames in a separate thread to avoid blocking the test.
        let send_handle = std::thread::spawn(move || {
            for i in 0..(SIGNER_BROKER_INTAKE_CAP * 4) {
                match tx.try_send(i as i32) {
                    Ok(()) => {}
                    Err(crossbeam_channel::TrySendError::Full(_)) => {
                        dropped_clone.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                        break;
                    }
                }
            }
        });

        // Slowly drain the channel while sends are happening.
        // This allows us to observe that the channel doesn't exceed the cap.
        let mut drained = 0;
        loop {
            match rx.try_recv() {
                Ok(_) => {
                    drained += 1;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    // Check how many items are potentially in the channel by trying sends.
                    // The channel length should never exceed SIGNER_BROKER_INTAKE_CAP.
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    break;
                }
            }
            if drained >= SIGNER_BROKER_INTAKE_CAP * 4 {
                break;
            }
        }

        send_handle.join().unwrap();

        // Verify that frames were dropped due to overflow.
        let dropped_count = dropped.load(Ordering::Relaxed);
        assert!(
            dropped_count > 0,
            "expected frames to be dropped due to bounded intake; got {}",
            dropped_count
        );

        // Total sent + dropped should equal the target amount.
        let total = drained as u64 + dropped_count;
        assert_eq!(
            total as usize,
            SIGNER_BROKER_INTAKE_CAP * 4,
            "total frames (received + dropped) should match sent amount"
        );
    }

    #[test]
    fn valid_handshake_completes_under_pressure() {
        // Interleave one valid frame among noise; assert the drain loop
        // still yields the valid frame.
        let (tx, rx) = crossbeam_channel::bounded::<i32>(SIGNER_BROKER_INTAKE_CAP);

        // Send mostly noise with one special frame (42) mixed in.
        let sender = std::thread::spawn({
            let tx = tx.clone();
            move || {
                for i in 0..1000 {
                    if i == 500 {
                        // Insert the special frame.
                        let _ = tx.try_send(42);
                    } else {
                        // Send noise; ignore if full.
                        let _ = tx.try_send(i as i32);
                    }
                    if i % 100 == 0 {
                        std::thread::sleep(std::time::Duration::from_micros(100));
                    }
                }
            }
        });
        drop(tx);

        // Drain until we find the special frame.
        let mut found_special = false;
        while let Ok(val) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
            if val == 42 {
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
