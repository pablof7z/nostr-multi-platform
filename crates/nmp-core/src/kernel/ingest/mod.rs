//! Relay-frame parsing and the single accepted-event ingest chokepoint.
//!
//! ADR-0057 — `handle_message` → `handle_text` → `handle_event` does the
//! relay-only bookkeeping (relay counters, transport provenance, wire-sub
//! diagnostics, claim-expansion match) then hands the parsed event to the ONE
//! kind-agnostic, source-agnostic chokepoint [`Kernel::ingest_accepted_event`].
//! The chokepoint replaces the two hand-maintained per-kind ingest ladders
//! (the old relay `match event.kind` arms here and the deleted
//! `record_local_publish_intent` mirror in `local_publish_intent.rs`).
//!
//! The chokepoint separates three concerns into three layers:
//! - **Admission** = valid signature only (inside [`Kernel::verify_and_persist`]).
//! - **Delivery vs persistence** = gated by the store [`crate::store::InsertOutcome`].
//!   `verify_and_persist` does PERSISTENCE ONLY (sig-verify → `store.insert` →
//!   raw-tap → provenance → TTL) and returns the `(InsertOutcome, VerifiedEvent)`.
//!   The shared [`Kernel::project_accepted_event`] then fires BOTH the NIP-parser
//!   [`crate::substrate::EventIngestDispatcher`] dispatch AND the app-facing
//!   `ObservedProjectionSink` notify on the canonical accepted outcome
//!   (`Inserted | Replaced | Ephemeral`) — so an ephemeral reaches both the
//!   parsers and the app observers (ADR-0057 §1 latent-bug fix), and a
//!   `Duplicate` (incl. the relay echo of a local publish) is projection-silent
//!   (D4 single-fire / read-your-writes). `project_accepted_event` is the ONE
//!   post-store fan-out, called by both the live chokepoint
//!   ([`Kernel::ingest_accepted_event`]) and cache-serve replay
//!   ([`Kernel::feed_served_event`]), so the two paths cannot diverge.
//! - **Projection / relevance** = read-time only. The kernel-owned post-store
//!   read-cache is gated by the timeline author projection and by active
//!   generic interests. Profiles (kind:0, ADR-0057 PR 2) AND contacts (kind:3,
//!   ADR-0057 PR 3) moved out to registered `nmp_nip01::Kind0Parser` /
//!   `Kind3Parser` writing the capability-owned `ProfileCache` /
//!   `ContactsCache` — both detected via a before/after cache snapshot exactly
//!   like the mailbox / DM-relay observers. For contacts the kernel reacts to
//!   the ACTIVE account's transition by enqueueing a source recompile trigger;
//!   the reduced feed-source compiler owns author-set expansion and generic
//!   interest replacement.
//!   Substrate `MailboxCache` / `DmInboxRelayLookup` transitions are likewise
//!   detected kind-agnostically by bracketing the chokepoint with before/after
//!   snapshots (the kernel only knows "this author's mailbox / contacts
//!   changed", never "a kind:10002 / kind:3 arrived" —
//!   `docs/architecture/crate-boundaries.md` §0).
//!
//! Local publishes enter the chokepoint at `publish_engine.rs` with
//! `local://publish` provenance ([`IngestSource::LocalPublish`]); cache-replay
//! keeps its ADR-0045 path (`cache_serve/continuation.rs::feed_served_event`).
//!
//! ADR-0057 PR 3 is the full D0 finish-line: the kernel ingest path now names
//! ZERO NIP kind literals. kind:0 (profiles) moved in PR 2, kind:3 (contacts)
//! moved here, and feed acquisition flows through generic interests rather
//! than a hard-coded follow-feed branch.

mod accepted;
mod auth_handlers;
mod claimed_event_stamp; // ADR-0055 Rung 1 (F1) claimed-event stamp — sibling for size baseline
mod closed;
mod contacts;
// EOSE frame handling (incl. K3 Stage D1 coverage write), split for the LOC cap.
mod eose;
// `pub(in crate::kernel)`: shares `kernel_event_from_nostr` with the
// local-publish-intent path (read-your-writes fan-out, one construction site).
pub(in crate::kernel) mod helpers;
mod persistence;
mod projection;
mod relay_frame;
mod timeline;
mod timeline_order;

#[cfg(test)]
use super::Kernel;

/// ADR-0057 — provenance discriminator for the single accepted-event
/// chokepoint ([`Kernel::ingest_accepted_event`]).
///
/// The chokepoint is source-agnostic for persistence + delivery, but each
/// source carries a distinct provenance encoding that ADR-0057 preserves
/// verbatim (it does NOT introduce the typed `Provenance` enum — that is left
/// to the ADR-0045 amendment that names `Provenance::LocalStore`). `Relay`
/// additionally carries the wire `sub_id` so the timeline projection's
/// read-time relevance predicate can consult oneshot / firehose / open-interest
/// sub schemes. Cache-replay keeps its own ADR-0045 path
/// (`feed_served_event`) and does not flow through this enum.
pub(in crate::kernel) enum IngestSource<'a> {
    /// A relay-delivered event. Provenance = the delivering relay URL; the
    /// wire `sub_id` feeds the timeline read-time relevance predicate.
    Relay { relay_url: &'a str, sub_id: &'a str },
    /// A locally-published event accepted by the publish engine. Provenance =
    /// the literal `local://publish`; there is no wire sub.
    LocalPublish,
}

impl IngestSource<'_> {
    /// The store-insert provenance string for this source.
    fn provenance(&self) -> &str {
        match self {
            IngestSource::Relay { relay_url, .. } => relay_url,
            IngestSource::LocalPublish => "local://publish",
        }
    }

    /// The wire `sub_id` for relay deliveries; empty for local publishes
    /// (a local publish has no wire sub, and an empty id cannot collide with
    /// the prefix-matched sub schemes consulted by `should_store_event`).
    fn sub_id(&self) -> &str {
        match self {
            IngestSource::Relay { sub_id, .. } => sub_id,
            IngestSource::LocalPublish => "",
        }
    }

    /// Relay provenance for parser-owned read models. Only live relay ingest
    /// carries a relay URL; local publish and cache replay must not be
    /// represented as network sources.
    fn parser_source_relay_url(&self) -> Option<&str> {
        match self {
            IngestSource::Relay { relay_url, .. } => Some(relay_url),
            IngestSource::LocalPublish => None,
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
