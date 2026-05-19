//! `load-key <nsec|hex>` — adopt an existing identity. Accepts either a
//! bech32 `nsec1…` or a 64-hex secret key.

use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{Keys, SecretKey};

use crate::error::{ReplError, Result};
use crate::session::Session;

pub fn run(session: &mut Session, input: String) -> Result<()> {
    let sk = if input.starts_with("nsec1") {
        SecretKey::from_bech32(&input).map_err(|e| ReplError::Other(format!("bad nsec: {e}")))?
    } else if input.len() == 64 && input.chars().all(|c| c.is_ascii_hexdigit()) {
        SecretKey::from_hex(&input).map_err(|e| ReplError::Other(format!("bad hex sk: {e}")))?
    } else {
        return Err(ReplError::Other(
            "load-key expects 'nsec1…' or a 64-hex secret key".into(),
        ));
    };

    let keys = Keys::new(sk);
    let pubkey = keys.public_key();
    let npub = pubkey
        .to_bech32()
        .map_err(|e| ReplError::Other(format!("encode npub: {e}")))?;

    println!("  loaded identity:");
    println!("    npub: {npub}");

    session.seed_hex = Some(pubkey.to_hex());
    session.mls_keys = Some(keys);
    // Distinct identity → fresh lifecycle (mailbox-probe dedup is per-id).
    session.reset_lifecycle();
    Ok(())
}
