//! K3 Stage D2 (ADR-0056 §4) — the fixture-relay JOURNEY test: the merge gate.
//!
//! ADR-0056 §4 mandates that Stage D not land without a fixture-relay journey
//! test proving the **H1 headline** end to end: *follow a user AFTER a thread
//! reply from them is already stored, and confirm the author's FULL history
//! backfills* — the floor must no longer suppress below-the-stray history. A
//! unit test on the ledger is necessary but not sufficient (those live in
//! `coverage_ledger_d2_tests.rs`); the swap changes real fetch behaviour and
//! must be proven against a relay.
//!
//! ## Why this drives the kernel, not the C-ABI
//!
//! The load-bearing path is `recompile → REQ → ingest → projection`, all
//! kernel-internal. The since-floor read lives in the kernel's installed
//! `WatermarkFn` closure (`kernel/mod.rs`), reachable only from inside this
//! crate (the closure reads the coverage ledger off the kernel store). So the gate is a
//! kernel-level integration test: it drives the REAL production watermark
//! closure (NOT a stub) through `lifecycle_mut().recompile_and_diff`, forwards
//! the compiled REQ over a REAL WebSocket to an in-process responding relay,
//! and ingests the relay's reply through the REAL Schnorr-verify + store-insert
//! gate via `handle_message`. The C-ABI shell it skips is thin (Chirp-rule:
//! zero logic) and is not where H1 lives.
//!
//! ## Why an in-process responding relay, not `nak serve`
//!
//! The gate must RUN in the default PR test lane (`cargo test --workspace`,
//! which does NOT pass `--ignored`), or it is not a gate. `nak serve` is an
//! external binary absent on the CI runner, so a nak-dependent test would
//! `#[ignore]` (nightly only) and never gate the PR. This in-process relay is
//! deterministic, hermetic, and exercises the identical real REQ→EVENT→EOSE
//! wire path. A `nak serve` companion is provided in `nmp-testing` as an
//! `#[ignore]` nightly variant for the ADR's literal "nak serve" language.
//!
//! ## The scenario (H1, end to end)
//!
//! 1. Author A has THREE kind:1 events on the relay: t=100, t=200, t=300.
//! 2. The client has stored ONLY the t=300 event — the "stray" thread reply,
//!    acquired earlier under an Etag/thread shape (modelled by a direct store
//!    insert; the acquiring shape is irrelevant — what matters is that a kind:1
//!    by A sits in `idx_author_kind` at t=300).
//! 3. The user FOLLOWS A: A joins `timeline_authors` and a follow-feed interest
//!    `authors:[A], kinds:[1]` is registered and recompiled.
//! 4. **Flag ON:** the follow-feed `(filter_hash, relay)` has NO coverage row,
//!    so the floor is REFUSED → the REQ is un-floored (`since` absent) → the
//!    relay backfills ALL THREE events → the client ends with A's FULL history.
//! 5. **Flag OFF (regression):** the presence floor finds the stray (t=300) and
//!    floors `since=301` → the relay returns only t=300 → history below the
//!    stray is SUPPRESSED (the bug). This proves the test genuinely
//!    discriminates the two floors AND that flag-off behaviour is unchanged.

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nostr::{EventBuilder, JsonUtil as _, Keys, Timestamp};
use tungstenite::Message;

use crate::kernel::{Kernel, RelayFrame};
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::StoreQuery;
use crate::subs::WireFrame;

const E1: u64 = 100;
const E2: u64 = 200;
const E3_STRAY: u64 = 300; // the stray thread reply already on disk

/// A signed kind:1 event by `keys` at `created_at`. Real Schnorr signature so
/// the kernel's ingest verify gate (`VerifiedEvent::try_from_raw`) admits it.
fn signed_note(keys: &Keys, created_at: u64, content: &str) -> nostr::Event {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:1")
}

// ─── In-process responding relay ────────────────────────────────────────────

/// A minimal in-process Nostr relay over a real WebSocket: it holds a fixed set
/// of events and, on `["REQ", sub, filter]`, replays every held event matching
/// the filter's `authors`/`kinds`/`since` window (newest-last), then `EOSE`.
/// This is the mechanism the floor poisons — a spec-compliant relay HONOURS the
/// REQ `since`, so a floored REQ never returns below-floor events.
struct RespondingRelay {
    addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl RespondingRelay {
    fn spawn(events: Vec<nostr::Event>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral relay");
        let addr = listener.local_addr().expect("relay local_addr");
        listener
            .set_nonblocking(true)
            .expect("relay set_nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            loop {
                if stop_t.load(Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let evs = events.clone();
                        let stop_c = Arc::clone(&stop_t);
                        thread::spawn(move || serve_conn(stream, evs, stop_c));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return,
                }
            }
        });
        Self {
            addr,
            stop,
            handle: Some(handle),
        }
    }

    fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.addr.port())
    }
}

impl Drop for RespondingRelay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve_conn(stream: TcpStream, events: Vec<nostr::Event>, stop: Arc<AtomicBool>) {
    stream.set_nonblocking(false).ok();
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(_) => return,
    };
    while !stop.load(Ordering::Relaxed) {
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return,
        };
        let Message::Text(text) = msg else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(arr) = value.as_array() else { continue };
        if arr.first().and_then(|v| v.as_str()) != Some("REQ") {
            continue;
        }
        let sub_id = arr.get(1).and_then(|v| v.as_str()).unwrap_or("sub");
        let filter = arr.get(2).cloned().unwrap_or(serde_json::Value::Null);
        let since = filter.get("since").and_then(serde_json::Value::as_u64);
        let authors: Vec<String> = filter
            .get("authors")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let kinds: Vec<u64> = filter
            .get("kinds")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();

        // Replay every held event matching the filter window. A spec-compliant
        // relay HONOURS `since` — this is exactly the mechanism the floor
        // poisons (a floored REQ never sees below-floor events).
        for ev in &events {
            let ts = ev.created_at.as_secs();
            if let Some(s) = since {
                if ts < s {
                    continue;
                }
            }
            if !authors.is_empty() && !authors.contains(&ev.pubkey.to_hex()) {
                continue;
            }
            if !kinds.is_empty() && !kinds.contains(&u64::from(ev.kind.as_u16())) {
                continue;
            }
            let frame = format!("[\"EVENT\",\"{}\",{}]", sub_id, ev.as_json());
            if ws.send(Message::Text(frame)).is_err() {
                return;
            }
        }
        let eose = format!("[\"EOSE\",\"{}\"]", sub_id);
        let _ = ws.send(Message::Text(eose));
        let _ = ws.flush();
    }
}

// ─── Shared scenario plumbing ────────────────────────────────────────────────

fn follow_feed_interest(author_hex: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [author_hex.to_string()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        // Tailing: a live follow feed. since=None is the cold-follow state; the
        // T129 rewrite narrows it to watermark+1 ONLY when the floor resolver
        // returns Some — under D2 flag-on with no coverage row it returns None,
        // so the REQ stays un-floored.
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Forward `req_text` to `relay_url` over a real WebSocket, collect every
/// EVENT/EOSE frame until EOSE (or a short deadline), and ingest each into the
/// kernel through the real `handle_message` verify+store gate.
fn run_req_through_relay(kernel: &mut Kernel, relay_url: &str, req_text: &str) {
    let mut ws = {
        let (ws, _resp) = tungstenite::connect(relay_url).expect("connect to relay");
        ws
    };
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_mut() {
        s.set_read_timeout(Some(Duration::from_millis(200))).ok();
    }
    ws.send(Message::Text(req_text.to_string()))
        .expect("send REQ");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut got_eose = false;
    while Instant::now() < deadline && !got_eose {
        match ws.read() {
            Ok(Message::Text(text)) => {
                if text.starts_with("[\"EOSE\"") {
                    got_eose = true;
                } else if text.starts_with("[\"EVENT\"") {
                    kernel.handle_message(
                        RelayRole::Content,
                        relay_url,
                        RelayFrame::Text(text),
                    );
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let _ = ws.close(None);
    assert!(got_eose, "relay must reach EOSE within the budget");
}

/// Count stored kind:1 events (the full-history oracle). Only author A's notes
/// exist in the test store, so a `KindTime` scan over kind:1 counts exactly A's
/// stored history without needing the raw-bytes pubkey decode.
fn stored_note_count(kernel: &Kernel, _author_hex: &str) -> usize {
    let mut count = 0usize;
    let query = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    kernel
        .event_store_handle()
        .query_visit(&query, 1000, &mut |_ev| {
            count += 1;
            std::ops::ControlFlow::Continue(())
        })
        .expect("store scan");
    count
}

/// Build a kernel pre-loaded with the stray (t=300) event for `keys`, the
/// author followed, and a follow-feed interest registered + recompiled against
/// `relay_url`. Returns the kernel and the compiled follow-feed REQ text.
fn scenario(keys: &Keys, relay_url: &str) -> (Kernel, String) {
    let author_hex = keys.public_key().to_hex();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // The user follows A (admits A's notes into the timeline store).
    kernel.timeline_authors.insert(author_hex.clone());

    // PHASE: the stray thread-reply (t=300) is ALREADY on disk. Ingest it
    // through the real verify+store gate so it lands in idx_author_kind exactly
    // as a prior Etag-shape acquisition would have left it.
    let stray = signed_note(keys, E3_STRAY, "stray thread reply");
    kernel.handle_message(
        RelayRole::Content,
        relay_url,
        RelayFrame::Text(format!("[\"EVENT\",\"sub-stray\",{}]", stray.as_json())),
    );
    assert_eq!(
        stored_note_count(&kernel, &author_hex),
        1,
        "precondition: exactly the stray (t=300) is stored before the follow REQ",
    );

    // The user follows A: register the follow-feed interest + recompile through
    // the REAL production watermark closure (it reads the coverage ledger).
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        author_hex.clone(),
        MailboxSnapshot {
            write_relays: vec![relay_url.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    {
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        use crate::subs::SubIdentity;
        let t = RegistryWriteToken::for_test();
        let interest = follow_feed_interest(&author_hex);
        let identity = SubIdentity::for_standing_interest(&interest);
        kernel
            .lifecycle_mut()
            .registry_mut()
            .apply(&t, InterestWrite::Replace, identity, interest);
    }
    let frames = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("recompile");

    let req_text = frames
        .iter()
        .find_map(|f| match f {
            WireFrame::Req {
                relay_url: r,
                sub_id,
                filter_json,
                ..
            } if r == relay_url => Some(format!("[\"REQ\",\"{}\",{}]", sub_id, filter_json)),
            _ => None,
        })
        .expect("recompile must emit a follow-feed REQ to the relay");

    // Register the wire sub so ingest accepts the relay's EVENTs on this sub_id.
    kernel.register_wire_frames_for_test(&frames);
    (kernel, req_text)
}

// ─── The merge gate: H1 backfills full history with the flag ON ────────────────

#[test]
fn h1_followfeed_backfills_full_history_with_coverage_ledger() {
    let keys = Keys::generate();
    let relay = RespondingRelay::spawn(vec![
        signed_note(&keys, E1, "oldest"),
        signed_note(&keys, E2, "middle"),
        signed_note(&keys, E3_STRAY, "stray thread reply"),
    ]);
    let url = relay.ws_url();

    let (mut kernel, req_text) = scenario(&keys, &url);

    // THE SWAP PROOF: with NO coverage row for the follow-feed (filter_hash,
    // relay), the coverage ledger REFUSES the floor — the REQ is un-floored.
    assert!(
        !req_text.contains("\"since\""),
        "no coverage row ⇒ the follow-feed REQ must be UN-floored (full window), \
         so the relay can backfill below the stray; got {req_text}",
    );

    run_req_through_relay(&mut kernel, &url, &req_text);

    // THE H1 HEADLINE: the author's FULL history (all three events) backfills —
    // the stray no longer suppresses the older t=100 / t=200 events.
    assert_eq!(
        stored_note_count(&kernel, &keys.public_key().to_hex()),
        3,
        "H1: with the coverage ledger, following A AFTER a stray reply must \
         backfill A's FULL history (t=100, t=200, t=300), not just the stray",
    );
}
