//! Shared helpers for the MLS / Marmot REPL commands.
//!
//! All MLS commands need the same handful of conversions: hex ↔ `GroupId`,
//! npub ↔ `PublicKey`, "is the service initialised?" guards, and the
//! group-relay → `RelayUrl` mapping. Keeping them here keeps each command
//! module a thin orchestration layer.

use std::sync::{Arc, Mutex};

use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{PublicKey, RelayUrl};

use crate::error::{ReplError, Result};
use crate::session::Session;

pub type Svc = Arc<Mutex<nmp_marmot::service::MarmotService>>;

/// The active MLS keys, or a friendly error pointing at `create-account`.
pub fn require_keys(session: &Session) -> Result<nostr::Keys> {
    session
        .mls_keys
        .clone()
        .ok_or_else(|| ReplError::Other("no identity — run 'create-account' or 'load-key' first".into()))
}

/// The initialised `MarmotService` handle, or a friendly error.
pub fn require_service(session: &Session) -> Result<Svc> {
    session
        .mls_service
        .clone()
        .ok_or_else(|| ReplError::Other("MLS not initialised — run 'mls-init' first".into()))
}

/// App relays as `RelayUrl`s (the MLS group / KeyPackage relay set). Errors
/// if none are configured (Marmot requires at least one group relay).
pub fn relay_urls(session: &Session) -> Result<Vec<RelayUrl>> {
    if session.app_relays.is_empty() {
        return Err(ReplError::Other(
            "no app relays — run 'set-app-relays wss://…' first".into(),
        ));
    }
    let mut out = Vec::with_capacity(session.app_relays.len());
    for u in &session.app_relays {
        let parsed = RelayUrl::parse(u)
            .map_err(|e| ReplError::Other(format!("bad relay url '{u}': {e}")))?;
        out.push(parsed);
    }
    Ok(out)
}

/// Parse an `npub1…` or 64-hex pubkey into a `PublicKey`.
pub fn parse_pubkey(s: &str) -> Result<PublicKey> {
    if s.starts_with("npub1") {
        PublicKey::from_bech32(s).map_err(|e| ReplError::Other(format!("bad npub: {e}")))
    } else {
        PublicKey::from_hex(s).map_err(|e| ReplError::Other(format!("bad pubkey hex: {e}")))
    }
}

/// Decode a group-id hex string into the raw bytes MDK's `GroupId` wraps.
pub fn group_id_bytes(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ReplError::Other(format!(
            "bad group id '{hex}' — expected even-length hex"
        )));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| ReplError::Other(format!("bad group id hex '{hex}'")))
        })
        .collect()
}

/// Short npub form for compact display (`"npub1<first10>…<last6>"`).
///
/// Delegates to the V-33 canonical helper so MLS status lines and every
/// other NMP surface speak the same abbreviated-pubkey dialect.
pub fn short_npub(pk: &PublicKey) -> String {
    nmp_core::display::short_npub(&pk.to_hex())
}

