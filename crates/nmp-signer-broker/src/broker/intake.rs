use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{Receiver as CbReceiver, TryRecvError, TrySendError};
use serde_json::Value;

use super::BunkerBroker;
use crate::events::{BrokerEvent, RelayIntakeDropReason};
use crate::relay_client::EventCallback;

/// Session-local relay event intake capacity.
///
/// The handshake drains one event at a time, and steady-state dispatch also
/// handles events serially. This bound keeps hostile relays from turning that
/// serial consumer into unbounded memory growth while leaving ample room for
/// reconnect replay bursts.
pub(crate) const RELAY_INTAKE_CAPACITY: usize = 256;

impl BunkerBroker {
    pub(super) fn make_relay_intake(self: &Arc<Self>) -> (EventCallback, CbReceiver<Value>) {
        let (tx, rx) = crossbeam_channel::bounded::<Value>(RELAY_INTAKE_CAPACITY);
        let rx_for_overflow = rx.clone();
        let dropped = Arc::new(AtomicU64::new(0));
        let broker = Arc::clone(self);
        let event_cb: EventCallback = Arc::new(move |event| {
            enqueue_or_drop_oldest(
                &tx,
                &rx_for_overflow,
                event,
                &dropped,
                &|reason, dropped_total| {
                    broker.emit_relay_intake_drop(reason, dropped_total);
                },
            );
        });
        (event_cb, rx)
    }

    fn emit_relay_intake_drop(&self, reason: RelayIntakeDropReason, dropped_total: u64) {
        if should_emit_drop_diagnostic(dropped_total) {
            self.emit(BrokerEvent::RelayIntakeDropped {
                reason,
                dropped_total,
                capacity: RELAY_INTAKE_CAPACITY,
            });
        }
    }
}

fn enqueue_or_drop_oldest(
    tx: &crossbeam_channel::Sender<Value>,
    rx: &CbReceiver<Value>,
    event: Value,
    dropped: &AtomicU64,
    report_drop: &dyn Fn(RelayIntakeDropReason, u64),
) {
    match tx.try_send(event) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(event)) => {
            match rx.try_recv() {
                Ok(_) => {
                    report_actual_drop(dropped, report_drop, RelayIntakeDropReason::DroppedOldest)
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return,
            }
            if let Err(TrySendError::Full(_)) = tx.try_send(event) {
                report_actual_drop(dropped, report_drop, RelayIntakeDropReason::DroppedNewest);
            }
        }
    }
}

fn report_actual_drop(
    dropped: &AtomicU64,
    report_drop: &dyn Fn(RelayIntakeDropReason, u64),
    reason: RelayIntakeDropReason,
) {
    let dropped_total = dropped.fetch_add(1, Ordering::AcqRel) + 1;
    report_drop(reason, dropped_total);
}

fn should_emit_drop_diagnostic(dropped_total: u64) -> bool {
    dropped_total.is_power_of_two()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    #[test]
    fn bounded_intake_keeps_fixed_capacity_and_admits_newest_event() {
        let (tx, rx) = crossbeam_channel::bounded::<Value>(RELAY_INTAKE_CAPACITY);
        let dropped = AtomicU64::new(0);
        let diagnostics = Mutex::new(Vec::new());
        let valid = json!({"id": "valid", "kind": 24133});

        for i in 0..RELAY_INTAKE_CAPACITY {
            enqueue_or_drop_oldest(
                &tx,
                &rx,
                json!({"id": i, "kind": 24133}),
                &dropped,
                &|reason, dropped_total| {
                    diagnostics.lock().unwrap().push((reason, dropped_total));
                },
            );
        }
        enqueue_or_drop_oldest(
            &tx,
            &rx,
            valid.clone(),
            &dropped,
            &|reason, dropped_total| {
                diagnostics.lock().unwrap().push((reason, dropped_total));
            },
        );

        assert_eq!(rx.len(), RELAY_INTAKE_CAPACITY);
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        assert_eq!(
            diagnostics.lock().unwrap().as_slice(),
            &[(RelayIntakeDropReason::DroppedOldest, 1)]
        );

        let drained: Vec<Value> = rx.try_iter().collect();
        assert!(
            drained.iter().any(|event| event == &valid),
            "drop-oldest policy must admit the newest valid relay event"
        );
    }

    #[test]
    fn drop_diagnostics_are_logarithmically_coalesced() {
        let emitted: Vec<u64> = (1..=10)
            .filter(|count| should_emit_drop_diagnostic(*count))
            .collect();
        assert_eq!(emitted, vec![1, 2, 4, 8]);
    }
}
