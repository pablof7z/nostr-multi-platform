//! Ignored performance harness for timeline ingest.
//!
//! Run with:
//! `cargo test -p nmp-core timeline_ingest_perf --release -- --ignored --nocapture`
//!
//! The test pre-generates signed Nostr events and times only kernel ingest.

use super::nostr::NostrEvent;
use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use std::time::Instant;

const EVENT_COUNT: usize = 5_000;
const VISIBLE_LIMIT: usize = 500;

fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    let nostr_event = ::nostr::EventBuilder::text_note(content)
        .custom_created_at(::nostr::Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("signing a generated-key note should succeed");
    NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|tag: &::nostr::Tag| tag.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

fn make_events(count: usize) -> Vec<NostrEvent> {
    let keys = ::nostr::Keys::generate();
    (0..count)
        .map(|i| {
            let newest_first_scramble = (i.wrapping_mul(37) % count) as u64;
            signed_note(
                &keys,
                &format!("timeline perf note {i}"),
                1_700_000_000 + newest_first_scramble,
            )
        })
        .collect()
}

#[test]
#[ignore = "manual perf harness; emits timings for PR evidence"]
fn timeline_ingest_perf() {
    let events = make_events(EVENT_COUNT);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_visible_limit(VISIBLE_LIMIT);

    let started = Instant::now();
    for event in events {
        kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://perf.example",
            "diag-firehose-timeline-perf",
            event,
        );
    }
    let elapsed = started.elapsed();

    let visible = kernel.timeline.len();
    assert_eq!(visible, VISIBLE_LIMIT);
    assert!(kernel
        .timeline
        .iter()
        .zip(kernel.timeline.iter().skip(1))
        .all(|(left, right)| {
            let a = kernel.events.get(left).expect("left event is cached");
            let b = kernel.events.get(right).expect("right event is cached");
            b.created_at < a.created_at || (b.created_at == a.created_at && left <= right)
        }));

    let legacy_sort_calls_avoided = EVENT_COUNT;
    let legacy_cloned_ids_avoided_estimate = VISIBLE_LIMIT * (VISIBLE_LIMIT + 1) / 2
        + VISIBLE_LIMIT * (EVENT_COUNT.saturating_sub(VISIBLE_LIMIT));
    println!(
        "timeline_ingest_perf events={EVENT_COUNT} visible_limit={VISIBLE_LIMIT} \
         elapsed_ms={} per_event_us={:.2} ordering=incremental_sorted_insert \
         legacy_sort_calls_avoided={} legacy_cloned_ids_avoided_estimate={}",
        elapsed.as_millis(),
        elapsed.as_secs_f64() * 1_000_000.0 / EVENT_COUNT as f64,
        legacy_sort_calls_avoided,
        legacy_cloned_ids_avoided_estimate
    );
}
