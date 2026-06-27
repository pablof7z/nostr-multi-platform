//! Inbound relay event queue and drain for the browser relay transport (#2050).
//!
//! # D4 single-writer contract
//!
//! JS callbacks (WebSocket onopen / onmessage / onclose / onerror) MUST NOT
//! mutate the `KernelReducer` directly. Instead each callback enqueues an
//! [`InboundRelayEvent`] into the shared bounded `VecDeque` and calls `wake()`
//! to schedule a `pump()`. The reducer is mutated only inside `drain_inbound()`
//! which is called from `BrowserRuntime::pump()` — the sole writer.
//!
//! # Budget
//!
//! Each `pump()` drains at most [`super::budgets::BROWSER_RELAY_DRAIN_BUDGET`]
//! events. Leftover events remain in the queue; the drain reports `yielded=true`
//! so the host schedules another `pump()`.
//!
//! # Admission
//!
//! The queue is bounded by [`super::budgets::MAX_INBOUND_QUEUED`]. When full,
//! the oldest event is dropped and the `dropped_inbound` counter is incremented
//! (D6-honest; never a silent loss).

// `InboundRelayEvent` variants and `InboundQueue::push` are only constructed
// from wasm32-gated JS handlers; on native they appear unused. Suppress the
// lint here rather than scattering cfg gates through the enum and struct.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use nmp_core::substrate::{fan_relay_connected_hooks, RelayConnectedHook, RelayTextInterceptor};
use nmp_core::time::Instant;
use nmp_core::{CommandSender, KernelReducer, OutboundMessage, RelayFrame};
use nmp_network::role::RelayRole;

use super::budgets::{BROWSER_RELAY_DRAIN_BUDGET, MAX_INBOUND_QUEUED};
use crate::BrowserRuntimeEvent;

/// One queued inbound event from a relay WebSocket callback.
#[derive(Debug)]
pub(crate) enum InboundRelayEvent {
    /// A relay socket opened (first connect or reconnect).
    Connected {
        role: RelayRole,
        url: String,
        is_reconnect: bool,
    },
    /// An inbound text frame (NIP-01 JSON traffic).
    Text {
        role: RelayRole,
        url: String,
        text: String,
    },
    /// An inbound binary frame (counted, otherwise ignored).
    Binary {
        role: RelayRole,
        url: String,
        bytes: Vec<u8>,
    },
    /// A WebSocket Close frame (optional reason surfaced as last_close_reason).
    Close {
        role: RelayRole,
        url: String,
        reason: Option<String>,
    },
    /// The socket teardown completed (kernel evicts wire-subs, etc.).
    Closed { role: RelayRole, url: String },
    /// A transient socket error.
    Failed {
        role: RelayRole,
        url: String,
        error: String,
    },
}

/// Bounded inbound queue + drop counter, shared via `Rc` between the relay
/// handlers (which push) and `RelayPool` (which drains in `pump()`).
pub(crate) struct InboundQueue {
    pub(crate) queue: RefCell<VecDeque<InboundRelayEvent>>,
    /// Cumulative count of inbound frames dropped (oldest-first) on overflow.
    pub(crate) dropped: Cell<u64>,
    /// The `dropped` value already surfaced to the host via a
    /// [`BrowserRuntimeEvent::RelayInboundDropped`] event. The drain reports the
    /// delta `dropped - reported` each turn so each drop is surfaced exactly
    /// once (D6-honest — never a silent loss).
    reported_dropped: Cell<u64>,
}

impl InboundQueue {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            queue: RefCell::new(VecDeque::new()),
            dropped: Cell::new(0),
            reported_dropped: Cell::new(0),
        })
    }

    /// Push one event, dropping the oldest if the queue is at capacity.
    pub(crate) fn push(&self, event: InboundRelayEvent) {
        let mut q = self.queue.borrow_mut();
        if q.len() >= MAX_INBOUND_QUEUED {
            q.pop_front();
            self.dropped.set(self.dropped.get().saturating_add(1));
        }
        q.push_back(event);
    }

    /// Number of inbound drops not yet surfaced to the host, consuming them so
    /// the next call returns only newly-dropped frames. Called once per pump by
    /// [`drain_inbound`]; a non-zero return becomes a
    /// [`BrowserRuntimeEvent::RelayInboundDropped`] event.
    pub(crate) fn take_dropped_delta(&self) -> u64 {
        let total = self.dropped.get();
        let delta = total.saturating_sub(self.reported_dropped.get());
        self.reported_dropped.set(total);
        delta
    }
}

/// Result of one [`drain_inbound`] pass.
pub(crate) struct InboundDrainOutcome {
    /// Outbound messages produced by relay lifecycle calls and interceptors.
    pub(crate) outbound: Vec<OutboundMessage>,
    /// True when the drain budget was hit and events remain in the queue.
    pub(crate) yielded: bool,
    /// Additional host events emitted during drain (empty for the inbound drain
    /// itself; extended by the caller for spawn-related budget events).
    pub(crate) events: Vec<BrowserRuntimeEvent>,
}

/// Drain up to [`BROWSER_RELAY_DRAIN_BUDGET`] events from `queue`, applying
/// each to the `reducer` and running the appropriate substrate hooks.
///
/// The relay lifecycle calls (`handle_relay_connected_at`, `handle_relay_frame_at`,
/// `handle_relay_failed`, `handle_relay_closed`) mutate the reducer and return
/// outbound messages. Text frames additionally run the registered
/// `RelayTextInterceptor`s via `KernelReducer::run_relay_text_interceptors`.
/// `Connected` events additionally fan each `RelayConnectedHook` (the hook
/// receives a `CommandSender` clone to post follow-up commands).
pub(crate) fn drain_inbound(
    queue: &Rc<InboundQueue>,
    reducer: &mut KernelReducer,
    interceptors: &[Arc<dyn RelayTextInterceptor>],
    hooks: &[Arc<dyn RelayConnectedHook>],
    command_sender: &CommandSender,
) -> InboundDrainOutcome {
    let mut outbound: Vec<OutboundMessage> = Vec::new();
    let mut events: Vec<BrowserRuntimeEvent> = Vec::new();
    let mut applied = 0usize;

    // Surface any inbound frames the queue dropped on overflow since the last
    // pump (D6-honest — never a silent loss).
    let dropped = queue.take_dropped_delta();
    if dropped > 0 {
        events.push(BrowserRuntimeEvent::RelayInboundDropped { count: dropped });
    }

    loop {
        if applied >= BROWSER_RELAY_DRAIN_BUDGET {
            return InboundDrainOutcome {
                outbound,
                yielded: true,
                events,
            };
        }

        let event = match queue.queue.borrow_mut().pop_front() {
            Some(e) => e,
            None => break,
        };
        applied += 1;

        let now = Instant::now();
        match event {
            InboundRelayEvent::Connected {
                role,
                url,
                is_reconnect,
            } => {
                let msgs = reducer.handle_relay_connected_at(role, &url, is_reconnect, now);
                outbound.extend(msgs);
                // Fan connected hooks via the canonical substrate helper (single
                // source of truth for the D15 panic-contained loop). D8: hooks
                // spawn async work and return immediately.
                fan_relay_connected_hooks(hooks, &url, is_reconnect, command_sender);
            }
            InboundRelayEvent::Text { role, url, text } => {
                let msgs =
                    reducer.handle_relay_frame_at(role, &url, RelayFrame::Text(text.clone()), now);
                outbound.extend(msgs);
                // Run text interceptors (NIP-47 NWC etc.) via the composition seam.
                let extra = reducer.run_relay_text_interceptors(interceptors, &url, &text);
                outbound.extend(extra);
            }
            InboundRelayEvent::Binary { role, url, bytes } => {
                let msgs =
                    reducer.handle_relay_frame_at(role, &url, RelayFrame::Binary(bytes), now);
                outbound.extend(msgs);
            }
            InboundRelayEvent::Close { role, url, reason } => {
                // Close frame carries the optional reason; the returned outbound
                // is always empty but we collect it for consistency.
                let msgs =
                    reducer.handle_relay_frame_at(role, &url, RelayFrame::Close(reason), now);
                outbound.extend(msgs);
            }
            InboundRelayEvent::Closed { role, url } => {
                reducer.handle_relay_closed(role, &url);
            }
            InboundRelayEvent::Failed { role, url, error } => {
                reducer.handle_relay_failed(role, &url, error);
            }
        }
    }

    InboundDrainOutcome {
        outbound,
        yielded: false,
        events,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use nmp_core::actor::ActorMail;

    use super::*;

    fn test_queue() -> Rc<InboundQueue> {
        InboundQueue::new()
    }

    fn test_sender() -> (CommandSender, mpsc::Receiver<ActorMail>) {
        let (tx, rx) = mpsc::channel::<ActorMail>();
        (CommandSender::new(tx), rx)
    }

    #[test]
    fn empty_queue_drains_cleanly() {
        let q = test_queue();
        let mut reducer = KernelReducer::new();
        let (sender, _rx) = test_sender();
        let out = drain_inbound(&q, &mut reducer, &[], &[], &sender);
        assert!(out.outbound.is_empty());
        assert!(!out.yielded);
    }

    #[test]
    fn budget_enforced() {
        let q = test_queue();
        // Push budget+10 Failed events (cheap — reducer handles gracefully).
        for i in 0..BROWSER_RELAY_DRAIN_BUDGET + 10 {
            q.push(InboundRelayEvent::Failed {
                role: RelayRole::Content,
                url: format!("wss://relay{i}.example"),
                error: "test".to_string(),
            });
        }
        let mut reducer = KernelReducer::new();
        let (sender, _rx) = test_sender();

        let first = drain_inbound(&q, &mut reducer, &[], &[], &sender);
        assert!(first.yielded, "budget hit must set yielded");
        assert_eq!(q.queue.borrow().len(), 10, "remainder must stay in queue");

        let second = drain_inbound(&q, &mut reducer, &[], &[], &sender);
        assert!(!second.yielded);
        assert!(q.queue.borrow().is_empty());
    }

    #[test]
    fn drop_oldest_when_full() {
        let q = test_queue();
        // Fill to capacity with unique URLs.
        for i in 0..MAX_INBOUND_QUEUED {
            q.push(InboundRelayEvent::Failed {
                role: RelayRole::Content,
                url: format!("wss://r{i}.example"),
                error: "x".to_string(),
            });
        }
        assert_eq!(q.dropped.get(), 0);
        // One more push should drop the oldest.
        q.push(InboundRelayEvent::Failed {
            role: RelayRole::Content,
            url: "wss://new.example".to_string(),
            error: "y".to_string(),
        });
        assert_eq!(q.dropped.get(), 1);
        assert_eq!(q.queue.borrow().len(), MAX_INBOUND_QUEUED);
        // The new event must be at the back.
        let back = q.queue.borrow().back().map(|e| match e {
            InboundRelayEvent::Failed { url, .. } => url.clone(),
            _ => panic!("unexpected variant"),
        });
        assert_eq!(back.as_deref(), Some("wss://new.example"));
    }

    #[test]
    fn dropped_overflow_surfaces_inbound_dropped_event_once() {
        let q = test_queue();
        // Overflow the queue by 3 so `dropped == 3`.
        for i in 0..MAX_INBOUND_QUEUED + 3 {
            q.push(InboundRelayEvent::Failed {
                role: RelayRole::Content,
                url: format!("wss://r{i}.example"),
                error: "x".to_string(),
            });
        }
        assert_eq!(q.dropped.get(), 3);

        let mut reducer = KernelReducer::new();
        let (sender, _rx) = test_sender();

        // First drain hits the budget (queue is full) but must still surface the
        // 3 drops exactly once via a RelayInboundDropped event.
        let first = drain_inbound(&q, &mut reducer, &[], &[], &sender);
        let dropped_events: Vec<u64> = first
            .events
            .iter()
            .filter_map(|e| match e {
                BrowserRuntimeEvent::RelayInboundDropped { count } => Some(*count),
                _ => None,
            })
            .collect();
        assert_eq!(dropped_events, vec![3], "drops surfaced once with count 3");

        // Drain the rest; no new drops occurred, so no further dropped event.
        while drain_inbound(&q, &mut reducer, &[], &[], &sender).yielded {}
        let second = drain_inbound(&q, &mut reducer, &[], &[], &sender);
        assert!(
            !second
                .events
                .iter()
                .any(|e| matches!(e, BrowserRuntimeEvent::RelayInboundDropped { .. })),
            "already-surfaced drops must not be re-reported"
        );
    }

    #[test]
    fn connected_hook_receives_callback() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingHook(Arc<AtomicUsize>);
        impl RelayConnectedHook for CountingHook {
            fn on_relay_connected(&self, _url: &str, _is_reconnect: bool, _sender: CommandSender) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let hook: Arc<dyn RelayConnectedHook> = Arc::new(CountingHook(Arc::clone(&counter)));

        let q = test_queue();
        q.push(InboundRelayEvent::Connected {
            role: RelayRole::Content,
            url: "wss://relay.example".to_string(),
            is_reconnect: false,
        });
        let mut reducer = KernelReducer::new();
        let (sender, _rx) = test_sender();
        drain_inbound(&q, &mut reducer, &[], &[hook], &sender);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn interceptor_called_on_text_frame() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // A counting interceptor — proves the fold path is exercised.
        // Real interceptors filter by relay URL and return non-empty only for
        // frames they own.
        struct CountingInterceptor(Arc<AtomicUsize>);
        impl RelayTextInterceptor for CountingInterceptor {
            fn on_relay_text(
                &self,
                _kernel: &mut nmp_core::Kernel,
                _url: &str,
                _text: &str,
            ) -> Vec<OutboundMessage> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let ic: Arc<dyn RelayTextInterceptor> = Arc::new(CountingInterceptor(Arc::clone(&count)));

        let q = test_queue();
        q.push(InboundRelayEvent::Text {
            role: RelayRole::Content,
            url: "wss://relay.example".to_string(),
            text: "[\"EVENT\",\"sub1\",{}]".to_string(),
        });
        let mut reducer = KernelReducer::new();
        let (sender, _rx) = test_sender();
        let out = drain_inbound(&q, &mut reducer, &[ic], &[], &sender);
        assert!(!out.yielded);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "interceptor must be called once"
        );
    }
}
