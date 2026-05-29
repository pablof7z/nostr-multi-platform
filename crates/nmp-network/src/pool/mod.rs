//! Push-model [`Pool`] API — `docs/architecture/crate-boundaries.md` §3.8,
//! step 8 phase B.
//!
//! ## What this is
//!
//! The kernel actor's interface to `nmp-network`. Callers:
//!
//! 1. Construct one [`Pool`] at startup, handing the constructor an
//!    `events: Sender<PoolEvent>` channel the actor loop will recv on.
//! 2. Call [`Pool::ensure_open`] to spin up (or look up) a worker for
//!    each URL the router resolves. The pool returns a generational
//!    [`RelayHandle`] the caller stores alongside its per-URL state.
//! 3. Call [`Pool::send`] with the handle and a [`WireFrame`] to push
//!    one frame at one specific (URL, generation). **There is no
//!    "send to all" method** — the kernel actor iterates its
//!    `RoutedRelaySet` and issues one constrained send per URL. This
//!    is the structural answer to NDK issue #175.
//! 4. `recv` [`PoolEvent`]s on the channel: `Opened` / `Frame` /
//!    `Closed` / `Failed` / `Health`. Each event carries its handle
//!    and the worker's `generation`; the pool's translator thread
//!    drops any inbound event whose generation no longer matches the
//!    current slot generation, so the actor never observes a frame
//!    targeted at a URL it has since reconnected.
//!
//! ## Structural invariants
//!
//! - **No "send to all".** The `Pool` exposes no method that fans a
//!   single frame across every connected relay. The
//!   `RoutedRelaySet`-iteration loop lives in the kernel actor where
//!   the routing decision is observable.
//! - **Generational handles.** [`RelayHandle`] is `(slot, generation)`.
//!   A stale handle from before a reconnect is structurally rejected
//!   by [`Pool::send`] / [`Pool::health`] / [`Pool::close`] — it
//!   cannot silently target the wrong generation of the same URL.
//! - **Cheap to clone.** `Pool` is `Arc<Mutex<PoolInner>>` inside; the
//!   kernel actor can clone a handle into protocol commands without
//!   thinking about lifetimes.
//!
//! ## What this PR does NOT do
//!
//! - **Phase C** (`nmp-wasm::BrowserRelayDriver` move into this crate)
//!   is a separate PR. Today's `Pool` is native-only (gated by the
//!   `native` Cargo feature); the wasm driver still lives in
//!   `nmp-wasm`.
//! - **Phase D** (signer-broker migration onto `Pool`) is shipped.
//!   `nmp-signer-broker::relay_client::PoolRelayClient` rides `Pool`;
//!   the duplicate mio/tungstenite client is gone (V-13 Stage 2 dedupe).
//! - **Phase E** (NIP-42 wire/FSM split — surface `AUTH` as a distinct
//!   [`RelayFrame::Auth`] variant) shipped: the translator's
//!   `tungstenite::Message::Text → RelayFrame` step pre-classifies the
//!   NIP-42 frame shape via `nmp-nip42-types::parse_auth_frame`.
//!   `nmp-network` still does NOT name `AuthGate`, kind:22242, or the
//!   `RelayAuthState` enum.
//! - **Actor migration** off the legacy `RelayEvent`/`RelayCommand`
//!   API. Today's actor (`crates/nmp-core/src/actor/relay_mgmt.rs`)
//!   still drives `spawn_relay_worker` directly. The legacy entry
//!   points stay re-exported alongside `Pool` so the actor compiles
//!   unchanged; the migration is the next PR in this lane. See
//!   `WIP.md` for the follow-up.
//!
//! ## Why phase B ships as additive
//!
//! The crate-boundary spec §3.8 says **Pool is the only caller above
//! `nmp-network`** once migration completes. But replacing the actor's
//! ~38 [`crate::relay_worker::RelayEvent`] / `RelayCommand` usages in
//! one PR pushes well past the 1500-LOC / 50-callsite STOP boundary the
//! agent task spec lays down. So phase B ships the `Pool` types +
//! lifecycle + translator thread, validated by unit tests in this
//! crate, and the actor migration follows in its own PR — three PRs
//! that each compile independently against master.

mod inner;
mod types;

#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use crate::role::RelayRole;

pub use types::{
    ClosedReason, HealthState, PoolConfig, PoolEvent, PoolSnapshot, PoolSnapshotRow, RelayFrame,
    RelayHandle, RelayHealth, RelayUrl, TransportError, WireFrame,
};
// V-58 — re-export so callers (nmp-core actor) can name the hint type through
// the pool's public surface without reaching into relay_protocol directly.
pub use crate::relay_protocol::BackoffClass;

use inner::{wire_frame_to_command, PoolInner};

/// The push-model relay-connection pool. Cheap to clone (`Arc` inside).
///
/// See module documentation for the full surface contract.
#[derive(Clone)]
pub struct Pool {
    inner: Arc<Mutex<PoolInner>>,
}

impl Pool {
    /// Construct a new pool. The translator thread is spawned eagerly;
    /// `events` is the channel `PoolEvent`s land on until the receiver
    /// is dropped.
    #[must_use]
    pub fn new(cfg: PoolConfig, events: std::sync::mpsc::Sender<PoolEvent>) -> Self {
        Self {
            inner: PoolInner::new(cfg, events),
        }
    }

    /// Ensure a worker is dialing/connected for `url`. Idempotent: a
    /// repeat call for the same canonical URL returns the existing
    /// handle (with its current generation). If the URL was previously
    /// closed (via [`Self::close`]), the slot is reopened with a bumped
    /// generation — the prior handle becomes stale.
    ///
    /// The diagnostic lane defaults to [`PoolConfig::default_role`];
    /// callers that need a per-URL role should use
    /// [`Self::ensure_open_with_role`].
    pub fn ensure_open(&self, url: &RelayUrl) -> RelayHandle {
        let role = self
            .inner
            .lock()
            .map(|g| g.config.default_role)
            .unwrap_or(RelayRole::Content);
        self.ensure_open_with_role(url, role)
    }

    /// Ensure-open with an explicit diagnostic role for the worker.
    /// Mirrors today's actor `ensure_relay_worker(role, url)` signature
    /// so the legacy actor path can swap onto `Pool` without losing
    /// the per-lane health-row attribution.
    pub fn ensure_open_with_role(&self, url: &RelayUrl, role: RelayRole) -> RelayHandle {
        let Ok(mut guard) = self.inner.lock() else {
            return RelayHandle {
                slot: u32::MAX,
                generation: 0,
            };
        };
        guard.ensure_open(url, role)
    }

    /// Close the slot for `h`. No-op if the handle is stale or the
    /// slot was already closed. Subsequent
    /// [`Self::ensure_open`] for the same URL reopens with a bumped
    /// generation.
    pub fn close(&self, h: RelayHandle) -> bool {
        match self.inner.lock() {
            Ok(mut guard) => guard.close(h),
            Err(_) => false,
        }
    }

    /// Tear down every worker. Subsequent
    /// [`Self::ensure_open`] calls return a sentinel handle (slot
    /// `u32::MAX`); subsequent `send` calls are structural no-ops.
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.shutdown();
        }
    }

    /// Push one frame at one specific (URL, generation). Stale handle
    /// → silent no-op (returns `false`); the structural invariant
    /// is that the caller cannot accidentally target the wrong
    /// generation of the same URL.
    ///
    /// Returns `true` iff the frame was handed to the worker
    /// channel — not iff it has been written to the socket. The
    /// worker may still be `Connecting`; the frame is queued in
    /// [`crate::relay_worker`]'s `pending` buffer until the socket
    /// opens (T130: frames sent before connect arrive after open).
    pub fn send(&self, h: RelayHandle, frame: WireFrame) -> bool {
        let Some(command) = wire_frame_to_command(frame) else {
            return false;
        };
        let Ok(guard) = self.inner.lock() else {
            return false;
        };
        let Some(tx) = guard.command_tx_for(h) else {
            return false;
        };
        drop(guard);
        tx.send(command).is_ok()
    }

    /// V-58 — deliver a one-shot [`BackoffClass`] hint to the worker for
    /// handle `h`. A `RateLimited` hint causes the **next** socket reconnect
    /// for this URL to use [`crate::relay_protocol::RELAY_RECONNECT_DELAY_RATE_LIMITED`]
    /// (60 s + jitter) instead of the normal exponential curve.
    ///
    /// The hint is one-shot: the worker consumes and clears it on the next
    /// disconnect so subsequent reconnects resume the normal schedule.
    ///
    /// Returns `true` iff the hint was successfully enqueued. Stale handles
    /// and closed slots return `false` (same structural rejection as `send`).
    pub fn set_backoff_hint(&self, h: RelayHandle, class: BackoffClass) -> bool {
        match self.inner.lock() {
            Ok(guard) => guard.set_backoff_hint_for(h, class),
            Err(_) => false,
        }
    }

    /// Per-handle health snapshot. Stale handle → `None`.
    #[must_use]
    pub fn health(&self, h: RelayHandle) -> Option<RelayHealth> {
        self.inner.lock().ok().and_then(|g| g.health_for(h))
    }

    /// Pool-wide diagnostic snapshot. Used by the FFI status
    /// projection.
    #[must_use]
    pub fn snapshot(&self) -> PoolSnapshot {
        self.inner
            .lock()
            .map(|g| g.snapshot())
            .unwrap_or_default()
    }
}
