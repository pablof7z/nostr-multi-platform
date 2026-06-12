//! Relay utilities for NIP-60 wallet-event fetching.
//!
//! NIP-60 wallet events MUST be fetched from the relays listed in the
//! wallet's `relay` tags. If no relay tags are present, fall back to the
//! user's NIP-65 (kind:10002) relays.
//!
//! This module provides synchronous (blocking) helpers that open a raw
//! WebSocket, send a REQ, collect events until EOSE, and close the socket.
//! These are meant to be called from off-actor worker threads (D8).

use std::net::TcpStream;
use std::sync::Once;
use std::time::{Duration, Instant};

use nostr::{Event, Filter, JsonUtil, PublicKey};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

const READ_TIMEOUT: Duration = Duration::from_millis(500);
const EOSE_BUDGET: Duration = Duration::from_secs(10);

static RUSTLS_INIT: Once = Once::new();

fn install_rustls() {
    RUSTLS_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

fn open_socket(url: &str) -> Result<Sock, String> {
    install_rustls();
    let (mut socket, _) = connect(url).map_err(|e| e.to_string())?;
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(READ_TIMEOUT));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(READ_TIMEOUT));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
    Ok(socket)
}

/// Fetch all events matching `filter` from `relay_url`, waiting for EOSE.
///
/// Returns the list of events (may be empty). Errors if the connection fails.
pub fn fetch_events(relay_url: &str, filter: Filter) -> Result<Vec<Event>, String> {
    let mut sock = open_socket(relay_url)?;
    let sub_id = "wallet-fetch";
    let req = format!(r#"["REQ","{sub_id}",{}]"#, filter.as_json());
    sock.send(Message::Text(req)).map_err(|e| e.to_string())?;

    let mut events = Vec::new();
    let deadline = Instant::now() + EOSE_BUDGET;

    loop {
        if Instant::now() > deadline {
            break;
        }
        let msg = match sock.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Ping(p) => {
                let _ = sock.send(Message::Pong(p));
                continue;
            }
            _ => continue,
        };
        let arr: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(arr) = arr.as_array() else { continue };
        match arr.first().and_then(|v| v.as_str()) {
            Some("EVENT") => {
                if let Some(evt_val) = arr.get(2) {
                    if let Ok(evt) = Event::from_json(evt_val.to_string()) {
                        events.push(evt);
                    }
                }
            }
            Some("EOSE") => break,
            _ => {}
        }
    }

    let _ = sock.send(Message::Text(format!(r#"["CLOSE","{sub_id}"]"#)));
    Ok(events)
}

/// Fetch a user's NIP-65 relay list (kind:10002) from purplepag.es.
///
/// purplepag.es is the canonical indexer for relay lists. Returns the write
/// (or read+write) relays so callers can locate the user's content. Falls
/// back to an empty list on any error — the caller handles the fallback.
pub fn fetch_nip65_relays(pubkey: &PublicKey) -> Vec<String> {
    const INDEXER: &str = "wss://purplepag.es";
    let filter = Filter::new()
        .kind(nostr::Kind::RelayList)
        .author(*pubkey)
        .limit(1);

    let events = match fetch_events(INDEXER, filter) {
        Ok(evts) => evts,
        Err(_) => return Vec::new(),
    };

    let Some(event) = events.into_iter().max_by_key(|e| e.created_at) else {
        return Vec::new();
    };

    // NIP-65: `r` tags — ["r", "<url>"] or ["r", "<url>", "read"|"write"].
    // Include relays marked "write" or unmarked. Skip "read"-only.
    let mut relays = Vec::new();
    for tag in event.tags.iter() {
        let slice = tag.as_slice();
        if slice.first().map(|s| s.as_str()) != Some("r") {
            continue;
        }
        let Some(url) = slice.get(1).map(|s| s.as_str()) else { continue };
        let marker = slice.get(2).map(|s| s.as_str());
        if marker == Some("read") {
            continue;
        }
        relays.push(url.to_owned());
    }
    relays
}

/// Publish an event to a relay. Returns the relay's OK/NOTICE response.
pub fn publish_event(relay_url: &str, event: &Event) -> Result<(), String> {
    let mut sock = open_socket(relay_url)?;
    let msg = format!(r#"["EVENT",{}]"#, event.as_json());
    sock.send(Message::Text(msg)).map_err(|e| e.to_string())?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            return Ok(()); // timeout — optimistic
        }
        let msg = match sock.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let text = match msg {
            Message::Text(t) => t,
            _ => continue,
        };
        let arr: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(arr) = arr.as_array() else { continue };
        match arr.first().and_then(|v| v.as_str()) {
            Some("OK") => {
                let accepted = arr.get(2).and_then(|v| v.as_bool()).unwrap_or(true);
                if !accepted {
                    let reason = arr
                        .get(3)
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_owned();
                    return Err(format!("relay rejected event: {reason}"));
                }
                return Ok(());
            }
            Some("NOTICE") => {
                // Log but don't fail — some relays send notices instead of OK.
                continue;
            }
            _ => continue,
        }
    }
}
