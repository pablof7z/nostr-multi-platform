//! `mls-invite <group_hex> <npub>` — add a member to an existing group.
//!
//! Flow:
//! 1. Ensure the invitee's KeyPackage is in the service cache (auto-fetch).
//! 2. `svc.add_members(gid, &[kp_event])` → `PendingGroupChange` carrying
//!    the kind:445 evolution_event + kind:444 Welcome rumor.
//! 3. Gift-wrap the Welcome rumor for the invitee (NIP-59 kind:1059).
//! 4. Publish kind:445 (group relays) + kind:1059 (invitee inbox = app
//!    relays here) and call `pending.commit()`.

use std::time::Duration;

use nmp_marmot::mls_types::GroupId;
use serde_json::json;

use crate::commands::mls_util::{group_id_bytes, parse_pubkey, require_keys, require_service};
// `require_keys` is the friendly preflight error when no identity is loaded.
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

const FETCH_WALL: Duration = Duration::from_secs(5);

pub fn run(session: &mut Session, group_hex: String, npub: String) -> Result<()> {
    let _keys = require_keys(session)?;
    let svc = require_service(session)?;
    let invitee = parse_pubkey(&npub)?;
    let gid_bytes = group_id_bytes(&group_hex)?;
    let gid = GroupId::from_slice(&gid_bytes);

    if session.app_relays.is_empty() {
        return Err(ReplError::Other("no app relays configured".into()));
    }

    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    // ── 1. Ensure the invitee's KeyPackage is cached ─────────────────────
    let mut kp_events = guard.cached_key_packages(&[invitee]);
    if kp_events.is_empty() {
        println!("  no cached key package for {npub} — fetching…");
        let filter = json!({
            "kinds": [30443, 443],
            "authors": [invitee.to_hex()],
            "limit": 4,
        });
        for relay in session.app_relays.clone() {
            let events = publish::fetch_events(&relay, &filter, FETCH_WALL);
            for ev_json in events {
                if let Ok(ev) = serde_json::from_value::<nostr::Event>(ev_json) {
                    if guard.validate_peer_key_package(&ev).is_ok() {
                        guard.cache_key_package(ev);
                    }
                }
            }
        }
        kp_events = guard.cached_key_packages(&[invitee]);
        if kp_events.is_empty() {
            return Err(ReplError::Other(format!(
                "no key package found for {npub} on app relays"
            )));
        }
        println!("  fetched + cached {} kp event(s)", kp_events.len());
    }

    // ── 2. add_members → PendingGroupChange ──────────────────────────────
    let pending = guard
        .add_members(&gid, &kp_events)
        .map_err(|e| ReplError::Other(format!("add_members: {e}")))?;
    let evolution_event = pending.evolution_event.clone();
    let welcome_rumors = pending.welcome_rumors.clone();

    if welcome_rumors.is_empty() {
        // Bail-out: clear the pending commit so the group is not wedged.
        let _ = pending.clear();
        return Err(ReplError::Other(
            "add_members returned no welcome rumors (unexpected)".into(),
        ));
    }

    // ── 3. Gift-wrap each Welcome rumor for the invitee ──────────────────
    // `add_members` produces ONE welcome per added member. We added a
    // single peer here so there is exactly one rumor; wrap it for `invitee`.
    let welcome_rumor = welcome_rumors[0].clone();
    let gift_wrap = match guard.wrap_welcome(&invitee, welcome_rumor) {
        Ok(ev) => ev,
        Err(e) => {
            // Clear on the failure path (mdk-api §7.7) — we did not publish.
            let _ = pending.clear();
            return Err(ReplError::Other(format!("wrap_welcome: {e}")));
        }
    };

    // ── 4. Publish kind:445 evolution_event + kind:1059 gift-wrap ────────
    // We keep the mutex guard held through publish: `pending` borrows
    // `&MarmotService` from the guard, so we cannot drop it until after
    // `commit()`. The REPL is single-threaded, so no contention.
    println!("  publishing kind:445 (evolution_event)…");
    let (ok_evo, fail_evo) = publish::publish_event(&evolution_event, &session.app_relays);
    println!("  publishing kind:1059 (gift-wrapped welcome) to {npub}…");
    let (ok_gw, fail_gw) = publish::publish_event(&gift_wrap, &session.app_relays);
    println!(
        "  results: evolution {ok_evo} ok / {fail_evo} fail, welcome {ok_gw} ok / {fail_gw} fail"
    );

    // Commit the local pending commit. mdk-api §7.7: if the evolution_event
    // never reached any relay we should `clear()` instead — but our soft
    // "no negative reply == success" heuristic already keeps the group
    // unwedged in the happy case. A strict caller could check `ok_evo > 0`.
    if ok_evo == 0 {
        let _ = pending.clear();
        return Err(ReplError::Other(
            "evolution_event failed to publish on every relay — cleared pending commit".into(),
        ));
    }
    pending
        .commit()
        .map_err(|e| ReplError::Other(format!("commit add_members: {e}")))?;

    println!("  invited {npub} to group {group_hex}");
    Ok(())
}
