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
#[derive(Default)]
pub struct EventIngestDispatcher {
    by_kind: HashMap<u32, Vec<Arc<dyn IngestParser>>>,
    by_range: Vec<(Range<u32>, Arc<dyn IngestParser>)>,
}

impl EventIngestDispatcher {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_kind(&mut self, kind: u32, parser: Arc<dyn IngestParser>) {
        self.by_kind.entry(kind).or_default().push(parser);
    }

    /// Replace **all** parsers registered for `kind` with a single new one.
    ///
    /// Used by lifecycle-managed singleton seams (e.g. the NIP-17 DM inbox
    /// parser, which must be swapped to a fresh projection instance on account
    /// switch so accumulated in-memory messages are cleared). Returns the
    /// previous parsers for that kind, if any, so callers can confirm whether
    /// a replacement actually happened (useful in tests).
    ///
    /// Distinct from [`Self::register_kind`] which appends; this method is a
    /// single-slot write — only one parser for `kind` survives the call.
    ///
    /// ## PR-2 forward-compatibility warning
    ///
    /// This method uses **replace-ALL-for-kind** semantics: every existing parser
    /// for `kind` is evicted and replaced by the single `parser` argument. A
    /// second lifecycle-managed parser for the same kind (e.g. Marmot's kind-1059
    /// parser arriving in raw-tap PR-2) MUST NOT use this method — calling it a
    /// second time would silently evict the first parser, breaking the peer
    /// registration. PR-2 converts this seam to slot-keyed replace (each caller
    /// owns a named slot; only that slot is swapped). Until that migration lands,
    /// each kind MUST have at most one `replace_kind_parser` caller.
    pub fn replace_kind_parser(
        &mut self,
        kind: u32,
        parser: Arc<dyn IngestParser>,
    ) -> Vec<Arc<dyn IngestParser>> {
        let prev = self.by_kind.remove(&kind).unwrap_or_default();
        self.by_kind.insert(kind, vec![parser]);
        prev
    }

    pub fn register_range(&mut self, range: Range<u32>, parser: Arc<dyn IngestParser>) {
        self.by_range.push((range, parser));
    }

    /// Fan `evt` to every parser registered for its kind. Called by the
    /// kernel's ingest path; non-existent registrations are a fast no-op.
    pub fn dispatch(&self, evt: &VerifiedEvent) {
        let kind = evt.raw().kind;
        if let Some(parsers) = self.by_kind.get(&kind) {
            for p in parsers {
                p.parse(evt);
            }
        }
        for (range, p) in &self.by_range {
            if range.contains(&kind) {
                p.parse(evt);
            }
        }
    }

    /// Number of parser registrations (for diagnostics + tests). Counts each
    /// per-kind and per-range registration once, not per kind matched.
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
        let new = CapturingParser::new();

        // Register two old parsers for kind 42.
        d.register_kind(42, old.clone());
        d.register_kind(42, old.clone());
        assert_eq!(d.registration_count(), 2);

        // Replace: only the new parser survives.
        let prev = d.replace_kind_parser(42, new.clone());
        assert_eq!(prev.len(), 2, "both old parsers returned as previous");
        assert_eq!(d.registration_count(), 1, "exactly one parser remains after replace");

        d.dispatch(&evt(42));
        assert_eq!(old.kinds(), Vec::<u32>::new(), "old parsers must NOT fire after replace");
        assert_eq!(new.kinds(), vec![42], "new parser must fire after replace");
    }

    #[test]
    fn replace_kind_parser_on_empty_slot_returns_empty() {
        let mut d = EventIngestDispatcher::new();
        let p = CapturingParser::new();
        let prev = d.replace_kind_parser(9999, p.clone());
        assert!(prev.is_empty(), "replacing an absent kind returns empty vec");
        assert_eq!(d.registration_count(), 1);
        d.dispatch(&evt(9999));
        assert_eq!(p.kinds(), vec![9999]);
    }
}
