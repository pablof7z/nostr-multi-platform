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
