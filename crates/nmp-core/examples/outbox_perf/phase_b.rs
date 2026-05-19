//! Phase B: fetch kind:10002 relay lists for the follow set.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use nmp_core::planner::MailboxSnapshot;
use serde_json::{json, Value};
use tungstenite::Message;

use super::transport::{next_text, Sock};

/// Duration budget for the kind:10002 batch fetch.
const KIND10002_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// REQ kind:10002 for all `follows` over an existing `socket`.
///
/// Returns a map of hex-pubkey → `MailboxSnapshot`.
pub fn phase_b_fetch_mailboxes(
    socket: &mut Sock,
    follows: &BTreeSet<String>,
) -> BTreeMap<String, MailboxSnapshot> {
    let sub_id = "mailboxes-1";
    let authors: Vec<String> = follows.iter().cloned().collect();
    let req = json!([
        "REQ",
        sub_id,
        { "kinds": [10002], "authors": authors }
    ])
    .to_string();
    socket.send(Message::Text(req)).expect("send REQ");

    let deadline = Instant::now() + KIND10002_WAIT;
    let mut out: BTreeMap<String, MailboxSnapshot> = BTreeMap::new();
    while Instant::now() < deadline {
        match next_text(socket) {
            None => continue,
            Some(text) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches!(v[0].as_str(), Some("EVENT")) && v[1].as_str() == Some(sub_id) {
                    if let Some(event) = v.get(2) {
                        if let Some((pk, snap)) = parse_kind10002(event) {
                            // newest-wins approximation
                            // NOTE: see T-parse-kind10002-timestamp — this uses last-received
                            // ordering rather than event `created_at`; tracked as a separate task.
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
    out
}

/// Parse a kind:10002 event into a `MailboxSnapshot`.
///
/// No personal-relay URL filtering: the greedy max-coverage selector in
/// `apply_selection` is the defense. Personal relays have coverage=1 by
/// construction and lose every tiebreak against real shared relays.
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

fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if !(trimmed.starts_with("wss://") || trimmed.starts_with("ws://")) {
        return String::new();
    }
    let mut s = trimmed.to_string();
    while s.ends_with('/') && s.matches('/').count() > 2 {
        s.pop();
    }
    if s.ends_with('/') {
        s.pop();
    }
    s
}
