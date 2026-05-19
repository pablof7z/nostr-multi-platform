//! `mls-fetch-kp <npub>` — REQ kind:30443 events for `<npub>` from every
//! app relay, parse each into a `nostr::Event`, and feed it to
//! `MarmotService::cache_key_package`. After this, `mls-invite <group> <npub>`
//! can pull the cached KeyPackage straight out of the service.

use std::time::Duration;

use serde_json::json;

use crate::commands::mls_util::{parse_pubkey, require_service};
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

const FETCH_WALL: Duration = Duration::from_secs(5);

pub fn run(session: &mut Session, npub: String) -> Result<()> {
    let svc = require_service(session)?;
    let pk = parse_pubkey(&npub)?;
    if session.app_relays.is_empty() {
        return Err(ReplError::Other("no app relays configured".into()));
    }

    // Fetch BOTH kind:30443 (current spec, NIP-33) and legacy kind:443 —
    // peers may publish only one of the two during the dual-publish window.
    let filter = json!({
        "kinds": [30443, 443],
        "authors": [pk.to_hex()],
        "limit": 4,
    });

    let mut cached = 0usize;
    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    for relay in session.app_relays.clone() {
        let events = publish::fetch_events(&relay, &filter, FETCH_WALL);
        println!("  {relay}: fetched {} kp events", events.len());
        for ev_json in events {
            match serde_json::from_value::<nostr::Event>(ev_json) {
                Ok(ev) => match guard.validate_peer_key_package(&ev) {
                    Ok(()) => {
                        guard.cache_key_package(ev);
                        cached += 1;
                    }
                    Err(e) => {
                        println!("    skipped one (validate: {e})");
                    }
                },
                Err(e) => {
                    println!("    skipped one (parse: {e})");
                }
            }
        }
    }

    if cached == 0 {
        return Err(ReplError::Other(format!(
            "no usable key packages found for {npub}"
        )));
    }
    println!("  cached {cached} key package event(s) for {npub}");
    Ok(())
}
