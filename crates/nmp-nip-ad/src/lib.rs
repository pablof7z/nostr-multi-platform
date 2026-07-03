//! NIP-AD resolver (#2927).
//!
//! An ordinary web URL `https://<domain>/<path>` can double as a pointer to
//! Nostr events. To resolve it, GET
//! `https://<domain>/.well-known/nostr.json?ad=<path>` (percent-encoded path),
//! select the entry keyed by `<path>`, and run its `{filter, relays}` as a live
//! collection query yielding 0..N events.
//!
//! # Layers
//!
//! * [`parse`] — PURE shape/selection parsing (no IO): select the path entry,
//!   parse `{filter, relays}` into an [`AdResolution`], keep the FULL filter
//!   (multi-event capable). Always-compiled.
//! * [`policy`] — the app-injected [`AdResolutionPolicy`] seam + built-ins
//!   (`NeverAutoResolve` / `Always` / `FollowsOnly` / `WebOfTrust`). Pure/sync.
//!   Always-compiled.
//! * [`resolve_ad_url_blocking`] — the IO layer: the blocking `.well-known`
//!   round-trip (SSRF-guarded, bounded), behind the `native` feature.

pub mod parse;
pub mod policy;
pub mod ui_codes;

pub use parse::{is_valid_domain, parse_ad_wellknown, AdResolution};
pub use policy::{
    AdRenderContext, AdResolutionPolicy, Always, FollowsOnly, NeverAutoResolve, WebOfTrust,
};

// The blocking `.well-known/nostr.json?ad=<path>` GET uses the shared
// `nmp-wellknown-http` bounded fetcher — native only (mirrors nip05). The pure
// `parse`/`policy` layers stay always-compiled.
#[cfg(feature = "native")]
mod http;

#[cfg(feature = "native")]
pub use http::resolve_ad_url_blocking;

#[cfg(test)]
mod tests;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
