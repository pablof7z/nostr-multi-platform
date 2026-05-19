//! `create-account [name]` — generate a fresh Nostr identity, publish
//! a kind:0 profile + kind:10002 relay list to `session.app_relays`, and
//! store the keys in the session for the MLS commands.
//!
//! NOT persisted: a process restart drops the identity. Two REPLs sharing a
//! relay set + each running `create-account` give Alice + Bob.

use nostr::nips::nip19::ToBech32;
use nostr::{EventBuilder, Keys, Kind, Tag};

use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

pub fn run(session: &mut Session, name: Option<String>) -> Result<()> {
    if session.app_relays.is_empty() {
        return Err(ReplError::Other(
            "no app relays — run 'set-app-relays wss://…' first".into(),
        ));
    }

    let keys = Keys::generate();
    let pubkey = keys.public_key();
    let display = name.unwrap_or_else(|| "repl-user".to_string());

    // Kind:0 profile.
    let profile_json = serde_json::json!({
        "name": display,
        "display_name": display,
    })
    .to_string();
    let kind0 = EventBuilder::new(Kind::Metadata, profile_json)
        .sign_with_keys(&keys)
        .map_err(|e| ReplError::Other(format!("sign kind:0: {e}")))?;

    // Kind:10002 relay list — every app_relay tagged as a "write" relay
    // (`["r", url, "write"]`). Outbox model: kind:10002 is where readers
    // look up where to fetch this author's events.
    let relay_tags: Vec<Tag> = session
        .app_relays
        .iter()
        .map(|u| Tag::parse(["r", u, "write"]).unwrap())
        .collect();
    let kind10002 = EventBuilder::new(Kind::RelayList, "")
        .tags(relay_tags)
        .sign_with_keys(&keys)
        .map_err(|e| ReplError::Other(format!("sign kind:10002: {e}")))?;

    let npub = pubkey
        .to_bech32()
        .map_err(|e| ReplError::Other(format!("encode npub: {e}")))?;
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|e| ReplError::Other(format!("encode nsec: {e}")))?;

    println!("  identity:");
    println!("    npub: {npub}");
    println!("    nsec: {nsec}");

    println!("  publishing kind:0 (profile)…");
    let (ok0, fail0) = publish::publish_event(&kind0, &session.app_relays);
    println!("  publishing kind:10002 (relay list)…");
    let (ok1, fail1) = publish::publish_event(&kind10002, &session.app_relays);
    println!(
        "  results: kind:0 {ok0} ok / {fail0} fail, kind:10002 {ok1} ok / {fail1} fail"
    );

    // Adopt the identity for subsequent MLS + REQ commands. Setting
    // `seed_hex` so the prompt + read-only `req` reflect the active id.
    session.seed_hex = Some(pubkey.to_hex());
    session.mls_keys = Some(keys);
    // A new identity invalidates the lifecycle's mailbox-probe dedup.
    session.reset_lifecycle();
    Ok(())
}
