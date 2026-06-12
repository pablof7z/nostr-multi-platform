//! Kernel-level contract test: `ingest_pre_verified_event` feeds the
//! `EventIngestDispatcher`.
//!
//! Regression guard for the PR-#1137 raw-tap retirement ladder regression:
//! after the DM inbox and Marmot migrated from `RawEventObserver` to
//! `IngestParser`, the `ingest_pre_verified_event` test-support path
//! (used by `ActorCommand::IngestPreVerifiedEvents` and therefore by
//! `nmp_app_inject_signed_event_json`) silently missed every registered
//! `IngestParser` because only `notify_raw_event_observers` was called —
//! the `EventIngestDispatcher::dispatch` call was absent.
//!
//! This test pins the contract directly at the kernel seam:
//! `ingest_pre_verified_event` MUST dispatch to registered `IngestParser`s
//! on `Inserted` and `Replaced` outcomes, and MUST NOT fire on `Duplicate`.
//!
//! Bisect evidence: fcec05d7f (green, pre-#1137) → b8f5332e1 (red, master).

use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::VerifiedEvent;
use crate::substrate::IngestParser;
use std::sync::{Arc, Mutex};

// ─── Fixture ────────────────────────────────────────────────────────────────

/// An `IngestParser` that records the raw ID of every event it receives.
/// Used to verify the `ingest_pre_verified_event` path fires the dispatcher.
struct CapturingIngestParser {
    seen_ids: Mutex<Vec<String>>,
}

impl CapturingIngestParser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen_ids: Mutex::new(Vec::new()),
        })
    }

    fn seen_ids(&self) -> Vec<String> {
        self.seen_ids.lock().unwrap().clone()
    }
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_ids
            .lock()
            .unwrap()
            .push(evt.raw().id.clone());
    }
}

/// Build a `VerifiedEvent` from raw fields — uses `from_raw_unchecked` since
/// these are test fixtures that do not go through Schnorr verification.  The
/// ID and pubkey are deterministic 64-hex strings; sig is a placeholder.
fn fixture_verified_event(id_suffix: &str, kind: u32) -> VerifiedEvent {
    let id = format!("{:0<64}", id_suffix);
    let id = id[..64].to_string();
    let raw = crate::store::RawEvent {
        id,
        pubkey: "0".repeat(64),
        created_at: 1_700_000_000,
        kind,
        tags: Vec::new(),
        content: format!("fixture-{id_suffix}"),
        sig: "0".repeat(128),
    };
    VerifiedEvent::from_raw_unchecked(raw)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CONTRACT: `ingest_pre_verified_event` dispatches to a registered
/// `IngestParser` for the event's kind on an `Inserted` outcome.
///
/// This is the direct kernel-level companion to the
/// `dm_inbox_full_round_trip_through_ffi` FFI-level regression test in
/// `nmp-app-chirp`. Together they pin the full end-to-end seam.
#[test]
fn ingest_pre_verified_event_fires_ingest_parser_on_insert() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parser = CapturingIngestParser::new();
    // Register for kind:1059 (the DM gift-wrap kind that exposed the regression).
    kernel.register_ingest_parser(1059, Arc::clone(&parser) as Arc<dyn IngestParser>);

    let event = fixture_verified_event("aaa001", 1059);
    let event_id = event.raw().id.clone();

    kernel.ingest_pre_verified_event(RelayRole::Content, "diag-firehose-test", event);
    kernel.sort_timeline_deferred();

    let seen = parser.seen_ids();
    assert_eq!(
        seen.len(),
        1,
        "IngestParser must fire exactly once on Inserted; got {seen:?}",
    );
    assert_eq!(
        seen[0], event_id,
        "IngestParser must receive the injected event ID",
    );
}

/// DUPLICATE GATE: `ingest_pre_verified_event` must NOT re-fire the
/// `IngestParser` when the same event is injected a second time
/// (store outcome = `Duplicate`). Mirrors the D4 dedup contract in
/// `verify_and_persist`.
#[test]
fn ingest_pre_verified_event_does_not_fire_parser_on_duplicate() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parser = CapturingIngestParser::new();
    kernel.register_ingest_parser(1059, Arc::clone(&parser) as Arc<dyn IngestParser>);

    let event_a = fixture_verified_event("bbb001", 1059);
    // Build an identical second copy by re-wrapping the same raw.
    let event_b = VerifiedEvent::from_raw_unchecked(event_a.raw().clone());

    // First inject — store outcome = Inserted → parser fires once.
    kernel.ingest_pre_verified_event(RelayRole::Content, "diag-firehose-test", event_a);
    // Second inject of the same ID — store outcome = Duplicate → parser must NOT fire.
    kernel.ingest_pre_verified_event(RelayRole::Content, "diag-firehose-test", event_b);
    kernel.sort_timeline_deferred();

    let seen = parser.seen_ids();
    assert_eq!(
        seen.len(),
        1,
        "IngestParser must fire only on the first (Inserted) delivery, not on Duplicate; got {seen:?}",
    );
}

/// KIND ROUTING: the dispatcher fans only to parsers registered for the
/// matching kind. A parser registered for kind:1059 must NOT fire for a
/// kind:1 event injected through the same path.
#[test]
fn ingest_pre_verified_event_routes_to_correct_kind_parser() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parser_1059 = CapturingIngestParser::new();
    kernel.register_ingest_parser(1059, Arc::clone(&parser_1059) as Arc<dyn IngestParser>);

    // Inject a kind:1 event — should NOT reach the kind:1059 parser.
    let kind1_event = fixture_verified_event("ccc001", 1);
    kernel.ingest_pre_verified_event(RelayRole::Content, "diag-firehose-test", kind1_event);
    kernel.sort_timeline_deferred();

    assert!(
        parser_1059.seen_ids().is_empty(),
        "kind:1059 parser must not fire for a kind:1 event; got {:?}",
        parser_1059.seen_ids(),
    );

    // Now inject a kind:1059 event — must reach the parser.
    let kind1059_event = fixture_verified_event("ccc002", 1059);
    let id_1059 = kind1059_event.raw().id.clone();
    kernel.ingest_pre_verified_event(RelayRole::Content, "diag-firehose-test", kind1059_event);
    kernel.sort_timeline_deferred();

    let seen = parser_1059.seen_ids();
    assert_eq!(seen.len(), 1, "kind:1059 parser must fire for kind:1059 event");
    assert_eq!(seen[0], id_1059);
}
