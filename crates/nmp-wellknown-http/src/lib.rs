//! Shared SSRF-guarded, bounded `.well-known` HTTP GET (#2927).
//!
//! The single workspace authority for the two security-critical pieces every
//! `.well-known` fetcher needs, previously forked across `nmp-nip05` and
//! `nmp-nip57`:
//!
//! * [`assert_host_is_public`] — reject IP-literal hosts and hosts that resolve
//!   to a non-public address (loopback / RFC-1918 / link-local / ULA / CGNAT /
//!   reserved) BEFORE a fetch, so an attacker-supplied host can't probe
//!   internal services (SSRF).
//! * [`http_get_json`] — a bounded GET (10s timeout, `redirects(0)`,
//!   caller-supplied response-body cap) → `serde_json::Value`.
//!
//! Both live behind the `native` feature: DNS resolution and the `ureq`
//! round-trip are blocking-worker concerns and never compiled into a wasm /
//! no-IO build.

// The SSRF guard and bounded GET are blocking / native — only reached from a
// spawned worker thread. Gated together behind `native` (mirrors the old nip05
// posture the extraction preserves).
#[cfg(feature = "native")]
mod host_guard;
#[cfg(feature = "native")]
mod http;

#[cfg(feature = "native")]
pub use host_guard::{assert_host_is_public, ip_is_public, ipv4_is_public};
#[cfg(feature = "native")]
pub use http::http_get_json;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
