//! `mls-init` — build the `MarmotService` and publish KeyPackages.
//!
//! Requires a prior `create-account` / `load-key` (so we have `mls_keys`).
//! Uses a persistent unencrypted SQLite store at
//! `~/.cache/nmp-repl/mls/<pubkey_hex>.sqlite` so MLS state (key packages,
//! group ratchet trees, welcomed groups) survives across REPL sessions. This
//! is critical for the two-terminal test: Bob can close his session after
//! `mls-init` and reopen it later; his key material is still present when
//! Alice's welcome arrives via `mls-poll`.
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

    let pubkey_hex = keys.public_key().to_hex();
    let db_path = mls_db_path(&pubkey_hex)?;
    println!("  mls db: {db_path}");

    let storage = MdkSqliteStorage::new_unencrypted(&db_path)
        .map_err(|e| ReplError::Other(format!("open mls storage at {db_path}: {e}")))?;
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
    println!("  mls service initialised (persistent store)");
    Ok(())
}

/// `~/.cache/nmp-repl/mls/<pubkey_hex>.sqlite`
fn mls_db_path(pubkey_hex: &str) -> Result<String> {
    let base = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            std::path::PathBuf::from(xdg)
        } else {
            home_cache()?
        }
    } else {
        home_cache()?
    };
    let dir = base.join("nmp-repl").join("mls");
    std::fs::create_dir_all(&dir)
        .map_err(|e| ReplError::Other(format!("create mls cache dir: {e}")))?;
    Ok(dir
        .join(format!("{pubkey_hex}.sqlite"))
        .to_string_lossy()
        .into_owned())
}

fn home_cache() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| ReplError::Other("$HOME not set".into()))?;
    Ok(std::path::PathBuf::from(home).join(".cache"))
}
