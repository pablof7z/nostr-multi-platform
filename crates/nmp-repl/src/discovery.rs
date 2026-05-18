//! Phase A (kind:3) + Phase B (kind:10002) discovery. Mirrors
//! `outbox_perf.rs::phase_a_fetch_kind3` and `phase_b_fetch_mailboxes`.
//! Single-indexer, sequential — first success wins; multi-indexer fan-out
//! is a v2 follow-up per §12.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use nmp_core::planner::MailboxSnapshot;
use serde_json::{json, Value};
use tungstenite::Message;

use crate::error::{ReplError, Result};
use crate::session::Session;
use crate::ws::{normalize_url, next_text, try_connect, Sock};

const KIND3_WAIT: Duration = Duration::from_secs(10);
const KIND10002_WAIT: Duration = Duration::from_secs(15);

/// Result of phase A — the follows set and the elapsed wall time. If the
/// cache was already populated, returns the cached set with `cached=true`.
pub struct PhaseAResult {
    pub follows: BTreeSet<String>,
    pub cached: bool,
    pub elapsed: Duration,
}

/// Result of phase B — the count of newly-fetched authors and elapsed time.
///
/// `queried` is the total number of authors we needed a kind:10002 for;
/// `have_after` is how many of those ended up with a cached mailbox. The
/// gap (`queried - have_after`) is the load-bearing unroutable surface
/// (design pitfall §13.8) — never collapse the two numbers.
pub struct PhaseBResult {
    pub fetched: usize,
    pub already_cached: usize,
    pub queried: usize,
    pub have_after: usize,
    pub elapsed: Duration,
}

/// Phase A — fetch kind:3 follows for the session seed via the first
/// reachable indexer. Populates `session.follows_cache`.
pub fn phase_a(session: &mut Session) -> Result<PhaseAResult> {
    let start = Instant::now();

    if let Some(cached) = &session.follows_cache {
        return Ok(PhaseAResult {
            follows: cached.clone(),
            cached: true,
            elapsed: start.elapsed(),
        });
    }
    let seed = session
        .seed_hex
        .clone()
        .ok_or_else(|| ReplError::Variable("$follows requires a seed; run `set-seed <nip05|npub>` first".to_string()))?;

    let indexer = first_reachable_indexer(&session.indexer_relays)?;
    let mut socket = indexer.socket;

    let sub_id = "follows-1";
    let req = json!([
        "REQ",
        sub_id,
        { "kinds": [3], "authors": [seed], "limit": 1 }
    ])
    .to_string();
    socket
        .send(Message::Text(req))
        .map_err(|e| ReplError::Network(format!("send REQ kind:3: {e}")))?;

    let deadline = Instant::now() + KIND3_WAIT;
    let mut follows: BTreeSet<String> = BTreeSet::new();
    while Instant::now() < deadline {
        match next_text(&mut socket) {
            None => continue,
            Some(text) => {
                if text.is_empty() {
                    // socket closed or error frame — bail.
                    break;
                }
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches!(v[0].as_str(), Some("EVENT")) && v[1].as_str() == Some(sub_id) {
                    if let Some(event) = v.get(2) {
                        for tag in event["tags"].as_array().into_iter().flatten() {
                            if let Some(arr) = tag.as_array() {
                                if arr.first().and_then(Value::as_str) == Some("p") {
                                    if let Some(pk) = arr.get(1).and_then(Value::as_str) {
                                        if pk.len() == 64
                                            && pk.chars().all(|c| c.is_ascii_hexdigit())
                                        {
                                            follows.insert(pk.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(v[0].as_str(), Some("EOSE")) && v[1].as_str() == Some(sub_id) {
                    break;
                }
            }
        }
    }
    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));

    // Store the open socket back on the indexer handle so phase B can reuse it.
    session.follows_cache = Some(follows.clone());

    Ok(PhaseAResult {
        follows,
        cached: false,
        elapsed: start.elapsed(),
    })
}

/// Phase B — fetch kind:10002 mailbox events for any author missing from
/// `session.mailbox_cache`. Authors that don't publish kind:10002 stay
/// missing and surface later as `unroutable_authors`.
pub fn phase_b(session: &mut Session, authors: &BTreeSet<String>) -> Result<PhaseBResult> {
    let start = Instant::now();

    // Partition cached vs. needed.
    let mut needed: Vec<String> = Vec::new();
    let mut already_cached = 0usize;
    for pk in authors {
        if session.mailbox_cache.contains_key(pk) {
            already_cached += 1;
        } else {
            needed.push(pk.clone());
        }
    }
    let queried = authors.len();
    if needed.is_empty() {
        return Ok(PhaseBResult {
            fetched: 0,
            already_cached,
            queried,
            have_after: authors
                .iter()
                .filter(|pk| session.mailbox_cache.contains_key(*pk))
                .count(),
            elapsed: start.elapsed(),
        });
    }

    let indexer = first_reachable_indexer(&session.indexer_relays)?;
    let mut socket = indexer.socket;

    let sub_id = "mailboxes-1";
    let req = json!([
        "REQ",
        sub_id,
        { "kinds": [10002], "authors": needed }
    ])
    .to_string();
    socket
        .send(Message::Text(req))
        .map_err(|e| ReplError::Network(format!("send REQ kind:10002: {e}")))?;

    let deadline = Instant::now() + KIND10002_WAIT;
    let mut out: BTreeMap<String, MailboxSnapshot> = BTreeMap::new();
    while Instant::now() < deadline {
        match next_text(&mut socket) {
            None => continue,
            Some(text) => {
                if text.is_empty() {
                    break;
                }
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches!(v[0].as_str(), Some("EVENT")) && v[1].as_str() == Some(sub_id) {
                    if let Some(event) = v.get(2) {
                        if let Some((pk, snap)) = parse_kind10002(event) {
                            // newest-wins approximation
                            out.insert(pk, snap);
                        }
                    }
                }
                if matches!(v[0].as_str(), Some("EOSE")) && v[1].as_str() == Some(sub_id) {
                    break;
                }
            }
        }
    }
    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    let _ = socket.close(None);

    let fetched = out.len();
    for (pk, snap) in out {
        session.mailbox_cache.insert(pk, snap);
    }
    let have_after = authors
        .iter()
        .filter(|pk| session.mailbox_cache.contains_key(*pk))
        .count();
    Ok(PhaseBResult {
        fetched,
        already_cached,
        queried,
        have_after,
        elapsed: start.elapsed(),
    })
}

struct IndexerHandle {
    socket: Sock,
}

/// Race connection attempts across the configured indexer set; return the
/// first successful one. v1 dials them sequentially in the listed order —
/// the §12 "multi-indexer fan-out" optimisation is out of scope.
fn first_reachable_indexer(indexers: &[String]) -> Result<IndexerHandle> {
    if indexers.is_empty() {
        return Err(ReplError::Network(
            "no indexer relays configured (try `set-indexer wss://purplepag.es`)".to_string(),
        ));
    }
    for url in indexers {
        if let Some(socket) = try_connect(url) {
            return Ok(IndexerHandle { socket });
        }
    }
    Err(ReplError::Network(format!(
        "no indexer reachable (tried {})",
        indexers.join(", ")
    )))
}

/// Parse a kind:10002 event into a `MailboxSnapshot`. Lifted verbatim from
/// `outbox_perf.rs:384` — preserves no-personal-relay-filter behaviour
/// (Pitfall §13.4 in the design doc).
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
