//! `mls-accept [welcome_hex]` — finalize joining a group.
//!
//! Without an arg: lists pending welcomes (from the last `mls-poll`).
//! With an arg: accepts the welcome identified by its gift-wrap event id
//! hex, runs the mandatory post-join `self_update` (MIP-02), and publishes
//! the resulting evolution_event.

use crate::commands::mls_util::require_service;
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

pub fn run(session: &mut Session, welcome_hex: Option<String>) -> Result<()> {
    let svc = require_service(session)?;

    // List mode.
    let target = match welcome_hex {
        None => {
            if session.mls_pending_welcomes.is_empty() {
                println!("  no pending welcomes (run 'mls-poll' first)");
                return Ok(());
            }
            println!("  pending welcomes:");
            for (id_hex, (_ev, group_name, inviter)) in &session.mls_pending_welcomes {
                println!("    {id_hex}  group={group_name}  from={inviter}");
            }
            println!("  run 'mls-accept <id_hex>' (or 'mls-accept first' to take any)");
            return Ok(());
        }
        Some(s) if s == "first" => {
            // Convenience: take any pending welcome.
            session
                .mls_pending_welcomes
                .keys()
                .next()
                .cloned()
                .ok_or_else(|| ReplError::Other("no pending welcomes".into()))?
        }
        Some(s) => s,
    };

    let (gift_wrap, group_name, inviter) = session
        .mls_pending_welcomes
        .remove(&target)
        .ok_or_else(|| {
            ReplError::Other(format!("no pending welcome with id '{target}'"))
        })?;

    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    // Re-unwrap to get the Welcome handle. We do not retain the handle from
    // `mls-poll` because it borrows the service — re-running is cheap and
    // idempotent (`process_welcome` upserts on wrapper id).
    let (welcome, _sender) = guard
        .unwrap_and_process_welcome(&gift_wrap)
        .map_err(|e| ReplError::Other(format!("unwrap_and_process_welcome: {e}")))?;
    let group_id = welcome.mls_group_id.clone();
    let group_id_hex = hex(group_id.as_slice());

    guard
        .accept_welcome(&welcome)
        .map_err(|e| ReplError::Other(format!("accept_welcome: {e}")))?;
    println!("  accepted welcome");
    println!("    group:    {group_name}");
    println!("    from:     {inviter}");
    println!("    group_id: {group_id_hex}");

    // MIP-02: post-join self-update is mandatory. Publish the evolution
    // event back to the group relays so the inviter advances epoch.
    let pending = guard
        .self_update(&group_id)
        .map_err(|e| ReplError::Other(format!("self_update (post-join): {e}")))?;
    let evolution_event = pending.evolution_event.clone();
    println!("  publishing post-join self_update (kind:445)…");
    let (ok, fail) = publish::publish_event(&evolution_event, &session.app_relays);
    println!("  results: {ok} ok / {fail} fail");
    if ok == 0 {
        let _ = pending.clear();
        return Err(ReplError::Other(
            "post-join self_update failed to publish; cleared pending commit. Try 'mls-accept' again later.".into(),
        ));
    }
    pending
        .commit()
        .map_err(|e| ReplError::Other(format!("commit self_update: {e}")))?;

    println!("  joined group {group_id_hex}");
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
