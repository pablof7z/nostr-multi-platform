//! Inbound ingest seam — `IngestParser` registration (raw-tap PR-2).
//!
//! Migrated from `RawEventObserver` to [`nmp_core::substrate::IngestParser`]
//! in PR-2 of the raw-tap retirement ladder (rule A5). The kernel's
//! `EventIngestDispatcher` delivers every accepted inbound signed event of the
//! registered kinds to this parser; the parser reconstructs the verbatim
//! signed `nostr::Event` from [`nmp_store::VerifiedEvent::raw`] (same
//! pattern as `nmp-nip17/src/inbox.rs`, PR-1) and calls the existing
//! [`crate::projection::ops::ingest_signed_event_core`] unchanged.
//!
//! ## Why `IngestParser` and not `RawEventObserver`
//!
//! The `RawEventObserver` tap was the ONLY way MDK (which requires a signed
//! `nostr::Event` with `sig` present for gift-wrap unwrapping and kind:445
//! decryption) could ride the kernel ingest path — the lossy
//! `ObservedProjectionSink` strips the signature. PR-1 of the raw-tap ladder
//! proved the pattern: `DmInboxProjection` serialises the `VerifiedEvent`'s
//! `RawEvent` to JSON and parses a `nostr::Event` from it, recovering the
//! `sig` field that MDK needs. This module applies the same technique to
//! Marmot's five registered kinds.
//!
//! ## Slot key
//!
//! Registered under the `"marmot"` slot key via
//! `EventIngestDispatcher::replace_kind_parser`. The NIP-17 DM inbox uses the
//! `"nip17.dm_inbox"` slot, so both parsers coexist safely on kind:1059
//! without evicting each other. See `substrate/ingest.rs` §Slot-keyed replace.
//!
//! ## Threading & D6
//!
//! `parse` fires on the kernel actor / ingest thread, between relay frames,
//! while host snapshot / dispatch calls may read the same projection. We take
//! the projection's inner `Mutex` exactly as `on_kernel_event` already does
//! (low contention; host calls are expected to serialize). The work is bounded
//! — local MDK + SQLite, never network. Every failure (poisoned mutex, parse
//! error, duplicate / malformed event, `MarmotService` error) is a **silent
//! no-op**: the parser discards the `ingest_signed_event_core` `Result` and
//! never panics across the actor boundary (D6).

use std::sync::Arc;

use nmp_core::substrate::IngestParser;
use nmp_store::VerifiedEvent;
use nostr::{Event, JsonUtil};

use crate::projection::ops::ingest_signed_event_core;
use crate::projection::state::MarmotProjection;

/// `IngestParser` that bridges the substrate dispatcher into the Marmot
/// projection. Holds an `Arc<MarmotProjection>`. The dispatcher owns this
/// parser as an `Arc<dyn IngestParser>` until teardown via the slot-keyed
/// replace seam.
pub struct MarmotIngestParser {
    projection: Arc<MarmotProjection>,
}

impl MarmotIngestParser {
    #[must_use]
    pub fn new(projection: Arc<MarmotProjection>) -> Self {
        Self { projection }
    }
}

impl IngestParser for MarmotIngestParser {
    /// One accepted inbound signed event from the substrate dispatcher.
    ///
    /// We reconstruct the verbatim signed NIP-01 JSON from the `RawEvent`
    /// (which includes the `sig` field — MDK requires it for gift-wrap
    /// unwrapping and kind:445 decryption) via `serde_json::to_string`, then
    /// parse a `nostr::Event` from the resulting JSON (same pattern as
    /// `nmp-nip17::inbox::DmInboxProjection::parse`, PR-1).
    ///
    /// All failures are silent (D6); the projection mutation is the
    /// load-bearing effect a later snapshot refresh surfaces.
    fn parse(&self, evt: &VerifiedEvent) {
        self.parse_at(evt, 0);
    }

    fn parse_at(&self, evt: &VerifiedEvent, now_secs: u64) {
        // Serialize the `RawEvent` (which includes `sig`) back to JSON.
        // `RawEvent` derives `Serialize` with the exact NIP-01 field order,
        // so this is lossless — no field is dropped.
        let Ok(json) = serde_json::to_string(evt.raw()) else {
            return; // Serialization failure → silent no-op (D6).
        };
        // Parse the signed event off the JSON string. This recovers the
        // `sig` field that `VerifiedEvent::raw()` carries via `RawEvent::sig`.
        let Ok(event) = Event::from_json(&json) else {
            return; // Parse failure → silent no-op (D6).
        };
        // Lock the projection's inner state. Poisoned mutex → silent no-op.
        let _ = self.projection.with_inner(|h| {
            // Discard the Result: the parser has no caller to surface a
            // duplicate / unsupported-kind / decrypt error to (D6). The
            // projection side-effects (pending-welcome row, relay cache,
            // MDK state) are what the next snapshot reflects.
            let _ = ingest_signed_event_core(h, &event, now_secs);
        });
    }
}
