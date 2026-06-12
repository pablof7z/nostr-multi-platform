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
            || self.by_range.iter().any(|(range, _, _)| range.contains(&kind))
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

    // ── dispatcher coverage tests ────────────────────────────────────────────

    /// (a) A slot-keyed range-parser AND a kind-slot parser both fire when a
    /// single event matches both registrations.
    ///
    /// Scenario: `"chirp-tui.raw-cache"` covers `0..u32::MAX`; `"marmot"`
    /// covers kind:1059 specifically. An event of kind:1059 must trigger both.
    #[test]
    fn slot_keyed_range_and_kind_slot_both_fire_on_one_event() {
        let mut d = EventIngestDispatcher::new();
        let range_p = CapturingParser::new();
        let kind_p = CapturingParser::new();

        d.replace_range_parser(0..u32::MAX, "chirp-tui.raw-cache", range_p.clone());
        d.replace_kind_parser(1059, "marmot", kind_p.clone());

        d.dispatch(&evt(1059));

        assert_eq!(range_p.kinds(), vec![1059], "range parser must fire");
        assert_eq!(kind_p.kinds(), vec![1059], "kind-slot parser must fire");
    }

    /// (b) Two overlapping distinct slot-keyed ranges both fire; replacing
    /// one slot does not touch the other (per-slot isolation).
    ///
    /// Scenario: slot `"crate-a"` covers `0..20_000`, slot `"crate-b"` covers
    /// `10_000..30_000`. Both cover kind:15000. After replacing `"crate-a"`,
    /// the new parser fires and the old does not; `"crate-b"` is unaffected.
    #[test]
    fn two_overlapping_distinct_slot_keyed_ranges_fire_independently() {
        let mut d = EventIngestDispatcher::new();
        let a1 = CapturingParser::new();
        let a2 = CapturingParser::new();
        let b = CapturingParser::new();

        d.replace_range_parser(0..20_000, "crate-a", a1.clone());
        d.replace_range_parser(10_000..30_000, "crate-b", b.clone());
        assert_eq!(d.registration_count(), 2);

        // Both fire on kind:15000 (falls in both ranges).
        d.dispatch(&evt(15_000));
        assert_eq!(a1.kinds(), vec![15_000], "slot-a fires before replace");
        assert_eq!(b.kinds(), vec![15_000], "slot-b fires before replace");

        // Replace slot-a — slot-b must survive untouched.
        let prev = d.replace_range_parser(0..20_000, "crate-a", a2.clone());
        assert!(prev.is_some(), "prior slot-a parser returned on replace");
        assert_eq!(d.registration_count(), 2, "still exactly 2 range registrations");

        d.dispatch(&evt(15_000));
        assert_eq!(a1.kinds(), vec![15_000], "old slot-a must NOT fire after replace");
        assert_eq!(a2.kinds(), vec![15_000], "new slot-a must fire");
        assert_eq!(b.kinds(), vec![15_000, 15_000], "slot-b must fire both times");
    }

    /// (c) An empty range (`5..5`) never fires, regardless of what event kind
    /// is dispatched. An empty `Range<u32>` contains no elements.
    #[test]
    fn empty_range_never_fires() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        // Register the empty range via the slot-keyed path (exercising that
        // codepath; register_range would work equally).
        d.replace_range_parser(5..5, "empty-slot", p.clone());
        assert_eq!(d.registration_count(), 1, "registration is recorded");

        // Dispatch several events — none should match the empty range.
        d.dispatch(&evt(4));
        d.dispatch(&evt(5)); // just past the empty range
        d.dispatch(&evt(6));
        d.dispatch(&evt(0));
        d.dispatch(&evt(u32::MAX - 1));

        assert!(
            p.kinds().is_empty(),
            "parser behind empty range must never fire"
        );
    }

    // ── is_interested tests ──────────────────────────────────────────────────

    /// `is_interested` returns true when a kind-specific parser is registered.
    #[test]
    fn is_interested_true_for_registered_kind() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.register_kind(1059, p.clone());
        assert!(d.is_interested(1059), "must be true for registered kind");
        assert!(!d.is_interested(1), "must be false for unregistered kind");
    }

    /// `is_interested` returns true when a range-registered parser covers the kind.
    #[test]
    fn is_interested_true_for_range_covered_kind() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.replace_range_parser(0..u32::MAX, "test.all-kinds", p.clone());
        assert!(d.is_interested(1), "all-kinds range must cover kind:1");
        assert!(d.is_interested(1059), "all-kinds range must cover kind:1059");
        assert!(d.is_interested(30023), "all-kinds range must cover kind:30023");
    }

    /// `is_interested` returns false for an empty dispatcher.
    #[test]
    fn is_interested_false_for_empty_dispatcher() {
        let d = EventIngestDispatcher::new();
        assert!(!d.is_interested(1), "empty dispatcher: is_interested must be false");
        assert!(!d.is_interested(1059), "empty dispatcher: is_interested must be false");
    }

    /// `is_interested` returns false after all parsers for a kind are removed.
    #[test]
    fn is_interested_false_after_kind_parser_removed() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        d.replace_kind_parser(1059, "test.slot", p.clone());
        assert!(d.is_interested(1059), "must be true before removal");
        d.remove_kind_parser_slot(1059, "test.slot");
        assert!(!d.is_interested(1059), "must be false after removal");
    }
}
