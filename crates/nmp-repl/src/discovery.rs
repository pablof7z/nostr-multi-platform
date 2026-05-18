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
use crate::ws::{normalize_url, next_text, try_connect, Sock};

const KIND3_WAIT: Duration = Duration::from_secs(10);

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
    let _ = socket.close(None);

    session.follows_cache = Some(follows.clone());

    Ok(FollowsResult {
        follows,
        cached: false,
        elapsed: start.elapsed(),
    })
}

struct IndexerHandle {
    socket: Sock,
}

/// Race connection attempts across the configured indexer set; return the
/// first successful one. v1 dials them sequentially in the listed order.
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
