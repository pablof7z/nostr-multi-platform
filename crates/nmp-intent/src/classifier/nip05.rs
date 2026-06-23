//! Rung 4 — NIP-05 SHAPE detection (issue #1804, unified in #1882).
//!
//! Recognizes a `name@domain` / `_@domain` identifier by SHAPE only — no HTTP,
//! no `.well-known/nostr.json` fetch (that is the dispatch layer's job). Pure.
//!
//! The shape decision DEFERS to the canonical [`nmp_nip05::parse_nip05`] so the
//! classifier's accept set is *exactly* the resolver's accept set. Previously
//! this rung hand-rolled its own (looser) shape check that accepted uppercase
//! local parts which `parse_nip05` rejects — so a classified NIP-05 candidate
//! could silently no-op on dispatch. Deferring to the single canonical parser
//! makes the two agree by construction (#1882): every identifier this rung
//! labels `Nip05` is guaranteed to be accepted by `parse_nip05`.
//!
//! Deliberately conservative: a false negative just falls through to free-text
//! search; a false positive would mis-route a query to a reverse-lookup fetch.

/// SHAPE-only NIP-05 detection. Returns the canonical identifier (the trimmed
/// input) when [`nmp_nip05::parse_nip05`] accepts it, else `None`. No IO.
pub(super) fn nip05_shape(input: &str) -> Option<String> {
    // Defer to the canonical parser. We keep the original (trimmed) identifier
    // string as the target payload — the dispatch layer re-parses it with the
    // same `parse_nip05`, so an identifier accepted here is accepted there.
    nmp_nip05::parse_nip05(input).map(|_| input.to_string())
}
