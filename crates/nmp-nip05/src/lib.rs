//! NIP-05 reverse resolver (issue #1804).
//!
//! Resolves a NIP-05 `name@domain` identifier into a Nostr pubkey by fetching
//! the domain's `https://<domain>/.well-known/nostr.json?name=<name>` document
//! and reading `names[<name>]`.
//!
//! # Two layers
//!
//! * [`parse_nip05`] — PURE shape validation: split `name@domain`, lowercase the
//!   domain, validate the `name` charset (NIP-05 local-part). No IO. The
//!   input-intent classifier ([`nmp_intent::classify`]) calls this in its pure
//!   pass to emit `InputIntentTarget::Nip05 { identifier }`.
//! * [`ResolveNip05Command`] — the IO layer: a generic
//!   [`nmp_core::substrate::ProtocolCommand`] the dispatch layer enqueues. Its
//!   [`ProtocolCommand::run`] spawns a blocking worker that performs the HTTP
//!   GET (mirroring the nmp-nip57 LNURL fetcher) and posts a follow-up
//!   `ActorCommand` carrying the resolved pubkey (landed through the
//!   `RefNamespace::Profile` resolve-ref seam). HTTP lives behind the `native`
//!   feature.

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};

pub mod parse;

pub use parse::parse_nip05;

/// Resolve a NIP-05 `name@domain` identifier to a pubkey via the domain's
/// `.well-known/nostr.json` endpoint.
///
/// Constructed by the dispatch layer when [`nmp_intent::classify`] returns an
/// `InputIntentTarget::Nip05`. The `run` worker performs the HTTP GET off the
/// actor thread and posts the resolved pubkey back as a follow-up
/// `ActorCommand` (S2 fills the body).
#[derive(Debug)]
pub struct ResolveNip05Command {
    /// The NIP-05 local-part (`name` in `name@domain`), already shape-validated
    /// and lowercased by [`parse_nip05`].
    pub name: String,
    /// The NIP-05 domain (`domain` in `name@domain`), already lowercased.
    pub domain: String,
    /// Registry-minted correlation id when this resolution originates from a
    /// host-visible action (so a spinner can clear on terminal stages). `None`
    /// for a direct caller with no spinner.
    pub correlation_id: Option<String>,
}

impl ProtocolCommand for ResolveNip05Command {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        // S2 fills the body: spawn a blocking worker that GETs
        // `https://<domain>/.well-known/nostr.json?name=<name>`, reads
        // `names[name]`, and posts a follow-up `ActorCommand` landing the
        // resolved pubkey through the `RefNamespace::Profile` resolve-ref seam
        // (mirrors the nmp-nip57 LNURL round-trip).
        let _ = ctx;
        todo!("S2: NIP-05 reverse-resolve HTTP round-trip (#1804)")
    }
}
