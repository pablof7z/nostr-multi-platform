//! INBOUND ingest seam — CLOSED.
//!
//! The kernel's lossy `KernelEventObserver` strips the signature, so MDK
//! (which requires a *signed* `nostr::Event` to unwrap a gift-wrap or
//! decrypt a kind:445 message) could not be fed from it — historically the
//! Chirp layer relied on a `{"op":"ingest_signed_event"}` dispatch op
//! called from a Swift relay path that never existed (see
//! [`crate::projection::state`]'s seam #2). The kernel now also exposes a
//! parallel **raw signed-event tap** (`RawEventObserver`) that delivers the
//! verbatim flat NIP-01 object *including `sig`* after the kernel's own
//! Schnorr + id gate. This module registers that tap and drives every
//! accepted inbound kind:1059 / kind:445 into the SAME
//! [`crate::projection::ops::ingest_signed_event_core`] the back-compat
//! dispatch op uses — so welcomes / messages received from relays surface
//! in the next `nmp_app_chirp_marmot_snapshot` with zero Swift change.
//!
//! ## Linkage
//!
//! Registered through the in-process Rust-trait API
//! ([`nmp_core::NmpApp::register_raw_event_observer`]) — the same shape as
//! the existing `KernelEventObserver` registration in the Chirp FFI shell,
//! no C-ABI hop. The kernel owns the
//! `Arc<dyn RawEventObserver>`; the tap holds an `Arc<MarmotProjection>`.
//! No reference cycle: `MarmotHandle` separately owns the projection and
//! the returned `RawEventObserverId`; nothing in the projection points
//! back at the tap. `nmp_app_chirp_marmot_unregister` drops the kernel's
//! `Arc` (via `unregister_raw_event_observer`), releasing the tap's
//! projection reference.
//!
//! ## Threading & D6
//!
//! `on_raw_event` fires on the kernel actor / ingest thread, between relay
//! frames, while the FFI snapshot / dispatch run on the serialized Swift
//! bridge thread. We take the projection's inner `Mutex` exactly as
//! `on_kernel_event` already does (low contention; the bridge serializes
//! its calls). The work is bounded — local MDK + SQLite, never network.
//! Every failure (poisoned mutex, parse error, duplicate / malformed
//! event, `MarmotService` error) is a **silent no-op**: the tap discards
//! the `ingest_signed_event_core` `Result` and never panics across the
//! actor / FFI boundary (D6; matches the `RawEventObserver` rustdoc
//! "panicking observer are silent no-ops").

use std::sync::Arc;

use nmp_core::{KindFilter, RawEventObserver};
use nostr::{Event, JsonUtil};

use crate::projection::ops::ingest_signed_event_core;
use crate::projection::state::MarmotProjection;

/// Kinds the inbound tap subscribes to:
/// - kind:443 / kind:30443 — key-packages, so the `kp_cache` in
///   `MarmotService` is populated when peers' events arrive;
/// - kind:1059 — gift-wrap welcome;
/// - kind:445 — group message / commit / proposal;
/// - kind:444 — welcome rumor, admitted defensively (the wire welcome is
///   the kind:1059 gift-wrap, but accepting 444 costs nothing — the
///   shared core silently skips it).
pub(crate) const TAP_KINDS: [u32; 5] = [443, 444, 445, 1059, 30443];

/// Raw signed-event observer that bridges the kernel tap into the Marmot
/// projection. Holds an `Arc<MarmotProjection>` (the same projection the
/// owning `MarmotHandle` retains); the kernel owns this observer as an
/// `Arc<dyn RawEventObserver>` until `unregister_raw_event_observer`.
pub struct MarmotIngestTap {
    projection: Arc<MarmotProjection>,
}

impl MarmotIngestTap {
    pub fn new(projection: Arc<MarmotProjection>) -> Self {
        Self { projection }
    }

    /// The kind filter to register this tap with.
    pub fn kind_filter() -> KindFilter {
        KindFilter::from_kinds(TAP_KINDS)
    }
}

impl RawEventObserver for MarmotIngestTap {
    /// One accepted inbound signed event (verbatim flat NIP-01 JSON,
    /// `sig` included). `json` is borrowed for the call only — we parse
    /// (which copies every field we keep) before doing anything that could
    /// defer, satisfying the borrowed-payload contract. All failures are
    /// silent (D6); the projection mutation is the load-bearing effect a
    /// later snapshot refresh surfaces.
    fn on_raw_event(&self, _kind: u32, json: &str) {
        // Parse off the borrowed buffer immediately (owns its bytes after).
        let Ok(event) = Event::from_json(json) else {
            return; // malformed → silent no-op (D6).
        };
        // Lock the projection's inner state the same way the FFI ops /
        // `on_kernel_event` do. Poisoned mutex → silent no-op.
        let _ = self.projection.with_inner(|h| {
            // Discard the Result: the tap has no caller to surface a
            // duplicate / unsupported-kind / decrypt error to (D6). The
            // projection side-effects (pending-welcome row, relay cache,
            // MDK state) are what the next snapshot reflects.
            let _ = ingest_signed_event_core(h, &event);
        });
    }
}
