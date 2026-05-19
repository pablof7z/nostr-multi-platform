//! `mls-create <name>` — create a solo MLS group (no members). Use
//! `mls-invite` afterwards to add peers. The group's relays are
//! `session.app_relays` so peers know where to read kind:445 traffic from.

use nmp_marmot::mls_types::NostrGroupConfigData;

use crate::commands::mls_util::{relay_urls, require_keys, require_service};
use crate::error::{ReplError, Result};
use crate::session::Session;

pub fn run(session: &mut Session, name: String) -> Result<()> {
    let keys = require_keys(session)?;
    let svc = require_service(session)?;
    let relays = relay_urls(session)?;

    let guard = svc
        .lock()
        .map_err(|_| ReplError::Other("mls service mutex poisoned".into()))?;

    let config = NostrGroupConfigData::new(
        name.clone(),
        "REPL-created group".to_string(),
        None,
        None,
        None,
        relays,
        vec![keys.public_key()],
    );

    let (group, pending) = guard
        .create_group(Vec::new(), config)
        .map_err(|e| ReplError::Other(format!("create_group: {e}")))?;
    let group_id_hex = pending.group_id_hex();
    let n_welcomes = pending.welcome_rumors.len();

    // Solo group: no welcomes; commit the create immediately. (mdk-api §7.3
    // — `create_group` produces a pending commit that must be merged
    // exactly once.)
    pending
        .commit()
        .map_err(|e| ReplError::Other(format!("commit create_group: {e}")))?;

    println!("  group created");
    println!("    name:     {}", group.name);
    println!("    group_id: {group_id_hex}");
    println!("    epoch:    {}", group.epoch);
    println!("    welcomes pending publish: {n_welcomes}");
    Ok(())
}
