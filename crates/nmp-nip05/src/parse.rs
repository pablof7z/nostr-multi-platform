//! Pure NIP-05 identifier shape parsing (no IO).

/// Parse a NIP-05 `name@domain` identifier into its `(name, domain)` parts.
///
/// PURE — no IO. Returns `None` when the input is not a valid NIP-05 shape
/// (missing/empty parts, illegal local-part charset, malformed domain). The
/// domain is lowercased; the `name` local-part is validated against the NIP-05
/// charset (`a-z0-9-_.`). The bare-`_` root identifier (`_@domain`) is accepted.
///
/// SHAPE ONLY — this never performs the `.well-known/nostr.json` lookup; that is
/// [`crate::ResolveNip05Command`] (the dispatch-layer IO step).
#[must_use]
pub fn parse_nip05(_identifier: &str) -> Option<(String, String)> {
    // S2 fills the body (split on '@', validate local-part charset, lowercase
    // domain). Signature is frozen.
    todo!("S2: NIP-05 shape parse (#1804)")
}
