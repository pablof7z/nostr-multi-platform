//! `set-seed <nip05|npub|hex>` — resolve the input to a hex pubkey, clear
//! caches, update the prompt label. The executor for the parsed
//! `Command::SetSeed`.

use nmp_core::nip19::decode_npub;

use crate::ast::SeedInput;
use crate::error::{ReplError, Result};
use crate::nip05;
use crate::session::Session;

pub fn run(session: &mut Session, input: SeedInput) -> Result<()> {
    let hex = match input {
        SeedInput::Hex(h) => h,
        SeedInput::Npub(npub) => decode_npub(&npub)
            .map(|h| h.to_lowercase())
            .map_err(|e| ReplError::Parse(format!("invalid npub '{npub}': {e:?}")))?,
        SeedInput::Nip05(nip) => nip05::resolve(&nip)?,
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ReplError::Other(format!(
            "resolved seed is not valid hex: '{hex}'"
        )));
    }
    println!("  seed: {hex}");
    session.seed_hex = Some(hex);
    // Pitfall §13: set-seed clears the follows cache AND replaces the
    // lifecycle + its mailbox cache with fresh instances (a new identity
    // means the per-session `probed_mailboxes` dedup and any cached
    // kind:10002 are meaningless).
    session.follows_cache = None;
    session.reset_lifecycle();
    Ok(())
}
