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
        let prev = if let Some(pos) = bucket
            .iter()
            .position(|(key, _)| *key == Some(slot_key))
        {
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
        let pos = bucket
            .iter()
            .position(|(key, _)| *key == Some(slot_key))?;
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

    /// Fan `evt` to every parser registered for its kind. Called by the
    /// kernel's ingest path; non-existent registrations are a fast no-op.
    pub fn dispatch(&self, evt: &VerifiedEvent) {
        let kind = evt.raw().kind;
        if let Some(parsers) = self.by_kind.get(&kind) {
            for (_, p) in parsers {
                p.parse(evt);
            }
        }
        for (range, _, p) in &self.by_range {
            if range.contains(&kind) {
                p.parse(evt);
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
mod tests {
    use super::*;
    use crate::store::{RawEvent, VerifiedEvent};
    use std::sync::Mutex;

    /// Captures every event the dispatcher hands it.
    struct CapturingParser {
        seen: Mutex<Vec<u32>>,
    }

    impl CapturingParser {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                seen: Mutex::new(Vec::new()),
            })
        }

        fn kinds(&self) -> Vec<u32> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl IngestParser for CapturingParser {
        fn parse(&self, evt: &VerifiedEvent) {
            self.seen.lock().unwrap().push(evt.raw().kind);
        }
    }

    fn evt(kind: u32) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: "00".repeat(32),
            pubkey: "11".repeat(32),
            created_at: 0,
            kind,
            tags: Vec::new(),
            content: String::new(),
            sig: "22".repeat(64),
        })
    }

    #[test]
    fn dispatch_calls_kind_parser() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.register_kind(10_050, p.clone());

        d.dispatch(&evt(10_050));
        d.dispatch(&evt(1)); // wrong kind — should not fire

        assert_eq!(p.kinds(), vec![10_050]);
    }

    #[test]
    fn dispatch_calls_range_parser() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        // NIP-51 list kinds.
        d.register_range(10_000..20_000, p.clone());

        d.dispatch(&evt(10_002));
        d.dispatch(&evt(19_999));
        d.dispatch(&evt(20_000)); // exclusive upper bound — should not fire

        assert_eq!(p.kinds(), vec![10_002, 19_999]);
    }

    #[test]
    fn multiple_parsers_for_one_kind_all_fire() {
        let mut d = EventIngestDispatcher::new();
        let a = CapturingParser::new();
        let b = CapturingParser::new();
        d.register_kind(1, a.clone());
        d.register_kind(1, b.clone());

        d.dispatch(&evt(1));

        assert_eq!(a.kinds(), vec![1]);
        assert_eq!(b.kinds(), vec![1]);
    }

    #[test]
    fn kind_and_range_overlap_each_fire() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.register_kind(10_002, p.clone());
        d.register_range(10_000..20_000, p.clone());

        d.dispatch(&evt(10_002));

        // Trait contract: dispatcher fans the event once per registration that
        // matched, not once per event. Parsers that register both ways own
        // the dedupe.
        assert_eq!(p.kinds(), vec![10_002, 10_002]);
    }

    #[test]
    fn empty_dispatcher_is_a_noop() {
        let d = EventIngestDispatcher::new();
        d.dispatch(&evt(1));
        assert_eq!(d.registration_count(), 0);
    }

    #[test]
    fn registration_count_tracks_both_axes() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.register_kind(1, p.clone());
        d.register_kind(1, p.clone());
        d.register_range(30_000..40_000, p.clone());
        assert_eq!(d.registration_count(), 3);
    }

    #[test]
    fn replace_kind_parser_swaps_single_slot() {
        let mut d = EventIngestDispatcher::new();
        let old = CapturingParser::new();
        let new_p = CapturingParser::new();

        // Register an old parser under slot "a" for kind 42.
        d.replace_kind_parser(42, "a", old.clone());
        assert_eq!(d.registration_count(), 1);

        // Replace: only the new parser survives under slot "a".
        let prev = d.replace_kind_parser(42, "a", new_p.clone());
        assert!(prev.is_some(), "old parser returned as previous");
        assert_eq!(d.registration_count(), 1, "exactly one parser remains after replace");

        d.dispatch(&evt(42));
        assert_eq!(old.kinds(), Vec::<u32>::new(), "old parser must NOT fire after replace");
        assert_eq!(new_p.kinds(), vec![42], "new parser must fire after replace");
    }

    #[test]
    fn replace_kind_parser_on_empty_slot_returns_none() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        let prev = d.replace_kind_parser(9999, "slot-a", p.clone());
        assert!(prev.is_none(), "replacing an absent slot returns None");
        assert_eq!(d.registration_count(), 1);
        d.dispatch(&evt(9999));
        assert_eq!(p.kinds(), vec![9999]);
    }

    #[test]
    fn two_slots_on_one_kind_coexist() {
        let mut d = EventIngestDispatcher::new();
        let p_a = CapturingParser::new();
        let p_b = CapturingParser::new();

        d.replace_kind_parser(1059, "nip17.dm_inbox", p_a.clone());
        d.replace_kind_parser(1059, "marmot", p_b.clone());
        assert_eq!(d.registration_count(), 2, "both slots registered");

        d.dispatch(&evt(1059));
        assert_eq!(p_a.kinds(), vec![1059], "slot-a parser must fire");
        assert_eq!(p_b.kinds(), vec![1059], "slot-b parser must fire");
    }

    #[test]
    fn per_slot_replacement_does_not_evict_peer_slot() {
        let mut d = EventIngestDispatcher::new();
        let p_a1 = CapturingParser::new();
        let p_a2 = CapturingParser::new();
        let p_b = CapturingParser::new();

        // Register both slots.
        d.replace_kind_parser(1059, "nip17.dm_inbox", p_a1.clone());
        d.replace_kind_parser(1059, "marmot", p_b.clone());
        assert_eq!(d.registration_count(), 2);

        // Re-register slot "a" (account switch) — slot "b" must survive.
        let evicted = d.replace_kind_parser(1059, "nip17.dm_inbox", p_a2.clone());
        assert!(evicted.is_some(), "prior slot-a parser returned");
        assert_eq!(d.registration_count(), 2, "slot count stays 2 after slot-a replace");

        d.dispatch(&evt(1059));
        assert_eq!(p_a1.kinds(), Vec::<u32>::new(), "old slot-a parser must NOT fire");
        assert_eq!(p_a2.kinds(), vec![1059], "new slot-a parser must fire");
        assert_eq!(p_b.kinds(), vec![1059], "slot-b parser must STILL fire after slot-a replace");
    }

    // ── range-slot tests ─────────────────────────────────────────────────────

    #[test]
    fn replace_range_parser_swaps_single_slot() {
        let mut d = EventIngestDispatcher::new();
        let old = CapturingParser::new();
        let new_p = CapturingParser::new();

        d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", old.clone());
        assert_eq!(d.registration_count(), 1);

        let prev = d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", new_p.clone());
        assert!(prev.is_some(), "old range parser returned as previous");
        assert_eq!(d.registration_count(), 1, "exactly one range registration after replace");

        d.dispatch(&evt(1));
        assert_eq!(old.kinds(), Vec::<u32>::new(), "evicted parser must NOT fire");
        assert_eq!(new_p.kinds(), vec![1], "new parser must fire");
    }

    #[test]
    fn replace_range_parser_on_empty_slot_returns_none() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        let prev = d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", p.clone());
        assert!(prev.is_none(), "first registration returns None");
        assert_eq!(d.registration_count(), 1);
        d.dispatch(&evt(42));
        assert_eq!(p.kinds(), vec![42]);
    }

    #[test]
    fn remove_range_parser_slot_evicts_and_silences() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();

        d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", p.clone());
        assert_eq!(d.registration_count(), 1);

        let evicted = d.remove_range_parser_slot("chirp-tui.raw-cache");
        assert!(evicted.is_some(), "returns evicted parser");
        assert_eq!(d.registration_count(), 0, "registration count drops to 0");

        d.dispatch(&evt(1));
        assert_eq!(p.kinds(), Vec::<u32>::new(), "evicted range parser must NOT fire");
    }

    #[test]
    fn remove_range_parser_slot_missing_returns_none() {
        let mut d = EventIngestDispatcher::new();
        assert!(d.remove_range_parser_slot("no-such-slot").is_none());
    }

    #[test]
    fn range_slot_does_not_evict_slot_less_range() {
        let mut d = EventIngestDispatcher::new();
        let slotless = CapturingParser::new();
        let slotted = CapturingParser::new();

        // A slot-less range registered via register_range must survive.
        d.register_range(0..u32::MAX, slotless.clone());
        d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", slotted.clone());
        assert_eq!(d.registration_count(), 2);

        d.dispatch(&evt(7));
        assert_eq!(slotless.kinds(), vec![7], "slot-less range must still fire");
        assert_eq!(slotted.kinds(), vec![7], "slot-keyed range must also fire");
    }

    #[test]
    fn range_all_kinds_fires_on_every_kind() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", p.clone());

        d.dispatch(&evt(0));
        d.dispatch(&evt(1));
        d.dispatch(&evt(10_050));
        d.dispatch(&evt(u32::MAX - 1));

        assert_eq!(p.kinds(), vec![0, 1, 10_050, u32::MAX - 1]);
    }
}
