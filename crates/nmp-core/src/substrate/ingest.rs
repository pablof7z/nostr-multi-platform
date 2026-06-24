//! `IngestParser` — the read-path substrate seam.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §4.2. Step 1 of the
//! 12-step migration: pure additions, no kernel cut-over. NIP crates that
//! own a kind-specific cache (NIP-65 `MailboxCache` for kind:10002, NIP-17
//! `DmRelayCache` for kind:10050, etc.) register a parser through
//! [`EventIngestDispatcher`] so the kernel never pattern-matches NIP kind
//! numbers directly. Wiring into [`crate::Kernel`]'s ingest path happens at
//! step 6 (V-40) when kind:10050 ingest moves out of the kernel.
//!
//! ```ignore
//! // Shape future NIP crates will use once the kernel wires the dispatcher:
//! struct DmRelayListParser { cache: Arc<DmRelayCache> }
//! impl IngestParser for DmRelayListParser {
//!     fn parse(&self, evt: &VerifiedEvent) { self.cache.upsert_from(evt) }
//! }
//! dispatcher.register_kind(10050, Arc::new(DmRelayListParser::new(cache)));
//! ```

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::store::VerifiedEvent;

/// Per-NIP read-path projection hook.
///
/// Called by [`EventIngestDispatcher::dispatch`] for every ingested event
/// whose kind matches a registration. Implementations MUST be side-effect-free
/// against the kernel's own state — they write to their owning NIP crate's
/// caches/projections only (typically via interior mutability over an
/// `Arc<RwLock<…>>` the parser captures).
pub trait IngestParser: Send + Sync {
    fn parse(&self, evt: &VerifiedEvent);

    /// Timestamped ingest hook for parsers whose state changes need the
    /// actor/kernel-authored clock. Existing parsers can keep implementing
    /// `parse`; the dispatcher calls this method and the default delegates.
    fn parse_at(&self, evt: &VerifiedEvent, _now_secs: u64) {
        self.parse(evt);
    }
}

/// Registry of [`IngestParser`]s the kernel fans every ingested event to.
///
/// The dispatcher is a plain map; registration order is preserved within a
/// kind bucket. Range registrations are matched in registration order against
/// the event's kind. A parser registered for both a specific kind and a
/// range that includes it is called twice (this matches the trait's
/// "MUST be side-effect-free against kernel state" contract — duplicate
/// dispatch is the parser's problem, not the dispatcher's).
///
/// Per-kind entries are stored as `(slot_key, parser)` pairs where `slot_key`
/// is `None` for slot-less registrations (via [`Self::register_kind`]) and
/// `Some(key)` for lifecycle-managed registrations (via
/// [`Self::replace_kind_parser`]). This allows multiple lifecycle-managed
/// parsers to coexist on the same kind without silently evicting each other —
/// each owns exactly one named slot.
///
/// Range entries are stored as `(range, slot_key, parser)` triples where
/// `slot_key` is `None` for slot-less registrations (via
/// [`Self::register_range`]) and `Some(key)` for lifecycle-managed
/// registrations (via [`Self::replace_range_parser`]).
#[derive(Default)]
pub struct EventIngestDispatcher {
    by_kind: HashMap<u32, Vec<(Option<&'static str>, Arc<dyn IngestParser>)>>,
    by_range: Vec<(Range<u32>, Option<&'static str>, Arc<dyn IngestParser>)>,
}

impl EventIngestDispatcher {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a parser for `kind`. Multiple calls with the same kind
    /// accumulate parsers; all fire on each matching event. Use
    /// [`Self::replace_kind_parser`] for lifecycle-managed singleton seams.
    pub fn register_kind(&mut self, kind: u32, parser: Arc<dyn IngestParser>) {
        self.by_kind.entry(kind).or_default().push((None, parser));
    }

    /// Slot-keyed replace for `kind`: evict the prior parser registered
    /// under `slot_key` (if any) for `kind`, then install `parser` under
    /// the same slot. Parsers registered under **other** slot keys (or via
    /// [`Self::register_kind`] with no slot key) are **not** touched.
    ///
    /// Used by lifecycle-managed singleton seams (e.g. the NIP-17 DM inbox
    /// parser, which must be swapped to a fresh projection instance on account
    /// switch so accumulated in-memory messages are cleared). Returns the
    /// previous parser for `(kind, slot_key)`, if any, so callers can confirm
    /// whether a replacement actually happened (useful in tests).
    ///
    /// Multiple lifecycle-managed parsers can safely coexist on the same kind
    /// (e.g. the NIP-17 DM inbox under `"nip17.dm_inbox"` and Marmot under
    /// `"marmot"` both on kind:1059). Each slot key acts as an independent
    /// lifecycle scope — a re-registration for one slot never evicts the other.
    ///
    /// **Slot keys MUST be globally unique across crates.** A second component
    /// that reuses an existing slot name for the same kind silently evicts the
    /// peer's parser (the slot replace is unconditional within its slot). Choose
    /// a fully-qualified reverse-domain key (e.g. `"nip17.dm_inbox"`,
    /// `"marmot"`) that cannot collide with any other crate's registration.
    ///
    /// Distinct from [`Self::register_kind`] which appends with no slot key.
    pub fn replace_kind_parser(
        &mut self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        let bucket = self.by_kind.entry(kind).or_default();
        // Find and evict any prior entry with the same slot_key.
        let prev = if let Some(pos) = bucket.iter().position(|(key, _)| *key == Some(slot_key)) {
            Some(bucket.remove(pos).1)
        } else {
            None
        };
        bucket.push((Some(slot_key), parser));
        prev
    }

    /// Remove the parser registered under `slot_key` for `kind`, if any.
    ///
    /// Used by teardown paths that need to clear a lifecycle-managed slot
    /// without installing a replacement (e.g. Marmot sign-out without
    /// immediate re-register). Returns the evicted parser, or `None` when
    /// no parser was registered under that `(kind, slot_key)` pair.
    pub fn remove_kind_parser_slot(
        &mut self,
        kind: u32,
        slot_key: &'static str,
    ) -> Option<Arc<dyn IngestParser>> {
        let bucket = self.by_kind.get_mut(&kind)?;
        let pos = bucket.iter().position(|(key, _)| *key == Some(slot_key))?;
        let removed = bucket.remove(pos).1;
        if bucket.is_empty() {
            self.by_kind.remove(&kind);
        }
        Some(removed)
    }

    /// Append a slot-less parser for all events whose kind falls in `range`.
    /// Multiple calls accumulate parsers; all fire on each matching event.
    /// Use [`Self::replace_range_parser`] for lifecycle-managed singleton seams.
    pub fn register_range(&mut self, range: Range<u32>, parser: Arc<dyn IngestParser>) {
        self.by_range.push((range, None, parser));
    }

    /// Slot-keyed replace for a kind range: evict the prior range-parser
    /// registered under `slot_key` (if any), then install `parser` under the
    /// same slot. Only the entry with a matching `slot_key` is evicted; all
    /// other range registrations are untouched.
    ///
    /// Used by lifecycle-managed all-kinds parsers (e.g. a debug raw-event
    /// cache that needs to cover every kind). Returns the previous parser for
    /// `slot_key`, or `None` when this is the first registration for that
    /// slot. D6 — callers should hold the dispatcher write-lock.
    ///
    /// **Slot keys MUST be globally unique across crates.** Choose a
    /// fully-qualified reverse-domain key (e.g. `"chirp-tui.raw-cache"`) that
    /// cannot collide with any other crate's registration.
    pub fn replace_range_parser(
        &mut self,
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        let prev = if let Some(pos) = self
            .by_range
            .iter()
            .position(|(_, key, _)| *key == Some(slot_key))
        {
            Some(self.by_range.remove(pos).2)
        } else {
            None
        };
        self.by_range.push((range, Some(slot_key), parser));
        prev
    }

    /// Remove the range-parser registered under `slot_key`, if any. Returns
    /// the evicted parser, or `None` when no entry with that slot key exists.
    pub fn remove_range_parser_slot(
        &mut self,
        slot_key: &'static str,
    ) -> Option<Arc<dyn IngestParser>> {
        let pos = self
            .by_range
            .iter()
            .position(|(_, key, _)| *key == Some(slot_key))?;
        Some(self.by_range.remove(pos).2)
    }

    /// Return `true` when at least one parser is registered that would fire
    /// for `kind`. Used by the cache-serve gate to decide whether to run the
    /// `IngestParser` dispatch path for a served event without needing a full
    /// `VerifiedEvent` — avoids the `from_store_verified_unchecked` call and
    /// lock acquisition when the dispatcher is empty or has no match.
    ///
    /// Cheap read: walks only the by-kind bucket for `kind` (O(parsers-for-kind),
    /// typically 0–2) and the short range-vec (O(ranges), typically 0–3) without
    /// any allocation. Safe to call under the read lock.
    #[must_use]
    pub fn is_interested(&self, kind: u32) -> bool {
        self.by_kind.contains_key(&kind)
            || self
                .by_range
                .iter()
                .any(|(range, _, _)| range.contains(&kind))
    }

    /// Fan `evt` to every parser registered for its kind. Called by legacy
    /// tests and non-kernel harnesses; timestamp-aware production paths should
    /// call [`Self::dispatch_at`].
    pub fn dispatch(&self, evt: &VerifiedEvent) {
        self.dispatch_at(evt, 0);
    }

    /// Fan `evt` to every parser registered for its kind, carrying the
    /// actor/kernel-authored Unix timestamp for parsers that need replayable
    /// state-affecting time.
    pub fn dispatch_at(&self, evt: &VerifiedEvent, now_secs: u64) {
        let kind = evt.raw().kind;
        if let Some(parsers) = self.by_kind.get(&kind) {
            for (_, p) in parsers {
                p.parse_at(evt, now_secs);
            }
        }
        for (range, _, p) in &self.by_range {
            if range.contains(&kind) {
                p.parse_at(evt, now_secs);
            }
        }
    }

    /// Number of parser registrations (for diagnostics + tests). Counts each
    /// per-kind and per-range registration once, not per kind matched.
    /// Range entries registered via [`Self::replace_range_parser`] (slot-keyed)
    /// are counted the same as those registered via [`Self::register_range`].
    #[must_use]
    pub fn registration_count(&self) -> usize {
        self.by_kind.values().map(Vec::len).sum::<usize>() + self.by_range.len()
    }
}

#[cfg(test)]
#[path = "ingest/tests.rs"]
mod tests;
