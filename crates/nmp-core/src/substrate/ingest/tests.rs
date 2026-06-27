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

struct SourceCapturingParser {
    seen: Mutex<Vec<(u32, u64, Option<String>)>>,
}

impl SourceCapturingParser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<(u32, u64, Option<String>)> {
        self.seen.lock().unwrap().clone()
    }
}

impl IngestParser for SourceCapturingParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen.lock().unwrap().push((evt.raw().kind, 0, None));
    }

    fn parse_at_source(&self, evt: &VerifiedEvent, now_secs: u64, source_relay_url: Option<&str>) {
        self.seen.lock().unwrap().push((
            evt.raw().kind,
            now_secs,
            source_relay_url.map(str::to_string),
        ));
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
fn dispatch_at_source_carries_timestamp_and_relay() {
    let mut d = EventIngestDispatcher::new();
    let p = SourceCapturingParser::new();
    d.register_kind(1059, p.clone());

    d.dispatch_at_source(&evt(1059), 1_700_000_123, Some("wss://dm.example"));

    assert_eq!(
        p.seen(),
        vec![(1059, 1_700_000_123, Some("wss://dm.example".to_string()))]
    );
}

#[test]
fn dispatch_at_keeps_source_absent() {
    let mut d = EventIngestDispatcher::new();
    let p = SourceCapturingParser::new();
    d.register_kind(1059, p.clone());

    d.dispatch_at(&evt(1059), 1_700_000_123);

    assert_eq!(p.seen(), vec![(1059, 1_700_000_123, None)]);
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
    assert_eq!(
        d.registration_count(),
        1,
        "exactly one parser remains after replace"
    );

    d.dispatch(&evt(42));
    assert_eq!(
        old.kinds(),
        Vec::<u32>::new(),
        "old parser must NOT fire after replace"
    );
    assert_eq!(
        new_p.kinds(),
        vec![42],
        "new parser must fire after replace"
    );
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
    assert_eq!(
        d.registration_count(),
        2,
        "slot count stays 2 after slot-a replace"
    );

    d.dispatch(&evt(1059));
    assert_eq!(
        p_a1.kinds(),
        Vec::<u32>::new(),
        "old slot-a parser must NOT fire"
    );
    assert_eq!(p_a2.kinds(), vec![1059], "new slot-a parser must fire");
    assert_eq!(
        p_b.kinds(),
        vec![1059],
        "slot-b parser must STILL fire after slot-a replace"
    );
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
    assert_eq!(
        d.registration_count(),
        1,
        "exactly one range registration after replace"
    );

    d.dispatch(&evt(1));
    assert_eq!(
        old.kinds(),
        Vec::<u32>::new(),
        "evicted parser must NOT fire"
    );
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
    assert_eq!(
        p.kinds(),
        Vec::<u32>::new(),
        "evicted range parser must NOT fire"
    );
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
    assert_eq!(
        d.registration_count(),
        2,
        "still exactly 2 range registrations"
    );

    d.dispatch(&evt(15_000));
    assert_eq!(
        a1.kinds(),
        vec![15_000],
        "old slot-a must NOT fire after replace"
    );
    assert_eq!(a2.kinds(), vec![15_000], "new slot-a must fire");
    assert_eq!(
        b.kinds(),
        vec![15_000, 15_000],
        "slot-b must fire both times"
    );
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
    assert!(
        d.is_interested(1059),
        "all-kinds range must cover kind:1059"
    );
    assert!(
        d.is_interested(30023),
        "all-kinds range must cover kind:30023"
    );
}

/// `is_interested` returns false for an empty dispatcher.
#[test]
fn is_interested_false_for_empty_dispatcher() {
    let d = EventIngestDispatcher::new();
    assert!(
        !d.is_interested(1),
        "empty dispatcher: is_interested must be false"
    );
    assert!(
        !d.is_interested(1059),
        "empty dispatcher: is_interested must be false"
    );
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
