//! `mls-messages <group_hex>` — print the decrypted message history in
//! chronological order (oldest first), one line per message.

use nmp_marmot::mls_types::GroupId;

use crate::commands::mls_util::{group_id_bytes, require_service, short_npub};
use crate::error::{ReplError, Result};
use crate::session::Session;

pub fn run(session: &mut Session, group_hex: String) -> Result<()> {
    let svc = require_service(session)?;
    let gid_bytes = group_id_bytes(&group_hex)?;
    let gid = GroupId::from_slice(&gid_bytes);

    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    let mut msgs = guard
        .get_messages(&gid)
        .map_err(|e| ReplError::Other(format!("get_messages: {e}")))?;
    msgs.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    if msgs.is_empty() {
        println!("  (no messages)");
        return Ok(());
    }

    for m in &msgs {
        println!(
            "  [{ts}] [{sender}] {content}",
            ts = m.created_at.as_secs(),
            sender = short_npub(&m.pubkey),
            content = m.content,
        );
    }
    Ok(())
}
