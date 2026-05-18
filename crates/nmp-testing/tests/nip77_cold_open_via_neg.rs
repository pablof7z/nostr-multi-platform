//! M4 exit-gate test #1 — cold open via negentropy.
//!
//! "Cold open of a profile against a NIP-77-enabled mock relay: completes
//! via negentropy, bytes-on-wire ≤ 5 % of equivalent REQ on a 10k-event
//! backfill." — `docs/plan/m4-negentropy.md`
//!
//! Approach: the M4 reconciler is transport-agnostic (see `nmp-nip77` crate
//! docs), so we exchange bytes between in-process client + server
//! reconcilers and assert the *bytes-on-wire* count against the worst-case
//! REQ baseline.  No WebSocket is needed to validate the contract this gate
//! is about.

mod nip77_common;

use nip77_common::{reconcile_in_process, req_baseline_bytes, synth_items};

const EVENT_COUNT: u32 = 10_000;
/// Average JSON-line cost per kind:1 event (`["EVENT", subid, {…}]`).
/// The Nostr `EVENT` envelope alone (id + pubkey + ts + kind + sig + empty
/// tags + empty content) is ~353 bytes.  Real-world kind:1s with one parent
/// reference (`e`-tag) + one mention (`p`-tag) + ~50 chars of content land
/// at ~700–800 bytes.  We pick 700 as a conservative lower bound — any real
/// 10k-event backfill would be larger than this, so REQ savings are strictly
/// better than the gate asserts.
const AVG_REQ_BYTES_PER_EVENT: u64 = 700;
const SAVINGS_THRESHOLD_PCT: u64 = 5;

#[test]
fn cold_open_via_neg_under_five_percent_of_req_baseline() {
    // Client starts empty (cold profile open).  Server holds the full 10k
    // event set the relay would otherwise send via REQ.
    let server_items = synth_items(EVENT_COUNT, 1_700_000_000);

    let session = reconcile_in_process(Vec::new(), server_items, 64);

    // Client must learn about every event server holds.
    assert_eq!(
        session.need.len(),
        EVENT_COUNT as usize,
        "client should need every server-held event id"
    );

    let baseline = req_baseline_bytes(EVENT_COUNT, AVG_REQ_BYTES_PER_EVENT);
    let neg_bytes = session.bytes_client_to_server + session.bytes_server_to_client;
    let pct = (neg_bytes * 100) / baseline;

    println!(
        "neg bytes on wire = {neg_bytes}; req baseline = {baseline}; ratio = {pct}%"
    );
    assert!(
        pct <= SAVINGS_THRESHOLD_PCT,
        "negentropy bytes-on-wire was {pct}% of REQ baseline; gate is ≤{SAVINGS_THRESHOLD_PCT}%"
    );
}
