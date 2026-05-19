//! Variable-resolution fetches + NIP-65 parsing.
//!
//! This module is deliberately *not* the outbox pipeline. The outbox
//! (mailbox discovery + compilation + selection) is now driven by the
//! production [`nmp_core::subs::SubscriptionLifecycle`] from `req.rs`. What
//! remains here is the thin targeted fetch that turns a `$follows` variable
//! into a concrete author set — exactly what a real "following feed"
//! ViewModule does to build its `LogicalInterest`. The manual phase-B
//! kind:10002 fan that hand-built an `InMemoryMailboxCache` has been
//! **deleted**; `recompile_and_diff` emits its own implicit discovery REQs.
//!
//! `parse_kind10002` survives because it is NIP-65 parsing (not outbox
//! logic): the `req` tick loop calls it to turn a discovery REQ response
//! into a `MailboxSnapshot` before `cache.put`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use nmp_core::planner::MailboxSnapshot;
use serde_json::{json, Value};
use tungstenite::Message;

use crate::error::{ReplError, Result};
use crate::session::Session;
use crate::ws::{next_frame, normalize_url, summarize_filter, try_connect_msg, Frame};

/// Per-indexer read budget for the kind:3 fetch. Terminal frames (EOSE /
/// CLOSED / AUTH / relay close / IO) end an indexer attempt immediately, so
/// this is only ever hit by a relay that connects, accepts the REQ, then
/// goes silent. Short so a stalled indexer falls through to the next one
/// fast instead of zeroing out discovery.
const KIND3_WAIT: Duration = Duration::from_secs(8);

/// Result of the kind:3 follows fetch — the follows set, whether it came
/// from the cache, and the elapsed wall time.
pub struct FollowsResult {
    pub follows: BTreeSet<String>,
    pub cached: bool,
    pub elapsed: Duration,
}

/// Fetch (or return cached) the seed's kind:3 `p`-tag set. This is variable
/// expansion, not outbox: turning `$follows` into a concrete pubkey set the
/// lifecycle consumes as a `LogicalInterest`. Populates
/// `session.follows_cache`.
///
/// Errors with a *seed-missing* message only when the seed is genuinely
/// unset — a set-but-uncached seed triggers the fetch.
pub fn fetch_follows(session: &mut Session) -> Result<FollowsResult> {
    let start = Instant::now();

    if let Some(cached) = &session.follows_cache {
        return Ok(FollowsResult {
            follows: cached.clone(),
            cached: true,
            elapsed: start.elapsed(),
        });
    }
    let seed = session.seed_hex.clone().ok_or_else(|| {
        ReplError::Variable(
            "$follows requires a seed; run `set-seed <nip05|npub>` first".to_string(),
        )
    })?;

    if session.indexer_relays.is_empty() {
        return Err(ReplError::Network(
            "no indexer relays configured (try `set-indexer wss://purplepag.es`)".to_string(),
        ));
    }

    let sub_id = "follows-1";
    let filter_json = json!({ "kinds": [3], "authors": [seed], "limit": 1 }).to_string();
    let summary = summarize_filter(&filter_json);

    println!("$follows: resolving kind:3 via indexer");

    // Try EVERY configured indexer in order. A relay that connects but then
    // CLOSEs / AUTHs / drops the REQ is NOT success — fall through to the
    // next indexer so a single rate-limited relay does not zero out
    // discovery. Every connect, every REQ, every terminal state is printed.
    let mut last_outcome = "no indexers".to_string();
    for url in session.indexer_relays.clone() {
        println!("  connecting {url} …");
        let mut socket = match try_connect_msg(&url) {
            Ok(s) => s,
            Err(e) => {
                println!("  {url}  ✗ error: {e}");
                last_outcome = format!("{url}: {e}");
                continue;
            }
        };
        let req = json!(["REQ", sub_id, serde_json::from_str::<Value>(&filter_json).unwrap()])
            .to_string();
        if let Err(e) = socket.send(Message::Text(req)) {
            println!("  {url}  ✗ error: send REQ: {e}");
            last_outcome = format!("{url}: send failed");
            continue;
        }
        println!("  {url}  → REQ {summary} {sub_id}");

        let deadline = Instant::now() + KIND3_WAIT;
        let mut follows: BTreeSet<String> = BTreeSet::new();
        let mut events = 0u64;
        let outcome = loop {
            if Instant::now() >= deadline {
                break Outcome::Timeout;
            }
            match next_frame(&mut socket) {
                Frame::Timeout | Frame::Other => continue,
                Frame::Event { sub_id: s, event } if s == sub_id => {
                    events += 1;
                    collect_p_tags(&event, &mut follows);
                }
                Frame::Event { .. } => continue,
                Frame::Eose { sub_id: s } if s == sub_id => break Outcome::Eose,
                Frame::Eose { .. } => continue,
                Frame::Closed { message, .. } => break Outcome::Closed(message),
                Frame::Auth { .. } => break Outcome::Auth,
                Frame::Notice { message } => {
                    println!("  {url}  • NOTICE: {message}");
                    continue;
                }
                Frame::RelayClosed => break Outcome::RelayClosed,
                Frame::Io { kind } => break Outcome::Io(kind),
            }
        };
        let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
        let _ = socket.close(None);

        match outcome {
            Outcome::Eose => {
                println!(
                    "  {url}  ✓ EOSE ({events} event{}, {} follows)",
                    if events == 1 { "" } else { "s" },
                    follows.len()
                );
                session.follows_cache = Some(follows.clone());
                return Ok(FollowsResult {
                    follows,
                    cached: false,
                    elapsed: start.elapsed(),
                });
            }
            Outcome::Closed(msg) => {
                println!("  {url}  ✗ CLOSED: {msg}");
                last_outcome = format!("{url}: CLOSED: {msg}");
            }
            Outcome::Auth => {
                println!("  {url}  ✗ AUTH required (read-only — not authing)");
                last_outcome = format!("{url}: AUTH required");
            }
            Outcome::RelayClosed => {
                println!("  {url}  ✗ error: connection closed by relay");
                last_outcome = format!("{url}: connection closed by relay");
            }
            Outcome::Io(kind) => {
                println!("  {url}  ✗ error: {kind}");
                last_outcome = format!("{url}: {kind}");
            }
            Outcome::Timeout => {
                if events > 0 {
                    // Got events but no EOSE before the per-indexer budget:
                    // surface what we have rather than discarding it.
                    println!(
                        "  {url}  ⚠ timeout after {events} event(s) — using {} follows",
                        follows.len()
                    );
                    session.follows_cache = Some(follows.clone());
                    return Ok(FollowsResult {
                        follows,
                        cached: false,
                        elapsed: start.elapsed(),
                    });
                }
                println!("  {url}  ✗ timeout (no terminal frame)");
                last_outcome = format!("{url}: timeout");
            }
        }
    }

    Err(ReplError::Network(format!(
        "$follows: no indexer returned a kind:3 (last: {last_outcome})"
    )))
}

/// Per-indexer attempt outcome for the kind:3 fetch loop.
enum Outcome {
    Eose,
    Closed(String),
    Auth,
    RelayClosed,
    Io(std::io::ErrorKind),
    Timeout,
}

/// Pull valid 64-hex `p`-tag pubkeys out of a kind:3 event into `out`.
fn collect_p_tags(event: &Value, out: &mut BTreeSet<String>) {
    for tag in event["tags"].as_array().into_iter().flatten() {
        if let Some(arr) = tag.as_array() {
            if arr.first().and_then(Value::as_str) == Some("p") {
                if let Some(pk) = arr.get(1).and_then(Value::as_str) {
                    if pk.len() == 64 && pk.chars().all(|c| c.is_ascii_hexdigit()) {
                        out.insert(pk.to_string());
                    }
                }
            }
        }
    }
}

/// Parse a kind:10002 event into a `MailboxSnapshot`. NIP-65 parsing —
/// preserves the no-personal-relay-filter behaviour (Pitfall §13.4). Used by
/// the `req` tick loop to fold discovery REQ responses into the lifecycle's
/// mailbox cache.
pub fn parse_kind10002(event: &Value) -> Option<(String, MailboxSnapshot)> {
    if event["kind"].as_u64()? != 10002 {
        return None;
    }
    let pk = event["pubkey"].as_str()?.to_string();
    let mut snap = MailboxSnapshot::default();
    for tag in event["tags"].as_array().into_iter().flatten() {
        let arr = match tag.as_array() {
            Some(a) => a,
            None => continue,
        };
        if arr.first().and_then(Value::as_str) != Some("r") {
            continue;
        }
        let url = match arr.get(1).and_then(Value::as_str) {
            Some(u) => normalize_url(u),
            None => continue,
        };
        if url.is_empty() {
            continue;
        }
        let marker = arr.get(2).and_then(Value::as_str);
        match marker {
            Some("read") => snap.read_relays.push(url),
            Some("write") => snap.write_relays.push(url),
            None | Some(_) => snap.both_relays.push(url),
        }
    }
    Some((pk, snap))
}
