//! Relay-pinned pull tests for host-scoped feed sources.

use std::num::NonZeroUsize;

use crate::kernel::pull::{PullError, PullLimits, PullScope};
use crate::kernel::Kernel;
use crate::planner::InterestShape;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, ScanLogResult, VerifiedEvent};

fn hex64(byte: u8) -> String { format!("{:02x}", byte).repeat(32) }

fn raw_tags(id_byte: u8, pk_byte: u8, kind: u32, ts: u64, tags: Vec<Vec<String>>) -> RawEvent {
    RawEvent { id: hex64(id_byte), pubkey: hex64(pk_byte), created_at: ts, kind,
               tags, content: String::new(), sig: "cc".repeat(64) }
}

fn unchecked(r: RawEvent) -> VerifiedEvent { VerifiedEvent::from_raw_unchecked(r) }

fn seed_on(k: &Kernel, r: RawEvent, relay: &str) -> u64 {
    let relay = relay.to_string();
    k.event_store_handle().insert(unchecked(r), &relay, 0).unwrap();
    k.event_store_handle().latest_ingest_seq().unwrap()
}

fn new_kernel() -> Kernel { Kernel::new(DEFAULT_VISIBLE_LIMIT) }

fn lim(max: usize, scan: usize) -> PullLimits {
    PullLimits { max_entries: NonZeroUsize::new(max).unwrap(),
                 max_scan_entries: NonZeroUsize::new(scan).unwrap() }
}

fn h_shape(local_id: &str, relay_pin: &str, kinds: impl IntoIterator<Item = u32>)
    -> InterestShape {
    let mut shape = InterestShape {
        relay_pin: Some(relay_pin.to_string()),
        ..InterestShape::default()
    };
    shape.kinds.extend(kinds);
    shape.tags.insert("h".to_string(), [local_id.to_string()].into());
    shape
}

fn page(r: ScanLogResult) -> crate::store::PullPage {
    match r { ScanLogResult::Page(p) => p, ScanLogResult::Gap(g) =>
        panic!("expected Page, got Gap(first={})", g.first_available_seq) }
}

fn pull_interest(k: &Kernel, shape: InterestShape, after: u64, max: usize, scan: usize)
    -> ScanLogResult {
    k.pull_page(PullScope::InterestShape(shape), after, lim(max, scan)).unwrap()
}

#[test]
fn relay_pin_filters_interest_shape_by_source_relay() {
    let k = new_kernel();
    seed_on(
        &k,
        raw_tags(1, 0xAA, 9, 1000, vec![vec!["h".into(), "room".into()]]),
        "wss://relay-a",
    );
    seed_on(
        &k,
        raw_tags(2, 0xBB, 9, 1100, vec![vec!["h".into(), "room".into()]]),
        "wss://relay-b",
    );
    seed_on(
        &k,
        raw_tags(3, 0xCC, 9, 1200, vec![vec!["h".into(), "room".into()]]),
        "local://publish",
    );

    let p = page(pull_interest(&k, h_shape("room", "wss://relay-a", [9]), 0, 10, 100));
    let ids = p
        .entries
        .iter()
        .map(|entry| entry.raw_event.as_ref().unwrap().id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![hex64(1), hex64(3)],
        "relay-pinned pull must include only the pinned host plus local publishes"
    );
}

#[test]
fn relay_pinned_pull_uses_current_provenance_after_duplicate_delivery() {
    let k = new_kernel();
    let event = raw_tags(1, 0xAA, 9, 1000, vec![vec!["h".into(), "room".into()]]);
    seed_on(&k, event.clone(), "wss://relay-b");
    seed_on(&k, event, "wss://relay-a");

    let p = page(pull_interest(&k, h_shape("room", "wss://relay-a", [9]), 0, 10, 100));
    assert_eq!(p.entries.len(), 1);
    assert_eq!(
        p.entries[0].raw_event.as_ref().unwrap().id,
        hex64(1),
        "host provenance learned by duplicate delivery must make the original log row visible"
    );
}

#[test]
fn interest_shapes_union_preserves_non_mergeable_relay_pins() {
    let k = new_kernel();
    seed_on(
        &k,
        raw_tags(1, 0xAA, 9, 1000, vec![vec!["h".into(), "room-a".into()]]),
        "wss://relay-a",
    );
    seed_on(
        &k,
        raw_tags(2, 0xBB, 9, 1100, vec![vec!["h".into(), "room-b".into()]]),
        "wss://relay-b",
    );
    seed_on(
        &k,
        raw_tags(3, 0xCC, 9, 1200, vec![vec!["h".into(), "room-a".into()]]),
        "wss://relay-b",
    );

    let p = page(
        k.pull_page(
            PullScope::InterestShapes(vec![
                h_shape("room-a", "wss://relay-a", [9]),
                h_shape("room-b", "wss://relay-b", [9]),
            ]),
            0,
            lim(10, 100),
        )
        .unwrap(),
    );
    let ids = p
        .entries
        .iter()
        .map(|entry| entry.raw_event.as_ref().unwrap().id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![hex64(1), hex64(2)],
        "multi-shape pull must union exact host-pinned interests without admitting same-h on another host"
    );
}

#[test]
fn interest_shapes_reject_any_unsupported_member() {
    let k = new_kernel();
    let mut supported = InterestShape::default();
    supported.authors.insert(hex64(0xAA));
    supported.kinds.insert(1);
    let mut unsupported = InterestShape::default();
    unsupported.kinds.insert(1);
    unsupported.event_ids.insert(hex64(1));

    let err = k
        .pull_page(PullScope::InterestShapes(vec![supported, unsupported]), 0, lim(10, 100))
        .unwrap_err();
    assert!(matches!(err, PullError::UnsupportedInterestShape));
}
