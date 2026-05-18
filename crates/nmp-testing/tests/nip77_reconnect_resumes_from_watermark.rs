//! M4 exit-gate test #3 — relay reconnect resumes from watermark.
//!
//! "Relay reconnect after 10 simulated minutes resumes from watermark; gap
//! filled by sync." — `docs/plan/m4-negentropy.md`
//!
//! Approach: write a watermark with `synced_up_to = T`, simulate the relay
//! coming back online with an extra batch of events created at `T+1..T+N`,
//! drive the reconciliation, and assert the client converges with `need.len()
//! == N` — i.e. it pulled exactly the gap, not the prior data it already had.

mod nip77_common;

use nip77_common::{reconcile_in_process, synth_items};
use nmp_core::store::{EventStore, MemEventStore, SyncMethod, WatermarkKey, WatermarkRow};
use nmp_nip77::{
    decide_strategy, GateInputs, RelayCapabilities, ReconcileWork, SyncStrategy,
    TriggerEngine, TriggerEvent,
};

const PRIOR_EVENTS: u32 = 800;
const GAP_EVENTS: u32 = 50;
const BASE_TS: u64 = 1_700_000_000;
const STALENESS_S: u64 = 600; // simulate 10 minutes offline

#[test]
fn reconnect_resumes_from_watermark_and_fills_gap() {
    // 1. Persist a watermark covering the prior 800 events.
    let store = MemEventStore::new();
    let watermark_ts = BASE_TS + PRIOR_EVENTS as u64;
    let key = WatermarkKey {
        filter_hash: [0xCC; 32],
        relay_url: "wss://r/".into(),
    };
    let resume_blob = vec![0x55u8, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55];
    store
        .write_watermark(WatermarkRow {
            key: key.clone(),
            synced_up_to: watermark_ts,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: Some(resume_blob.clone()),
            bytes_saved_vs_req: 12_345,
            updated_at: watermark_ts.saturating_sub(STALENESS_S),
        })
        .expect("write watermark");
    let stored = store.read_watermark(&key).expect("read").unwrap();
    assert_eq!(stored.synced_up_to, watermark_ts);

    // 2. The relay-reconnect trigger fires for this relay.
    let mut triggers = TriggerEngine::new();
    triggers.register([0xCC; 32], "wss://r/");
    let work: Vec<ReconcileWork> = triggers.on_event(TriggerEvent::RelayReconnected {
        relay_url: "wss://r/".into(),
    });
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].filter_hash, [0xCC; 32]);

    // 3. The coverage gate sees a stale watermark with a resume blob and the
    //    relay's known NIP-77 support, so it returns Resume { NegThenReq }.
    //    `updated_at` is `watermark_ts - STALENESS_S` (i.e. 10 minutes old);
    //    the store's 300 s staleness window therefore reports `PartialUpTo`.
    let coverage = nmp_core::store::Coverage::PartialUpTo(stored.synced_up_to);
    let strat = decide_strategy(
        &key,
        GateInputs {
            coverage,
            capabilities: Some(RelayCapabilities {
                supports_nip77: true,
            }),
            watermark: Some(stored.clone()),
        },
    );
    match &strat {
        SyncStrategy::Resume { next, state } => {
            assert_eq!(**next, SyncStrategy::NegThenReq);
            assert_eq!(state, &resume_blob);
        }
        other => panic!("expected Resume{{NegThenReq}}, got {other:?}"),
    }

    // 4. Drive the reconciliation: the client knows the prior 800 events; the
    //    relay (server) holds prior 800 + an extra `GAP_EVENTS`. The client
    //    must converge with `need.len() == GAP_EVENTS` — exactly the gap.
    //
    //    `synth_items` packs `base_ts + i` into the id bytes, so the prior
    //    block (ts ∈ [BASE_TS, BASE_TS + 800)) and the gap block
    //    (ts ∈ [BASE_TS + 801, BASE_TS + 851)) are guaranteed not to overlap
    //    — the id set difference is exactly `GAP_EVENTS` items.
    let client_items = synth_items(PRIOR_EVENTS, BASE_TS);
    let mut server_items = synth_items(PRIOR_EVENTS, BASE_TS);
    server_items.extend(synth_items(GAP_EVENTS, BASE_TS + PRIOR_EVENTS as u64 + 1));

    let session = reconcile_in_process(client_items, server_items, 32);

    assert_eq!(
        session.need.len(),
        GAP_EVENTS as usize,
        "client should pull only the gap events"
    );
    assert!(
        session.have.is_empty(),
        "client should have nothing extra to push"
    );

    // 5. Update the watermark to reflect the new synced_up_to.  Round-trip
    //    confirms persistence works.
    let new_ts = BASE_TS + (PRIOR_EVENTS + GAP_EVENTS) as u64 + 1;
    store
        .write_watermark(WatermarkRow {
            key: key.clone(),
            synced_up_to: new_ts,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: Some(session.resume_state.clone()),
            bytes_saved_vs_req: stored.bytes_saved_vs_req
                + (session.bytes_client_to_server + session.bytes_server_to_client) / 4,
            updated_at: new_ts,
        })
        .expect("write resumed watermark");
    let refreshed = store.read_watermark(&key).expect("read again").unwrap();
    assert_eq!(refreshed.synced_up_to, new_ts);
}
