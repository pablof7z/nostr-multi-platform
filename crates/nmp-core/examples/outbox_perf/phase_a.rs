//! Phase A: fetch the seed author's kind:3 contact list from the indexer.

use std::collections::BTreeSet;
use std::time::Instant;

use serde_json::{json, Value};
use tungstenite::Message;

use super::transport::{connect, next_text, Sock};

/// Duration budget for the kind:3 fetch.
const KIND3_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Connect to `indexer`, REQ the seed's kind:3, and return the open socket
/// plus the parsed follow set (hex pubkeys).
pub fn phase_a_fetch_kind3(indexer: &str, seed_hex: &str) -> (Sock, BTreeSet<String>) {
    let mut socket = connect(indexer);
    let sub_id = "follows-1";
    let req = json!([
        "REQ",
        sub_id,
        { "kinds": [3], "authors": [seed_hex], "limit": 1 }
    ])
    .to_string();
    socket.send(Message::Text(req)).expect("send REQ");

    let deadline = Instant::now() + KIND3_WAIT;
    let mut follows: BTreeSet<String> = BTreeSet::new();
    while Instant::now() < deadline {
        match next_text(&mut socket) {
            Some(text) => {
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
            None => continue,
        }
    }
    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    (socket, follows)
}
