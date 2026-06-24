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
//!   input-intent classifier (`nmp_intent::classify`) calls this in its pure
//!   pass to emit `InputIntentTarget::Nip05 { identifier }`.
//! * [`ResolveNip05Command`] — the IO layer: a generic
//!   [`nmp_core::substrate::ProtocolCommand`] the dispatch layer enqueues. Its
//!   [`ProtocolCommand::run`] spawns a blocking worker that performs the HTTP
//!   GET (mirroring the nmp-nip57 LNURL fetcher) and posts a follow-up
//!   [`ActorCommand::ResolveRef`] carrying the resolved pubkey (landed through
//!   the `RefNamespace::Profile` resolve-ref seam). HTTP lives behind the
//!   `native` feature.

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_core::actor::{ActionLedgerCommand, ActorCommand};
#[cfg(feature = "native")]
use nmp_core::actor::RefsCommand;

pub mod parse;

// The blocking `.well-known/nostr.json` GET uses `ureq` + `std::thread::spawn`
// — native only (mirrors `nmp_nip57::lnurl`, which is itself `#[cfg(native)]`).
// `parse_nip05` stays always-compiled for the pure classifier pass.
#[cfg(feature = "native")]
mod http;
// SSRF host guard for the `.well-known/nostr.json` fetch (#1882). Native-only —
// it does blocking DNS resolution and is only reached from the native worker.
#[cfg(feature = "native")]
mod host_guard;

pub use parse::parse_nip05;

/// Refcount owner key used for the follow-up [`ActorCommand::ResolveRef`] this
/// command posts. The NIP-05 reverse lookup resolves a profile on behalf of the
/// input-intent dispatch (not a long-lived UI view), so it claims under one
/// shared consumer id. A `CacheOk` claim (see [`ResolveNip05Command::run`])
/// means the slot is one-shot on a miss and is not retained as a live sub, so a
/// dedicated per-call release is unnecessary — the existing ref refcounting
/// dedups concurrent NIP-05 resolutions of the same pubkey.
///
/// Only referenced by the native HTTP worker (the follow-up `ResolveRef`), so
/// it is `native`-gated to stay dead-code-clean in the wasm / no-IO build.
#[cfg(feature = "native")]
const NIP05_RESOLVE_CONSUMER_ID: &str = "nip05-reverse-resolve";

/// Resolve a NIP-05 `name@domain` identifier to a pubkey via the domain's
/// `.well-known/nostr.json` endpoint.
///
/// Constructed by the dispatch layer when `nmp_intent::classify` returns an
/// `InputIntentTarget::Nip05`. The `run` worker performs the HTTP GET off the
/// actor thread and posts the resolved pubkey back as a follow-up
/// [`ActorCommand::ResolveRef`] (`RefNamespace::Profile`), so the resolved
/// profile lands in the `refs.profile` projection exactly as a tapped
/// `nmp_app_resolve_ref` call would. On any failure it emits a diagnostic
/// [`ActorCommand::ShowToast`] (and, when a `correlation_id` is present, a
/// `RecordActionFailure`) — the failure is never swallowed.
#[derive(Debug)]
pub struct ResolveNip05Command {
    /// The NIP-05 local-part (`name` in `name@domain`), already shape-validated
    /// and matched verbatim against the `names` map by [`parse_nip05`].
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
        #[cfg(feature = "native")]
        {
            let Self {
                name,
                domain,
                correlation_id,
            } = *self;
            // Re-validate the shape through the canonical `parse_nip05` before
            // doing anything else. The fields are public and may have been
            // constructed directly (bypassing the classifier), so a caller could
            // smuggle an illegal local-part / malformed domain straight into the
            // URL builder. `parse_nip05` lowercases the domain and enforces the
            // local-part charset; on rejection we fail closed (#1882). Pure — no
            // IO, safe on the actor thread.
            let validated = parse_nip05(&format!("{name}@{domain}"));
            let (name, domain) = match validated {
                Some(parts) => parts,
                None => {
                    let message =
                        format!("NIP-05 lookup rejected `{name}@{domain}`: not a valid identifier");
                    ctx.send(ActorCommand::ShowToast {
                        message: message.clone(),
                    });
                    if let Some(cid) = correlation_id {
                        ctx.send(ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
                            correlation_id: cid,
                            reason: message,
                        }));
                    }
                    return Ok(());
                }
            };
            // D8 — never block the actor thread. Hand the worker an owned
            // `CommandSender` clone (cheap atomic ref-count bump) and SPAWN the
            // blocking HTTP off the actor loop (the nmp-nip57 LNURL pattern).
            let worker_tx = ctx.command_sender_clone();
            std::thread::spawn(move || {
                match http::resolve_nip05_pubkey_blocking(&name, &domain) {
                    Ok(pubkey) => {
                        // Success: land the resolved pubkey through the unified
                        // resolve-ref seam (`RefNamespace::Profile`). The kernel
                        // fetches the kind:0 (store-first, OneShot on miss) and
                        // surfaces it in `refs.profile` keyed by the pubkey —
                        // identical to a `nmp_app_resolve_ref` profile claim.
                        let _ = worker_tx.send(ActorCommand::Refs(RefsCommand::Resolve {
                            namespace: nmp_core::RefNamespace::Profile,
                            key: pubkey,
                            consumer_id: NIP05_RESOLVE_CONSUMER_ID.to_string(),
                            shape: nmp_core::RefShape::Profile(nmp_core::ProfileShape::Card),
                            // CacheOk — one-shot on a miss; this is a navigation
                            // resolve, not an open live screen.
                            liveness: nmp_core::RefLiveness::CacheOk,
                            force: false,
                            hints: Vec::new(),
                        }));
                    }
                    Err(reason) => {
                        // D6 — surface the failure as observable state; never
                        // swallow it. The reason is human-readable and never
                        // echoes the response body verbatim.
                        let message = format!("NIP-05 lookup failed for {name}@{domain}: {reason}");
                        let _ = worker_tx.send(ActorCommand::ShowToast {
                            message: message.clone(),
                        });
                        if let Some(cid) = correlation_id {
                            let _ = worker_tx.send(ActorCommand::ActionLedger(
                                ActionLedgerCommand::RecordFailure {
                                    correlation_id: cid,
                                    reason: message,
                                },
                            ));
                        }
                    }
                }
            });
            Ok(())
        }
        // wasm32 / `default-features = false`: the HTTP worker is not compiled.
        // Fail closed with a diagnostic rather than silently dropping the
        // command, so the absence of the native fetcher is observable.
        #[cfg(not(feature = "native"))]
        {
            let Self {
                name,
                domain,
                correlation_id,
            } = *self;
            let message =
                format!("NIP-05 lookup for {name}@{domain} requires the native HTTP fetcher");
            ctx.send(ActorCommand::ShowToast {
                message: message.clone(),
            });
            if let Some(cid) = correlation_id {
                ctx.send(ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
                    correlation_id: cid,
                    reason: message,
                }));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
