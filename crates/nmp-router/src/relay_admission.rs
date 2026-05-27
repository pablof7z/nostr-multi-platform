//! `RelayAdmissionPolicy` — pluggable guard for whether a relay URL is safe
//! to connect to at all.
//!
//! This is a *structural* check, distinct from [`nmp_core::substrate::BlockedRelayLookup`]
//! (which is per-account, user-declared, kind:10006-driven). The admission
//! policy is system-wide and URL-property-based: it answers "would we ever
//! connect to this URL regardless of who declared it?"
//!
//! The [`GenericOutboxRouter`][crate::GenericOutboxRouter] applies the policy
//! on **lanes 1–3 only** (NIP-65 mailbox, event-tag hints, provenance) — the
//! untrusted, network-sourced lanes. Operator-controlled lanes (4 UserConfigured,
//! 6 Indexer, 7 AppRelay) are not filtered so that local dev relays work as
//! expected.
//!
//! # Default policy
//!
//! [`PrivateNetworkPolicy`] — rejects URLs whose host resolves to:
//! - Loopback (`127.x.x.x`, `::1`, `localhost`)
//! - RFC-1918 private ranges (`10.x`, `172.16–31.x`, `192.168.x`)
//! - Link-local (`169.254.x.x`, `fe80::/10`)
//! - Unspecified (`0.0.0.0`, `::`)
//!
//! A future policy could compose this with an operator deny-list, a TLD
//! blocklist, or any other structural rule without touching the router.

use std::net::{Ipv4Addr, Ipv6Addr};

// ─── Trait ───────────────────────────────────────────────────────────────────

/// Gate for relay URL admission on untrusted lanes (1–3). Implementations must
/// be cheap to call — `route_publish` and `route_subscription` invoke this once
/// per candidate URL.
pub trait RelayAdmissionPolicy: Send + Sync {
    /// Return `true` if `url` is admissible (the router should use it),
    /// `false` to drop it silently.
    fn is_admissible(&self, url: &str) -> bool;
}

// ─── PrivateNetworkPolicy ────────────────────────────────────────────────────

/// Default admission policy: rejects loopback, RFC-1918, link-local, and
/// unspecified addresses. Allows all public hostnames and IPs.
pub struct PrivateNetworkPolicy;

impl RelayAdmissionPolicy for PrivateNetworkPolicy {
    fn is_admissible(&self, url: &str) -> bool {
        match extract_host(url) {
            Some(host) => !is_private_host(host),
            // Unparseable URL: can't extract a host to check, so reject it —
            // we can't safely connect to something we can't even parse.
            None => false,
        }
    }
}

// ─── Internals ───────────────────────────────────────────────────────────────

/// Extract the host portion from a `wss://` URL. Returns `None` on parse
/// failure. Handles:
/// - `wss://relay.example` → `"relay.example"`
/// - `wss://relay.example:443/path` → `"relay.example"`
/// - `wss://192.168.1.1:7777` → `"192.168.1.1"`
/// - `wss://[::1]:7777` → `"::1"` (brackets stripped)
/// - `wss://[::1]` → `"::1"`
fn extract_host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("wss://")?;
    let authority = rest.split('/').next()?; // up to first '/', no path
    if authority.is_empty() {
        return None;
    }
    if authority.starts_with('[') {
        // IPv6 bracketed: [::1] or [::1]:port
        let close = authority.find(']')?;
        Some(&authority[1..close])
    } else {
        // IPv4 or hostname, optional :port
        Some(match authority.rfind(':') {
            Some(pos) => &authority[..pos],
            None => authority,
        })
    }
}

fn is_private_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(addr) = host.parse::<Ipv4Addr>() {
        return addr.is_loopback() || addr.is_private() || addr.is_link_local() || addr.is_unspecified();
    }
    if let Ok(addr) = host.parse::<Ipv6Addr>() {
        return addr.is_loopback() || addr.is_unspecified() || is_ipv6_link_local(addr);
    }
    false
}

/// `fe80::/10` — link-local unicast for IPv6.
fn is_ipv6_link_local(addr: Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xffc0) == 0xfe80
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn admit(url: &str) -> bool {
        PrivateNetworkPolicy.is_admissible(url)
    }

    // ── public relays must pass ──────────────────────────────────────────────

    #[test]
    fn public_domain_admitted() {
        assert!(admit("wss://relay.damus.io"));
        assert!(admit("wss://nos.lol"));
        assert!(admit("wss://relay.nostr.band/path"));
        assert!(admit("wss://relay.example:443"));
    }

    #[test]
    fn public_ipv4_admitted() {
        assert!(admit("wss://8.8.8.8"));
        assert!(admit("wss://1.1.1.1:443"));
    }

    #[test]
    fn public_ipv6_admitted() {
        assert!(admit("wss://[2001:db8::1]"));
        assert!(admit("wss://[2606:4700:4700::1111]:443"));
    }

    // ── loopback rejected ────────────────────────────────────────────────────

    #[test]
    fn localhost_rejected() {
        assert!(!admit("wss://localhost"));
        assert!(!admit("wss://localhost:7777"));
        assert!(!admit("wss://LOCALHOST")); // case-insensitive
    }

    #[test]
    fn ipv4_loopback_rejected() {
        assert!(!admit("wss://127.0.0.1"));
        assert!(!admit("wss://127.0.0.1:7777"));
        assert!(!admit("wss://127.1.2.3")); // all of 127.x
    }

    #[test]
    fn ipv6_loopback_rejected() {
        assert!(!admit("wss://[::1]"));
        assert!(!admit("wss://[::1]:7777"));
    }

    // ── RFC-1918 rejected ────────────────────────────────────────────────────

    #[test]
    fn rfc1918_10_rejected() {
        assert!(!admit("wss://10.0.0.1"));
        assert!(!admit("wss://10.255.255.255:8080"));
    }

    #[test]
    fn rfc1918_172_rejected() {
        assert!(!admit("wss://172.16.0.1"));
        assert!(!admit("wss://172.31.255.255"));
    }

    #[test]
    fn rfc1918_192_168_rejected() {
        assert!(!admit("wss://192.168.0.1"));
        assert!(!admit("wss://192.168.1.1:7777"));
    }

    // ── link-local rejected ───────────────────────────────────────────────────

    #[test]
    fn ipv4_link_local_rejected() {
        assert!(!admit("wss://169.254.0.1"));
        assert!(!admit("wss://169.254.1.2:8080"));
    }

    #[test]
    fn ipv6_link_local_rejected() {
        assert!(!admit("wss://[fe80::1]"));
        assert!(!admit("wss://[fe80::1]:7777"));
        assert!(!admit("wss://[febf::1]")); // fe80::/10 includes up to febf
    }

    // ── unspecified rejected ─────────────────────────────────────────────────

    #[test]
    fn unspecified_rejected() {
        assert!(!admit("wss://0.0.0.0"));
        assert!(!admit("wss://[::]"));
    }

    // ── unparseable URL rejected ─────────────────────────────────────────────

    #[test]
    fn non_wss_scheme_rejected() {
        assert!(!admit("ws://relay.example")); // ws:// not wss://
        assert!(!admit("http://relay.example"));
        assert!(!admit("not-a-url"));
    }

    #[test]
    fn empty_host_rejected() {
        assert!(!admit("wss://"));
    }

    // ── extract_host unit tests ──────────────────────────────────────────────

    #[test]
    fn extract_host_plain_domain() {
        assert_eq!(extract_host("wss://relay.example"), Some("relay.example"));
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(extract_host("wss://relay.example:443"), Some("relay.example"));
    }

    #[test]
    fn extract_host_with_path() {
        assert_eq!(extract_host("wss://relay.example:443/ws"), Some("relay.example"));
    }

    #[test]
    fn extract_host_ipv4_with_port() {
        assert_eq!(extract_host("wss://192.168.1.1:7777"), Some("192.168.1.1"));
    }

    #[test]
    fn extract_host_ipv6_with_port() {
        assert_eq!(extract_host("wss://[::1]:7777"), Some("::1"));
    }

    #[test]
    fn extract_host_ipv6_no_port() {
        assert_eq!(extract_host("wss://[::1]"), Some("::1"));
    }
}
