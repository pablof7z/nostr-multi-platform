//! Publish-engine runtime hooks owned by the kernel.
//!
//! Split from `publish_engine.rs` so the publish owner stays under the
//! hand-authored file-size ceiling while keeping relay availability, retry
//! ticks, and boot resume beside the engine wiring.

use crate::relay::OutboundMessage;
use nmp_network::role::RelayRole;

use super::super::publish_engine_wire::describe_engine_error;
use super::super::Kernel;

impl Kernel {
    /// Wall-clock variant for the live ingest seam. Tests use the
    /// `tick_publish_engine(now_ms)` injection point directly.
    pub(crate) fn tick_publish_engine_for_now(&mut self) -> Vec<OutboundMessage> {
        self.tick_publish_engine(self.now_ms())
    }

    /// Earliest publish-engine deadline expressed as a delay from now. This is
    /// the kernel-owned scheduling predicate the wasm runtime uses for bounded
    /// deadline timers; no deadline means no publish work should wake the
    /// runtime.
    #[must_use]
    pub(crate) fn next_publish_engine_deadline_delay_ms(&self) -> Option<u32> {
        let now_ms = self.now_ms();
        self.publish_engine
            .next_deadline_ms(now_ms)
            .map(|deadline| deadline.saturating_sub(now_ms).min(u64::from(u32::MAX)) as u32)
    }

    /// Drive the publish engine's wall-clock retries. Called from
    /// `kernel::ingest::handle_message` opportunistically (every inbound
    /// relay text frame ticks the engine, so the live path bounds retry latency
    /// by inbound traffic). Tests inject `now_ms` directly.
    pub(crate) fn tick_publish_engine(&mut self, now_ms: u64) -> Vec<OutboundMessage> {
        let engine_rev_before = self.publish_engine.snapshot().rev;
        self.publish_engine.tick(now_ms);
        // T128: `tick` -> `dispatch_pending` -> synchronous `dispatch_due` may
        // return an OK / failure ack inline. Drain any settled verdicts so
        // the queue entry flips to `"ok"` / `"failed"` on the same tick.
        self.drain_engine_terminals_into_ledger();
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        drained.into_iter().map(content_message).collect()
    }

    /// Notify the publish engine that a relay socket is unavailable. Any
    /// in-flight publish for that relay is moved back to durable Pending by
    /// the engine; the actor will retry when a fresh Connected event arrives.
    pub(crate) fn mark_publish_relay_unavailable(&mut self, relay_url: &str) {
        let now_ms = self.now_ms();
        let engine_rev_before = self.publish_engine.snapshot().rev;
        if let Err(err) = self
            .publish_engine
            .mark_relay_unavailable(relay_url, now_ms)
        {
            self.publish_engine
                .record_engine_error(&err, &String::new(), "", now_ms);
            let (toast, _, category) =
                describe_engine_error(&err, self.publish_engine.resolver_composed());
            self.set_error_toast_with_category(toast, category);
        }
        self.bump_publish_if_engine_view_changed(engine_rev_before);
    }

    /// Notify the publish engine that a relay socket is available. Pending
    /// publishes targeting this relay are dispatched through the normal actor
    /// outbound path, which also keeps relay-worker connection ownership in
    /// one place.
    pub(crate) fn mark_publish_relay_available(&mut self, relay_url: &str) -> Vec<OutboundMessage> {
        let now_ms = self.now_ms();
        let engine_rev_before = self.publish_engine.snapshot().rev;
        if let Err(err) = self.publish_engine.mark_relay_available(relay_url, now_ms) {
            self.publish_engine
                .record_engine_error(&err, &String::new(), "", now_ms);
            let (toast, _, category) =
                describe_engine_error(&err, self.publish_engine.resolver_composed());
            self.set_error_toast_with_category(toast, category);
            self.bump_publish_if_engine_view_changed(engine_rev_before);
            return Vec::new();
        }
        self.drain_engine_terminals_into_ledger();
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        drained.into_iter().map(content_message).collect()
    }

    /// Resume any pending publishes that survived a kernel restart. Called by
    /// the actor (T127, `actor/dispatch.rs::Start`) once per `Start` command,
    /// and by integration tests directly. Returns any outbound frames the
    /// engine emitted as it brought live relays back into `InFlight` from a
    /// `Pending` / due-`RelayError` state.
    pub(crate) fn resume_publish_engine(&mut self) -> Vec<OutboundMessage> {
        let now_ms = self.now_ms();
        let engine_rev_before = self.publish_engine.snapshot().rev;
        if let Err(err) = self.publish_engine.resume_from_store(now_ms) {
            // D6: durable-resume failure surfaces as a snapshot failure row
            // plus a toast; never a panic, never a `Result` across FFI.
            self.publish_engine
                .record_engine_error(&err, &String::new(), "", now_ms);
            let (toast, _, category) =
                describe_engine_error(&err, self.publish_engine.resolver_composed());
            self.set_error_toast_with_category(toast, category);
            self.bump_publish_if_engine_view_changed(engine_rev_before);
            return Vec::new();
        }
        // T128: resume can complete a publish synchronously when the
        // dispatcher returns OK acks for a re-dispatched retry. Drain
        // terminal verdicts before returning so the boot-resume path
        // surfaces the final status on the same actor frame. (The queue
        // entry for resumed publishes was pushed by the original kernel
        // process; on a fresh kernel B in tests there is no entry to flip,
        // so `set_publish_entry_terminal` is a no-op in that case.)
        self.drain_engine_terminals_into_ledger();
        let drained = self.publish_dispatcher.drain();
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        drained.into_iter().map(content_message).collect()
    }

    /// Test/diagnostic accessor for the publish engine's snapshot. Exposed
    /// crate-private so integration tests can assert on `recent_ok` /
    /// `recent_errors` after driving the kernel through `publish_signed` +
    /// `handle_publish_ok`. The FFI-side projection bridge will read this
    /// through `make_update` in a follow-up wiring task.
    // `allow(dead_code)`: called from kernel integration tests today; the
    // production FFI projection bridge wires this in a follow-up task.
    #[allow(dead_code)]
    pub(crate) fn publish_status_snapshot(&self) -> &crate::publish::PublishStatusSnapshot {
        self.publish_engine.snapshot()
    }
}

fn content_message((relay_url, text): (String, String)) -> OutboundMessage {
    OutboundMessage {
        role: RelayRole::Content,
        relay_url,
        text,
    }
}
