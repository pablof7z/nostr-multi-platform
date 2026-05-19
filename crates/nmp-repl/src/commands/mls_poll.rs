//! `mls-poll` — inbox-style sweep of `session.app_relays`.
//!
//! Fetches recent kind:1059 (gift-wrapped Welcomes), kind:445 (group
//! messages), and kind:30443/443 (KeyPackages) and feeds them to the
//! `MarmotService`. We do NOT filter by `#p` author because gift-wraps
//! intentionally hide the recipient; the unwrap step (which only succeeds
//! for our keys) is the actual filter. Duplicates / replays are silent.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nostr::nips::nip19::ToBech32;
use serde_json::json;

use crate::commands::mls_util::require_service;
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

const FETCH_WALL: Duration = Duration::from_secs(8);
/// 7-day inbox window — long enough that a Welcome sent overnight does not
/// fall out before the receiver runs `mls-poll`.
const POLL_WINDOW_SECS: u64 = 7 * 24 * 60 * 60;

pub fn run(session: &mut Session) -> Result<()> {
    let svc = require_service(session)?;
    if session.app_relays.is_empty() {
        return Err(ReplError::Other("no app relays configured".into()));
    }
    let me_pubkey = session
        .mls_keys
        .as_ref()
        .map(|k| k.public_key())
        .ok_or_else(|| ReplError::Other("no identity loaded".into()))?;

    let since = now_secs().saturating_sub(POLL_WINDOW_SECS);
    let mut all_events: HashMap<String, nostr::Event> = HashMap::new();

    // Two queries per relay: an inbox query for ourselves (kind:1059 gift-
    // wraps are `#p`-tagged to recipient; kind:445 is a topic stream) and an
    // open KP query (no author filter so peers' KPs land too).
    let inbox_filter = json!({
        "kinds": [1059, 445],
        "#p": [me_pubkey.to_hex()],
        "since": since,
        "limit": 200,
    });
    let kp_filter = json!({
        "kinds": [30443, 443],
        "since": since,
        "limit": 50,
    });

    for relay in session.app_relays.clone() {
        for (label, filter) in [("inbox", &inbox_filter), ("key-packages", &kp_filter)] {
            let events = publish::fetch_events(&relay, filter, FETCH_WALL);
            println!("  {relay} [{label}]: {} events", events.len());
            for ev_json in events {
                match serde_json::from_value::<nostr::Event>(ev_json) {
                    Ok(ev) => {
                        // De-duplicate by event id across relays.
                        all_events.entry(ev.id.to_hex()).or_insert(ev);
                    }
                    Err(e) => println!("    parse failed: {e}"),
                }
            }
        }
    }

    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    let mut n_total = 0usize;
    let mut n_welcomes = 0usize;
    let mut n_messages = 0usize;
    let mut n_kps = 0usize;

    for (_id, ev) in all_events {
        n_total += 1;
        match ev.kind {
            // KeyPackages — cache silently for later invites.
            nostr::Kind::MlsKeyPackage | nostr::Kind::Custom(30443) => {
                if guard.validate_peer_key_package(&ev).is_ok() {
                    guard.cache_key_package(ev);
                    n_kps += 1;
                }
            }
            // Welcomes — try to unwrap; success means it was sealed for us.
            nostr::Kind::GiftWrap => match guard.unwrap_and_process_welcome(&ev) {
                Ok((welcome, sender)) => {
                    let sender_npub = sender.to_bech32().unwrap_or_else(|_| sender.to_hex());
                    let id_hex = ev.id.to_hex();
                    session
                        .mls_pending_welcomes
                        .insert(id_hex, (ev, welcome.group_name.clone(), sender_npub));
                    n_welcomes += 1;
                }
                Err(_) => {
                    // Not our welcome (or duplicate / already processed).
                    // Silent: an inbox sweep sees many wraps we cannot open.
                }
            },
            // Group messages — feed to MDK; duplicates / wrong-epoch are silent.
            nostr::Kind::MlsGroupMessage => {
                if let Ok(_) = guard.process_message(&ev) {
                    n_messages += 1;
                }
            }
            other => {
                if session.verbose {
                    println!("    ignored kind:{}", other.as_u16());
                }
            }
        }
    }

    println!(
        "  polled: {n_total} events  (welcomes={n_welcomes}, messages={n_messages}, key-packages={n_kps})"
    );
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
