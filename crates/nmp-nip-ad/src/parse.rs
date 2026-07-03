//! Pure NIP-AD `.well-known/nostr.json?ad=<path>` response parsing (no IO).
//!
//! An ordinary web URL `https://<domain>/<path>` can double as a pointer to
//! Nostr events. Resolving it means fetching
//! `https://<domain>/.well-known/nostr.json?ad=<path>` and reading the entry
//! whose key matches `<path>`. That entry is `{"filter": {<nostr-filter>},
//! "relays": [<relay>, ...]}`; running the filter against those relays yields
//! the matching event(s).
//!
//! CRITICAL: the filter is a LIVE COLLECTION QUERY that may return 0..N events
//! (`https://golf.com/highlights` → many `kind:20` images;
//! `https://trellis.rs/legible` → one `kind:30023` article). We keep the FULL
//! `nostr::Filter` intact — there is no `limit` requirement and NO reduction to
//! a single pointer. Also: the well-known response contains ALL path entries
//! regardless of the `?ad=` query, so we MUST select the entry whose key
//! matches the requested path, never blindly take the first.

/// A resolved NIP-AD entry: a live collection query.
///
/// `filter` is the site-supplied `nostr::Filter` kept verbatim (multi-event
/// capable); `relays` are the site-supplied relays the filter should run
/// against (used once for this resolution — never merged into the outbox/gossip
/// relay model, exactly like an `nevent`/`naddr` relay hint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdResolution {
    /// The full site-supplied filter. May match multiple events.
    pub filter: nostr::Filter,
    /// Site-supplied relays to run the filter against (one-shot relay hints).
    pub relays: Vec<String>,
}

/// Select the entry keyed by `path` from a NIP-AD `.well-known/nostr.json`
/// document and parse it into an [`AdResolution`].
///
/// PURE — no IO. `document` is the already-fetched JSON `Value`; `path` is the
/// requested URL path (with its leading `/`, e.g. `/legible`).
///
/// Returns `Err` when the document is not an object, has no entry for `path`,
/// or the entry is not the `{filter:{…}, relays:[…]}` shape. The filter is
/// deserialized through `nostr::Filter` (rust-nostr), never a hand-rolled
/// parse, so a structurally invalid filter is a bounded error.
pub fn parse_ad_wellknown(
    document: &serde_json::Value,
    path: &str,
) -> Result<AdResolution, String> {
    let object = document.as_object().ok_or_else(|| {
        "NIP-AD document is not a JSON object (not a .well-known/nostr.json ad response)"
            .to_string()
    })?;
    // Select the entry whose KEY matches the requested path. The response
    // carries every path the domain publishes, not just the queried one, so we
    // must not take the first entry.
    let entry = object
        .get(path)
        .ok_or_else(|| format!("NIP-AD document has no entry for path `{path}`"))?;
    parse_ad_entry(entry)
}

/// Parse a single `{filter:{…}, relays:[…]}` entry into an [`AdResolution`].
/// Split out so it is unit-testable without the surrounding object.
fn parse_ad_entry(entry: &serde_json::Value) -> Result<AdResolution, String> {
    let entry = entry
        .as_object()
        .ok_or_else(|| "NIP-AD entry is not a JSON object".to_string())?;

    let filter_value = entry
        .get("filter")
        .ok_or_else(|| "NIP-AD entry is missing the `filter` object".to_string())?;
    if !filter_value.is_object() {
        return Err("NIP-AD entry `filter` is not a JSON object".to_string());
    }
    // Keep the FULL filter — rust-nostr's canonical deserialization, never a
    // hand-rolled parse. A multi-field / no-`limit` filter parses fine.
    let filter: nostr::Filter = serde_json::from_value(filter_value.clone())
        .map_err(|e| format!("NIP-AD entry `filter` is not a valid Nostr filter: {e}"))?;

    let relays_value = entry
        .get("relays")
        .ok_or_else(|| "NIP-AD entry is missing the `relays` array".to_string())?;
    let relays_array = relays_value
        .as_array()
        .ok_or_else(|| "NIP-AD entry `relays` is not an array".to_string())?;
    let mut relays = Vec::with_capacity(relays_array.len());
    for r in relays_array {
        let relay = r
            .as_str()
            .ok_or_else(|| "NIP-AD entry `relays` contains a non-string element".to_string())?;
        relays.push(relay.to_string());
    }
    if relays.is_empty() {
        return Err("NIP-AD entry `relays` is empty".to_string());
    }

    Ok(AdResolution { filter, relays })
}

/// Minimal host-shape validation for a NIP-AD domain: non-empty labels of
/// `[a-z0-9-]` joined by `.`, at least one dot, no leading/trailing/double
/// dots, no leading/trailing hyphen per label. A shape guard, not a registry
/// check — the SSRF host guard + HTTP layer are the real authority.
///
/// PURE. Duplicated from the equivalent nip05 shape check (a trivial ~15-line
/// pure predicate); the security-critical SSRF guard is NOT duplicated (it is
/// shared via `nmp-wellknown-http`).
#[must_use]
pub fn is_valid_domain(domain: &str) -> bool {
    if !domain.contains('.') {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}
