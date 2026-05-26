//! V-51 phase 4 — routing-trace real-relay validation harness.
//!
//! Proves the architectural property the user named on 2026-05-24:
//! **"Chirp doesn't lift a finger"** — the kernel's substrate routes a
//! subscription for a real author to that author's *declared* NIP-65 read
//! relays, with no app-level intervention. The test:
//!
//! 1. Constructs the production routing impls in place:
//!    [`nmp_router::GenericOutboxRouter`] + [`nmp_router::InMemoryMailboxCache`].
//! 2. Wires a fresh [`nmp_core::RoutingTraceProjection`] onto the router
//!    via `with_trace_observer` — exactly the same projection type the
//!    kernel creates and threads through production composition's
//!    `set_routing_substrate` factory (`Kernel::set_routing` injection).
//! 3. Fetches the real, signed kind:10002 for **pablof7z** (`fa984bd7…`)
//!    from a small set of public relays.
//! 4. Feeds the live kind:10002 through [`nmp_router::Kind10002Parser`] to
//!    populate the cache (the same parser the kernel registers via
//!    `EventIngestDispatcher`).
//! 5. Routes a subscription for `pablof7z` via
//!    `OutboxRouter::route_subscription` and reads the projection's
//!    `snapshot_subscriptions()` ring.
//! 6. **Asserts**: every resolved URL carries the `Nip65 { Read }` lane,
//!    none carry `AppRelay { Fallback }` (lane 7), and the resolved set
//!    equals pablo's *declared* read-relay set.
//!
//! This is the smallest e2e proof that the routing architecture works
//! against real network data; no NmpApp instance is required because the
//! kernel's default router and `GenericOutboxRouter` share the same
//! algorithm and the same observability seam.
//!
//! ## To run
//!
//! ```bash
//! cargo test --ignored -p nmp-testing --test routing_trace_real_nostr -- --nocapture
//! ```
//!
//! Marked `#[ignore]` by default — CI is hermetic; the supervisor runs
//! this manually post-merge per the V-51 phase 4 plan.
//!
//! ## Honest-validation
//!
//! If no candidate relay returns pablof7z's kind:10002 within budget, the
//! test prints a SKIP line and pass-but-skips. It NEVER fabricates a green
//! assertion. The lane-attribution assertion only runs against real data.

#[path = "real_relay_common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{drain_until, now_ms, send_text, try_open, DAMUS_RELAY, NOS_LOL, NOSTR_BAND};
use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use nmp_core::store::{RawEvent, VerifiedEvent};
use nmp_core::substrate::{
    BlockedRelaySet, Direction, MailboxCache, OutboxRouter, RoutingContext, RoutingSource,
    SessionKeySet,
};
use nmp_core::RoutingTraceProjection;
use nmp_router::{GenericOutboxRouter, InMemoryMailboxCache, Kind10002Parser};
use serde_json::Value;

/// pablof7z's NIP-65 hex pubkey. Hardcoded ground truth — this account is
/// the user's own. Their published kind:10002 is the load-bearing reference
/// data for this test.
const PABLO_HEX: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";

/// Relays to try, in priority order. Small, reliable set. Any one of them
/// holding pablo's kind:10002 is enough.
const RELAYS: &[&str] = &[
    DAMUS_RELAY,
    NOS_LOL,
    NOSTR_BAND,
    "wss://relay.snort.social",
    "wss://relay.primal.net",
];

const FETCH_BUDGET: Duration = Duration::from_secs(10);

/// Parse `["EVENT", subid, {event}]` and return the kind:10002 if it
/// matches the expected sub-id, kind, and author. Structural sanity-checks
/// the signed event shape (id+sig+created_at present, hex-shaped).
fn parse_kind10002(text: &str, sub_id: &str, author_hex: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?.as_object()?;
    if ev.get("kind")?.as_u64()? != 10002 {
        return None;
    }
    if ev.get("pubkey")?.as_str()? != author_hex {
        return None;
    }
    let id_ok = ev.get("id")?.as_str()?.len() == 64;
    let sig_ok = ev.get("sig")?.as_str()?.len() == 128;
    let ts_ok = ev.get("created_at")?.as_u64().is_some();
    if id_ok && sig_ok && ts_ok {
        Some(arr.get(2)?.clone())
    } else {
        None
    }
}

/// Extract pablo's *declared* read-relay set from the live kind:10002. Used
/// as the ground-truth oracle for the lane-attribution assertion. Mirrors
/// the `nmp_router::Kind10002Parser` rules (unmarked ⇒ both ⇒ read).
fn declared_read_relays(ev: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(tags) = ev.get("tags").and_then(Value::as_array) else {
        return out;
    };
    for tag in tags {
        let Some(parts) = tag.as_array() else { continue };
        if parts.first().and_then(Value::as_str) != Some("r") {
            continue;
        }
        let Some(url) = parts.get(1).and_then(Value::as_str) else {
            continue;
        };
        if !(url.starts_with("wss://") || url.starts_with("ws://")) {
            continue;
        }
        match parts.get(2).and_then(Value::as_str) {
            // unmarked or "read" lands in the read set
            None | Some("") | Some("read") => {
                out.insert(url.to_string());
            }
            Some("write") => {}
            // unknown markers — `Kind10002Parser` drops them; mirror that
            Some(_) => {}
        }
    }
    out
}

/// Try every candidate relay until one yields pablo's kind:10002. Returns
/// `Some((relay_used, raw_event_json))` on hit; `None` if no relay had it
/// within budget.
fn fetch_pablo_kind10002() -> Option<(&'static str, String)> {
    for relay in RELAYS {
        let Some(mut socket) = try_open(relay) else {
            continue;
        };
        let sub_id = format!("v51p4-{}", now_ms());
        let req = format!(
            "[\"REQ\",\"{sub_id}\",{{\"authors\":[\"{PABLO_HEX}\"],\"kinds\":[10002],\"limit\":1}}]"
        );
        if send_text(&mut socket, req).is_err() {
            let _ = socket.close(None);
            continue;
        }
        let deadline = Instant::now() + FETCH_BUDGET;
        let mut captured: Option<Value> = None;
        drain_until(&mut socket, deadline, |text| {
            if let Some(ev) = parse_kind10002(text, &sub_id, PABLO_HEX) {
                captured = Some(ev);
                return true;
            }
            // EOSE — stop waiting on this relay
            if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(text) {
                if a.first().and_then(Value::as_str) == Some("EOSE")
                    && a.get(1).and_then(Value::as_str) == Some(sub_id.as_str())
                {
                    return true;
                }
            }
            false
        });
        let _ = send_text(&mut socket, format!("[\"CLOSE\",\"{sub_id}\"]"));
        let _ = socket.close(None);

        if let Some(ev) = captured {
            return Some((*relay, ev.to_string()));
        }
        eprintln!("[v51p4] {relay}: no kind:10002 within {FETCH_BUDGET:?}");
    }
    None
}

fn interest_for_pablo() -> LogicalInterest {
    LogicalInterest {
        id: InterestId(20251024),
        scope: InterestScope::Global,
        shape: InterestShape::timeline_for([PABLO_HEX.to_string()].into_iter().collect()),
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn routing_trace_real_nostr_pablo_nip65_read_set() {
    let Some((relay_used, event_json)) = fetch_pablo_kind10002() else {
        eprintln!(
            "SKIP: pablof7z's kind:10002 was not returned by any of {RELAYS:?} within \
             {FETCH_BUDGET:?} per relay. Re-run with network access."
        );
        return;
    };
    eprintln!("[v51p4] fetched kind:10002 from {relay_used}");

    // 1. Set up the production routing impls.
    let cache = Arc::new(InMemoryMailboxCache::new());
    let projection = Arc::new(RoutingTraceProjection::new());
    let router = GenericOutboxRouter::new().with_trace_observer(
        Arc::clone(&projection) as Arc<dyn nmp_core::substrate::RoutingTraceObserver>,
    );

    // 2. Feed the live kind:10002 through the substrate parser to seed the
    //    cache. This is exactly what the kernel does when a kind:10002
    //    lands via `EventIngestDispatcher`.
    let raw: RawEvent =
        serde_json::from_str(&event_json).expect("RawEvent decode of pablo's live kind:10002");
    let verified =
        VerifiedEvent::try_from_raw(raw).expect("verify pablo's kind:10002 schnorr signature");
    let parser = Kind10002Parser::new(Arc::clone(&cache));
    parser.parse_event(&verified);

    // 3. Extract declared read-set for the ground-truth comparison.
    let event_value: Value = serde_json::from_str(&event_json).expect("re-parse for tags read");
    let declared_reads = declared_read_relays(&event_value);
    assert!(
        !declared_reads.is_empty(),
        "pablo's kind:10002 must have at least one read or both `r`-tag for this test to mean anything",
    );
    eprintln!(
        "[v51p4] pablo's declared read-relays ({}): {declared_reads:?}",
        declared_reads.len()
    );

    // 4. Confirm the cache returns the declared set on the read lane.
    let cache_reads: BTreeSet<String> = cache
        .read_relays(&PABLO_HEX.to_string())
        .expect("cache must hold pablo after parse")
        .into_iter()
        .collect();
    assert_eq!(
        cache_reads, declared_reads,
        "Kind10002Parser must seed the cache with pablo's declared read-relay set"
    );

    // 5. Route a subscription for pablo via the production router.
    let blocked = BlockedRelaySet::new();
    // No app-relay configured — the assertion is precisely that lane 7
    // (AppRelay/Fallback) does NOT fire, so we leave `app_relays` empty.
    // If lane 1 misbehaves and returns nothing, the router would surface
    // `Unroutable` rather than secretly fall back.
    let app_relays: Vec<String> = vec![];
    let ctx = RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            app_relays: &app_relays,
            ..SessionKeySet::default()
        },
        mailbox_cache: &*cache,
        blocked_relays: &blocked,
        explicit_targets: None,
    };
    let interest = interest_for_pablo();
    let routed = router
        .route_subscription(&interest, &ctx)
        .expect("router must resolve a real NIP-65 read set");

    // 6. Read the projection ring buffer — the observer fired on the
    //    successful route call.
    let snap = projection.snapshot_subscriptions();
    assert_eq!(
        snap.len(),
        1,
        "exactly one subscription routing decision should be observed"
    );
    let entry = &snap[0];
    assert_eq!(entry.trace.interest_id, interest.id.0);
    assert_eq!(entry.trace.authors_count, 1);
    assert!(
        !entry.trace.explicit_targets_set,
        "we did not set explicit_targets — observer must reflect that"
    );

    // 7. The actual property: every resolved URL is attributed to the
    //    Nip65/Read lane, and none to AppRelay/Fallback.
    let resolved_urls: BTreeSet<String> = entry.urls.iter().map(|(u, _)| u.clone()).collect();
    assert_eq!(
        resolved_urls, declared_reads,
        "router must resolve to pablo's exact declared NIP-65 read-relay set; \
         got {resolved_urls:?} declared {declared_reads:?}"
    );

    let mut any_lane7_seen = false;
    for (url, sources) in &entry.urls {
        let mut has_nip65_read = false;
        for source in sources {
            match source {
                RoutingSource::Nip65 { direction: Direction::Read } => {
                    has_nip65_read = true;
                }
                RoutingSource::AppRelay { .. } => {
                    any_lane7_seen = true;
                    eprintln!("[v51p4] UNEXPECTED lane 7 attribution on {url}: {source:?}");
                }
                _ => {}
            }
        }
        assert!(
            has_nip65_read,
            "URL {url} resolved but was not attributed to Nip65/Read: {sources:?}"
        );
    }
    assert!(
        !any_lane7_seen,
        "no resolved URL may carry AppRelay/Fallback (lane 7) — \
         that would mean the NIP-65 path failed silently"
    );

    // 8. Also sanity-check the underlying routed set matches what the
    //    observer saw (the projection is the seam for downstream consumers
    //    like chirp-repl's routing-trace subcommand and any inspector UI).
    let routed_urls: BTreeSet<String> = routed.urls().cloned().collect();
    assert_eq!(
        routed_urls, declared_reads,
        "routed set must equal declared read set (cache + router are consistent with the projection)"
    );

    eprintln!(
        "[v51p4] PASS — pablo's subscription routed to {} relay(s) via Nip65/Read; \
         AppRelay/Fallback NOT used. Lanes attributed by URL:",
        entry.urls.len()
    );
    for (url, sources) in &entry.urls {
        let lanes: Vec<String> = sources.iter().map(|s| format!("{s:?}")).collect();
        eprintln!("  {url} -> {}", lanes.join(", "));
    }
}
