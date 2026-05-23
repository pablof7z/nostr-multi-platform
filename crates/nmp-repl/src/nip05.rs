//! NIP-05 resolver. Sync HTTPS GET to `/.well-known/nostr.json?name=<local>`
//! and pluck `names[<local>]` from the JSON. See `docs/design/nmp-repl.md` §11
//! step 4.

use std::time::Duration;

use crate::error::{ReplError, Result};

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolve `localpart@domain` to a 64-hex pubkey via the domain's
/// `/.well-known/nostr.json?name=<localpart>` endpoint.
#[must_use]
pub fn resolve(nip05: &str) -> Result<String> {
    let (local, domain) = nip05
        .split_once('@')
        .ok_or_else(|| ReplError::Nip05(format!("'{nip05}' is not a 'name@domain' string")))?;
    if local.is_empty() || domain.is_empty() {
        return Err(ReplError::Nip05(format!(
            "'{nip05}' is not a valid 'name@domain' string"
        )));
    }

    let url = format!("https://{domain}/.well-known/nostr.json?name={local}");

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(HTTP_TIMEOUT)
        .timeout_read(HTTP_TIMEOUT)
        .timeout_write(HTTP_TIMEOUT)
        .build();

    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| ReplError::Nip05(format!("GET {url}: {e}")))?;

    let body: serde_json::Value = resp
        .into_json()
        .map_err(|e| ReplError::Nip05(format!("{url}: invalid JSON: {e}")))?;

    let names = body
        .get("names")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ReplError::Nip05(format!("{url}: missing 'names' object")))?;

    let hex = names
        .get(local)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ReplError::Nip05(format!("{url}: no entry for '{local}' in 'names'"))
        })?;

    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ReplError::Nip05(format!(
            "{url}: '{local}' resolved to invalid hex: {hex}"
        )));
    }
    Ok(hex.to_lowercase())
}
