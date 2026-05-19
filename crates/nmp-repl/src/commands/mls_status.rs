//! `mls-status` — read-only snapshot of the service state. Groups list with
//! member counts, pending welcomes, and cached key packages.

use crate::commands::mls_util::{require_service, short_npub};
use crate::error::{ReplError, Result};
use crate::session::Session;

pub fn run(session: &mut Session) -> Result<()> {
    let svc = require_service(session)?;
    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    let groups = guard
        .get_groups()
        .map_err(|e| ReplError::Other(format!("get_groups: {e}")))?;
    println!("  groups: {}", groups.len());
    for g in &groups {
        let gid_hex = hex(g.mls_group_id.as_slice());
        let members = guard
            .get_members(&g.mls_group_id)
            .map(|m| m.len())
            .unwrap_or(0);
        println!(
            "    [{state:?}] {name}  id={gid_hex}  epoch={epoch}  members={members}",
            state = g.state,
            name = g.name,
            epoch = g.epoch,
        );
    }

    println!("  pending welcomes: {}", session.mls_pending_welcomes.len());
    for (id_hex, (_event, group_name, inviter)) in &session.mls_pending_welcomes {
        println!("    {id_hex}  group={group_name}  from={inviter}");
    }

    let kp = guard.cached_kp_pubkeys();
    println!("  cached key packages: {}", kp.len());
    for pk_hex in &kp {
        let short = nostr::PublicKey::from_hex(pk_hex)
            .map(|pk| short_npub(&pk))
            .unwrap_or_else(|_| pk_hex.clone());
        println!("    {short}");
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
