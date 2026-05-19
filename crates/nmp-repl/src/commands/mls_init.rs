//! `mls-init` — build the in-memory `MarmotService` and publish KeyPackages.
//!
//! Requires a prior `create-account` / `load-key` (so we have `mls_keys`).
//! Uses an in-memory SQLite MLS store (`MdkSqliteStorage::new_in_memory`) so
//! the REPL is single-process and stateless — perfect for the two-instance
//! end-to-end test.
//!
//! # USAGE — two-party MLS round-trip via two REPL instances
//!
//! ```text
//! # Alice (REPL #1)
//! > set-app-relays wss://relay.damus.io
//! > create-account alice
//! > mls-init
//! > mls-create TestGroup
//! > mls-invite <group_hex> <bob_npub>
//!
//! # Bob (REPL #2)
//! > set-app-relays wss://relay.damus.io
//! > load-key <bob_nsec>          # or create-account bob
//! > mls-init
//! > mls-poll                      # picks up Alice's gift-wrapped Welcome
//! > mls-accept                    # accepts the first pending Welcome
//! > mls-poll                      # picks up any messages
//!
//! # Alice
//! > mls-send <group_hex> hello bob
//!
//! # Bob
//! > mls-poll
//! > mls-messages <group_hex>      # decrypted history
//! ```
//!
//! Both instances must share at least one app relay (the Welcome /
//! evolution_event / message round-trip flows through it).

use std::sync::{Arc, Mutex};

use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_marmot::service::MarmotService;

use crate::commands::mls_util::{relay_urls, require_keys};
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

pub fn run(session: &mut Session) -> Result<()> {
    let keys = require_keys(session)?;
    let relays = relay_urls(session)?;

    let storage = MdkSqliteStorage::new_in_memory()
        .map_err(|e| ReplError::Other(format!("init in-memory mls storage: {e}")))?;
    let svc = MarmotService::from_storage(storage, keys, Default::default());

    let pub_result = svc
        .publish_key_package(relays)
        .map_err(|e| ReplError::Other(format!("publish_key_package: {e}")))?;

    println!("  key package: d_tag = {}", pub_result.d_tag);
    println!("  publishing kind:30443 (current spec)…");
    let (ok_a, fail_a) = publish::publish_event(&pub_result.event_30443, &session.app_relays);
    println!("  publishing kind:443 (legacy)…");
    let (ok_b, fail_b) = publish::publish_event(&pub_result.event_443, &session.app_relays);
    println!(
        "  results: kind:30443 {ok_a} ok / {fail_a} fail, kind:443 {ok_b} ok / {fail_b} fail"
    );

    session.mls_service = Some(Arc::new(Mutex::new(svc)));
    println!("  mls service initialised (in-memory store)");
    Ok(())
}
