//! `mls-send <group_hex> <text...>` — encrypt a TextNote rumor as an MLS
//! ApplicationMessage and publish it as kind:445 to the group's relays
//! (which the REPL treats as `session.app_relays`).

use nmp_marmot::mls_types::GroupId;
use nostr::{EventBuilder, Kind};

use crate::commands::mls_util::{group_id_bytes, require_keys, require_service};
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

pub fn run(session: &mut Session, group_hex: String, text: String) -> Result<()> {
    let keys = require_keys(session)?;
    let svc = require_service(session)?;
    let gid_bytes = group_id_bytes(&group_hex)?;
    let gid = GroupId::from_slice(&gid_bytes);
    if session.app_relays.is_empty() {
        return Err(ReplError::Other("no app relays configured".into()));
    }

    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    // Build the inner rumor (unsigned — MDK signs the kind:445 wrapper).
    let rumor = EventBuilder::new(Kind::TextNote, text.clone()).build(keys.public_key());

    let signed = guard
        .create_message(&gid, rumor)
        .map_err(|e| ReplError::Other(format!("create_message: {e}")))?;
    let event_id = signed.id.to_hex();
    drop(guard); // create_message returns an owned Event — no borrow held.

    println!("  publishing kind:445 message…");
    let (ok, fail) = publish::publish_event(&signed, &session.app_relays);
    println!("  results: {ok} ok / {fail} fail");

    if ok == 0 {
        return Err(ReplError::Other(
            "message failed to publish on every relay".into(),
        ));
    }
    println!("  sent message {event_id}");
    Ok(())
}
