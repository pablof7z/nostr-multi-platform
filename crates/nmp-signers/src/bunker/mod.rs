//! NIP-46 `bunker://` URL parsing.
//!
//! Canonical format per the NIP-46 spec:
//!
//! ```text
//! bunker://<remote-pubkey-hex>?relay=<wss-url>[&relay=...][&secret=...][&perms=...]
//! ```
//!
//! - **`<remote-pubkey-hex>`** is 64 lowercase hex chars (32 bytes).
//! - **`?relay=...`** appears one or more times; each value must be a valid
//!   `ws://` or `wss://` URL.  Per NIP-46 at least one is required for the
//!   connection to be possible.
//! - **`?secret=...`** carries the bunker's challenge secret; the client uses
//!   it in the `connect` RPC.
//! - **`?perms=...`** is a CSV of permission strings (`sign_event:N`,
//!   `nip04_encrypt`, etc.).  Preserved verbatim.
//!
//! Unknown query params are preserved in `BunkerUri::extra` so we don't break
//! round-trip for forward-compatible extensions.
//!
//! ## Why not `nostr::nips::nip46::NostrConnectURI::Bunker`?
//!
//! The `nostr` crate ships a `bunker://` parser behind its `nip46` feature, but
//! its `Bunker` variant models only `{remote_signer_public_key, relays, secret}`.
//! `BunkerUri` is a hardened superset that additionally provides — each pinned
//! by a test in `tests.rs`:
//!
//! - **`MAX_BUNKER_URI_LEN` length cap** — reject adversarial 4 KiB+ input fast
//!   (a fuzz-asserted invariant); upstream has no bound.
//! - **`perms` / `permissions`** — a NIP-46 spec field upstream's `Bunker`
//!   variant does not carry.
//! - **`extra` round-trip** — unknown params preserved for forward-compat
//!   `Display`; upstream silently drops them.
//! - **Case-insensitive scheme + `Zeroizing` connection secret** — upstream
//!   rejects `Bunker://` and exposes the secret as a bare `String`.
//!
//! Switching to the upstream type would regress every one of these. `nip46` is
//! intentionally not enabled on the `nostr` dependency.
//!
//! ## Hardening invariants (fuzz-tested)
//!
//! - URIs longer than [`MAX_BUNKER_URI_LEN`] (4 KiB) are rejected fast.
//! - Pubkey must be exactly 64 lowercase hex chars.  Mixed-case is normalised
//!   to lowercase before validation; upper-case in the source is accepted but
//!   re-emitted as lowercase.
//! - At least one parseable `ws://` / `wss://` relay URL.
//! - All percent-encoded values are decoded; round-trip is via
//!   [`BunkerUri::to_string`].
//! - The parser never panics on adversarial input (fuzz-asserted).

mod parser;

#[cfg(test)]
mod tests;

pub use parser::{parse_bunker_uri, BunkerParseError, BunkerUri, MAX_BUNKER_URI_LEN};
