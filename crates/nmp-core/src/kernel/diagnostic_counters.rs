//! Diagnostic drop/queue counters and claim/wire test accessors.
//!
//! A cohesive `impl Kernel` cluster of read-only diagnostic accessors that
//! surface internal counters for the snapshot/`Metrics` path and for tests:
//!
//! - **Queue-depth counters** — the `queue_depth` handle plumbing
//!   (`set_queue_depth_handle`, `take_queue_depth_handle_for_reset`,
//!   `actor_queue_depth`) consumed by `Metrics::actor_queue_depth`.
//! - **Backpressure drop counters** (#2767) — the `command_drops` and
//!   `relay_backlog_drops` handle plumbing, mirroring the `queue_depth`
//!   pattern exactly, consumed by `Metrics::command_drops` /
//!   `Metrics::relay_backlog_drops`.
//! - **Claim drop counters** — `claim_drops_total` (+ its `#[cfg(test)]`
//!   twin) and `lnurl_for_pubkey`, a cached-profile read used by the zap path.
//! - **`#[cfg(test)]` claim/wire-row accessors** — single-field reads over
//!   `profile_claims`, `event_claims`, `event_claim_requested`,
//!   `pending_event_claims`, and `wire.subs` used by the retention and
//!   claim-cap regression suites.
//!
//! These were split out of `kernel/mod.rs` verbatim (no behaviour change) to
//! keep that file under its file-size baseline; every method keeps its
//! original visibility so all call sites compile unchanged.

use super::Kernel;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

impl Kernel {
    /// G-S4 — install the actor's command-channel depth counter so the
    /// diagnostic snapshot surfaces it as `actor_queue_depth`. Idempotent:
    /// re-binding replaces the prior handle. `None`-on-construction is fine —
    /// the snapshot reports zero when unbound (tests, codegen). Called once by
    /// `run_actor_with_observers` immediately after the kernel is built.
    pub(crate) fn set_queue_depth_handle(&mut self, handle: Arc<AtomicU64>) {
        self.queue_depth = Some(handle);
    }

    /// G-S4 — extract the queue-depth counter handle before a `Reset` replaces
    /// the kernel. The counter is process-lifetime (shared with `NmpApp`'s
    /// `send_cmd`) so the Reset path moves it onto the fresh kernel via
    /// `set_queue_depth_handle`.
    pub(crate) fn take_queue_depth_handle_for_reset(&mut self) -> Option<Arc<AtomicU64>> {
        self.queue_depth.take()
    }

    /// G-S4 — current actor command-channel depth (`send_cmd` increments,
    /// the actor loop decrements per dequeued command). Returns 0 when the
    /// kernel was constructed outside the actor and no handle is bound.
    /// Saturates at `u32::MAX` because `Metrics::actor_queue_depth` is `u32`.
    pub(crate) fn actor_queue_depth(&self) -> u32 {
        let depth = self
            .queue_depth
            .as_ref()
            .map_or(0, |c| c.load(Ordering::Relaxed));
        depth.min(u64::from(u32::MAX)) as u32
    }

    /// #2767 — install the actor's command-drop counter handle so the
    /// diagnostic snapshot surfaces it as `command_drops`. Idempotent:
    /// re-binding replaces the prior handle. `None`-on-construction is fine —
    /// the snapshot reports zero when unbound (tests, codegen). Called once by
    /// `run_actor_with_observers` immediately after the kernel is built,
    /// mirroring `set_queue_depth_handle`.
    pub(crate) fn set_command_drops_handle(&mut self, handle: Arc<AtomicU64>) {
        self.command_drops = Some(handle);
    }

    /// #2767 — extract the command-drop counter handle before a `Reset`
    /// replaces the kernel. The counter is process-lifetime (shared with the
    /// host's `CommandSender::command_drops`) so the Reset path moves it onto
    /// the fresh kernel via `set_command_drops_handle`.
    pub(crate) fn take_command_drops_handle_for_reset(&mut self) -> Option<Arc<AtomicU64>> {
        self.command_drops.take()
    }

    /// #2767 — number of command sends shed because the bounded actor inbox
    /// was full. Returns 0 when the kernel was constructed outside the actor
    /// and no handle is bound. Monotonic for the process lifetime (no
    /// saturation — unlike `actor_queue_depth`, this is a counter, not a
    /// gauge).
    pub(crate) fn command_drops(&self) -> u64 {
        self.command_drops
            .as_ref()
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// #2767 — install the actor's relay-backlog-drop counter handle so the
    /// diagnostic snapshot surfaces it as `relay_backlog_drops`. Mirrors
    /// `set_queue_depth_handle` / `set_command_drops_handle`.
    pub(crate) fn set_relay_backlog_drops_handle(&mut self, handle: Arc<AtomicU64>) {
        self.relay_backlog_drops = Some(handle);
    }

    /// #2767 — extract the relay-backlog-drop counter handle before a `Reset`
    /// replaces the kernel. The counter is process-lifetime (shared with the
    /// actor's `MailScheduler::relay_backlog_drops`) so the Reset path moves it
    /// onto the fresh kernel via `set_relay_backlog_drops_handle`.
    pub(crate) fn take_relay_backlog_drops_handle_for_reset(&mut self) -> Option<Arc<AtomicU64>> {
        self.relay_backlog_drops.take()
    }

    /// #2767 — number of relay events shed because the actor's local relay
    /// backlog was at `RELAY_BACKLOG_CAP`. Returns 0 when the kernel was
    /// constructed outside the actor and no handle is bound. Monotonic for the
    /// process lifetime (no saturation).
    pub(crate) fn relay_backlog_drops(&self) -> u64 {
        self.relay_backlog_drops
            .as_ref()
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// T114b — number of `resolve_ref` requests dropped because a pubkey's
    /// `consumer_id` set hit `MAX_CLAIMS_PER_PUBKEY`. Read-only accessor; the
    /// counter is owned by the kernel and mutated only by `resolve_ref`.
    pub(crate) fn claim_drops_total(&self) -> u64 {
        self.claim_drops_total
    }

    #[cfg(test)]
    pub(crate) fn claim_drops_total_test(&self) -> u64 {
        self.claim_drops_total
    }

    /// Return the lightning address / LNURL from the author's cached kind:0
    /// profile, or `None` if the profile hasn't arrived yet or has no
    /// lightning address. Used by `ProtocolCommandContext::lnurl_for_pubkey`
    /// so `FetchLnurlInvoiceCommand` can resolve the destination without
    /// the shell having to carry or know about LNURL.
    pub(crate) fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String> {
        self.profile_lookup().profile(pubkey)?.lnurl
    }

    #[cfg(test)]
    pub(crate) fn profile_claims_len_for_test(&self, pubkey: &str) -> usize {
        self.profile_claims
            .get(pubkey)
            .map(|consumers| consumers.len())
            .unwrap_or(0)
    }

    /// Test-only: number of consumers currently holding an event ref
    /// on `primary_id`. Mirrors `profile_claims_len_for_test`.
    #[cfg(test)]
    pub(crate) fn event_claims_len_for_test(&self, primary_id: &str) -> usize {
        self.event_claims
            .get(primary_id)
            .map(|consumers| consumers.len())
            .unwrap_or(0)
    }

    /// Test-only: event ref requests dropped because a single
    /// `primary_id`'s consumer set hit `MAX_EVENT_CLAIMS_PER_KEY`.
    #[cfg(test)]
    pub(crate) fn event_claim_drops_total_for_test(&self) -> u64 {
        self.event_claim_drops_total
    }

    /// Test-only: `true` when `primary_id` is on the
    /// `event_claim_requested` set (an interest has been registered with
    /// the OneshotApi but not yet released by `complete_unknown_oneshot`).
    #[cfg(test)]
    pub(crate) fn event_claim_is_requested_for_test(&self, primary_id: &str) -> bool {
        self.event_claim_requested.contains(primary_id)
    }

    /// Test-only: number of `(uri, consumer_id)` pairs currently parked in
    /// `pending_event_claims` — i.e. claims that hit the cold-start
    /// `!can_send` branch and registered NO OneshotApi interest. A non-zero
    /// count means the claim is stuck waiting for `pending_event_claim_requests`
    /// to drain it once the send-gate flips.
    #[cfg(test)]
    pub(crate) fn pending_event_claims_len_for_test(&self) -> usize {
        self.pending_event_claims.len()
    }

    /// T133 retention-test accessor — total `wire_subs` row count, evicted or
    /// not. The whole point of T133 is that this stabilises rather than
    /// growing with close-cycle count.
    #[cfg(test)]
    pub(crate) fn wire_subs_len_for_test(&self) -> usize {
        self.wire.subs.len()
    }
}
