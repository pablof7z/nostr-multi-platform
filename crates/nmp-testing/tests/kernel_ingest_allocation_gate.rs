//! Real allocation gate for the kernel ingest hot path (#2798).
//!
//! The old D8 static "hot path" marker was deleted because it matched zero
//! functions. This gate measures the actor's pre-verified ingest path with
//! dhat-rs after actor startup and warm-up, then asserts both an absolute
//! per-event allocation budget and stable per-event cost as event volume grows.

use std::sync::mpsc;
use std::time::Duration;

use nmp_core::actor::{LifecycleCommand, TestSupportCommand};
use nmp_core::testing::{spawn_actor, wait_barrier, ActorCommand};
use nmp_store::{RawEvent, VerifiedEvent};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const SMALL_BATCH: u64 = 16;
const LARGE_BATCH: u64 = 96;
const MAX_BLOCKS_PER_EVENT: u64 = 256;
const MAX_BYTES_PER_EVENT: u64 = 64 * 1024;

#[test]
fn preverified_kernel_ingest_stays_inside_allocation_budget() {
    let (tx, _rx) = spawn_actor();
    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 512,
        emit_hz: 1,
        initial_relays: Vec::new(),
    }))
    .expect("send Start");
    assert!(wait_barrier(&tx, Duration::from_secs(5)));

    send_acked_batch(&tx, "alloc-gate-warmup".to_string(), events(0, 8));

    let small = events(1_000, SMALL_BATCH);
    let large = events(2_000, LARGE_BATCH);

    let profiler = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();
    send_acked_batch(&tx, "alloc-gate-small".to_string(), small);
    let after_small = dhat::HeapStats::get();
    send_acked_batch(&tx, "alloc-gate-large".to_string(), large);
    let after_large = dhat::HeapStats::get();
    drop(profiler);

    let small_blocks = after_small.total_blocks - before.total_blocks;
    let small_bytes = after_small.total_bytes - before.total_bytes;
    let large_blocks = after_large.total_blocks - after_small.total_blocks;
    let large_bytes = after_large.total_bytes - after_small.total_bytes;
    let small_blocks_per_event = small_blocks.div_ceil(SMALL_BATCH);
    let large_blocks_per_event = large_blocks.div_ceil(LARGE_BATCH);
    let small_bytes_per_event = small_bytes.div_ceil(SMALL_BATCH);
    let large_bytes_per_event = large_bytes.div_ceil(LARGE_BATCH);

    eprintln!(
        "kernel ingest allocation gate: small={} blocks/event {} bytes/event; large={} blocks/event {} bytes/event",
        small_blocks_per_event,
        small_bytes_per_event,
        large_blocks_per_event,
        large_bytes_per_event
    );

    assert!(
        large_blocks_per_event <= MAX_BLOCKS_PER_EVENT,
        "large batch allocated {large_blocks_per_event} blocks/event; budget is {MAX_BLOCKS_PER_EVENT}"
    );
    assert!(
        large_bytes_per_event <= MAX_BYTES_PER_EVENT,
        "large batch allocated {large_bytes_per_event} bytes/event; budget is {MAX_BYTES_PER_EVENT}"
    );
    assert!(
        large_blocks_per_event <= small_blocks_per_event + 16,
        "per-event allocation count grew with volume: small={small_blocks_per_event}, large={large_blocks_per_event}"
    );
    assert!(
        large_bytes_per_event <= small_bytes_per_event + 4096,
        "per-event allocation bytes grew with volume: small={small_bytes_per_event}, large={large_bytes_per_event}"
    );
}

fn send_acked_batch(tx: &nmp_core::CommandSender, sub_id: String, events: Vec<VerifiedEvent>) {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    tx.send(ActorCommand::TestSupport(
        TestSupportCommand::IngestPreVerifiedEventsForSubId {
            sub_id,
            events,
            ack: ack_tx,
        },
    ))
    .expect("send ingest batch");
    ack_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("ingest batch ack");
}

fn events(start: u64, count: u64) -> Vec<VerifiedEvent> {
    (start..start + count)
        .map(|i| {
            VerifiedEvent::from_raw_unchecked(RawEvent {
                id: format!("{i:064x}"),
                pubkey: "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c"
                    .to_string(),
                created_at: 1_700_000_000 + i,
                kind: 1,
                tags: Vec::new(),
                content: format!("allocation gate event {i}"),
                sig: "a".repeat(128),
            })
        })
        .collect()
}
