//! `PendingSign` — an in-flight remote (NIP-46) sign operation parked on the
//! actor loop.
//!
//! Background: `sign_active` (`commands/identity.rs`) blocks the actor thread
//! for up to `REMOTE_SIGN_TIMEOUT` (5s) waiting on a NIP-46 broker via
//! `SignerOp::wait`. While it blocks, relay ingest, subscription management,
//! and UI emits all stall — a D8 violation (no polling / no blocking the
//! actor).
//!
//! The fix: the publish path signs through `sign_active_nonblocking`, which
//! hands back the raw `SignerOp` instead of blocking. A local signer's op is
//! `Ready` and resolves on the spot; a remote signer's op is `Pending` and is
//! stashed here. The actor's idle section then `poll()`s every parked
//! `PendingSign` once per loop tick — non-blocking `try_recv` — and publishes
//! the signed event the moment the broker turns the request around.
//!
//! `deadline` bounds the wait: a broker that never responds within
//! `PENDING_SIGN_TIMEOUT` has its `PendingSign` dropped and a toast surfaced
//! (D6 — the error becomes kernel state, the actor never wedges).

use crate::substrate::SignedEvent;
use nmp_signer_iface::SignerOp;
use std::time::{Duration, Instant};

/// Wall-clock budget for a parked remote-sign op. Mirrors the old blocking
/// `REMOTE_SIGN_TIMEOUT` (5s) — long enough for a fast / auto-approving
/// bunker, short enough that a crashed broker cannot strand the publish.
pub(crate) const PENDING_SIGN_TIMEOUT: Duration = Duration::from_secs(5);

/// A remote-sign operation parked on the actor loop, awaiting the broker's
/// kind:24133 response.
pub(crate) struct PendingSign {
    /// The in-flight signer op. `poll()`ed once per idle tick.
    pub op: SignerOp<SignedEvent>,
    /// The `p_tags` to forward to `Kernel::publish_signed` once the signed
    /// event lands. Empty for every current publish callsite (the publish
    /// engine resolves NIP-65 outbox relays itself); carried so the field
    /// can route p-tagged publishes without another signature change.
    pub p_tags: Vec<String>,
    /// Drop-dead time. Past this, the op is abandoned with a toast.
    pub deadline: Instant,
}

impl PendingSign {
    pub fn new(op: SignerOp<SignedEvent>, p_tags: Vec<String>) -> Self {
        Self {
            op,
            p_tags,
            deadline: Instant::now() + PENDING_SIGN_TIMEOUT,
        }
    }

    /// True once the op has overrun `PENDING_SIGN_TIMEOUT`.
    pub fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }
}
