//! NIP-05 SSRF host guard (#1882) — native only.
//!
//! Before the `.well-known/nostr.json` GET, a NIP-05 `domain` must be proven to
//! be a public DNS host. Without this guard a `name@127.0.0.1` /
//! `name@internal.corp` identifier could coerce the worker into issuing
//! requests against loopback / RFC-1918 / link-local / ULA / CGNAT / reserved
//! services (server-side request forgery).
//!
//! Runs on the blocking worker thread (DNS resolution is itself blocking IO —
//! never on the actor loop, D8).

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

/// HTTPS port the host-safety pre-resolve targets (also the scheme the GET
/// uses). Resolving against the real port the request will use keeps the
/// pre-flight check aligned with the actual connection.
const NIP05_HTTPS_PORT: u16 = 443;

/// Reject a NIP-05 host that is an IP literal or that resolves to a non-public
/// address.
///
/// CAVEAT — this resolves the host, then `ureq` independently resolves it again
/// when it connects, so a DNS-rebinding attacker who returns a public address to
/// this check and a private one to the connect could bypass the guard
/// (TOCTOU). Closing that fully needs resolve-then-pin (connect to the vetted
/// IP with SNI), which `ureq` 2.x does not expose cleanly. Documented as a
/// residual limitation; the literal/standard-resolution vectors are covered.
pub(crate) fn assert_host_is_public(domain: &str) -> Result<(), String> {
    // A NIP-05 `domain` is a DNS name, never an IP literal. Reject literals
    // outright — they are the most direct SSRF vector and never legitimate here.
    if domain.parse::<IpAddr>().is_ok() {
        return Err(format!(
            "NIP-05 domain `{domain}` is an IP literal; refusing the fetch (SSRF guard)"
        ));
    }
    // Resolve and require EVERY candidate address to be public. `to_socket_addrs`
    // is blocking — acceptable on this worker thread (D8).
    let addrs: Vec<_> = (domain, NIP05_HTTPS_PORT)
        .to_socket_addrs()
        .map_err(|e| format!("NIP-05 domain `{domain}` did not resolve: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!(
            "NIP-05 domain `{domain}` did not resolve to any address"
        ));
    }
    for addr in addrs {
        if !ip_is_public(&addr.ip()) {
            return Err(format!(
                "NIP-05 domain `{domain}` resolves to a non-public address; refusing the fetch (SSRF guard)"
            ));
        }
    }
    Ok(())
}

/// True iff `ip` is a public (globally-routable unicast) address. Anything
/// loopback / private / link-local / unique-local / CGNAT / reserved /
/// unspecified / multicast / broadcast is treated as non-public.
fn ip_is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_public(v4),
        IpAddr::V6(v6) => {
            // An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) must be judged by
            // its embedded v4 address, else `::ffff:127.0.0.1` would bypass.
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ipv4_is_public(&mapped);
            }
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()                          // ff00::/8
                || (seg[0] & 0xfe00) == 0xfc00                // fc00::/7 unique-local
                || (seg[0] & 0xffc0) == 0xfe80                // fe80::/10 link-local unicast
                || (seg[0] == 0x2001 && seg[1] == 0x0db8)) // 2001:db8::/32 documentation
        }
    }
}

/// True iff an IPv4 address is public. Wraps the stable `Ipv4Addr` predicates
/// (`is_private` / `is_loopback` / `is_link_local` / `is_broadcast` /
/// `is_documentation` / `is_multicast` / `is_unspecified`) and adds the
/// non-public ranges those omit: `0.0.0.0/8`, `100.64.0.0/10` (CGNAT),
/// `198.18.0.0/15` (benchmarking), and `240.0.0.0/4` (reserved).
fn ipv4_is_public(v4: &Ipv4Addr) -> bool {
    let o = v4.octets();
    !(v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()                          // 224.0.0.0/4
        || v4.is_unspecified()
        || o[0] == 0                                  // 0.0.0.0/8 "this network"
        || (o[0] == 100 && (o[1] & 0xc0) == 64)       // 100.64.0.0/10 CGNAT
        || (o[0] == 198 && (o[1] & 0xfe) == 18)       // 198.18.0.0/15 benchmarking
        || o[0] >= 240) // 240.0.0.0/4 reserved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn public_addresses_are_public() {
        for s in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "2606:4700:4700::1111",
        ] {
            assert!(ip_is_public(&ip(s)), "{s} should be public");
        }
    }

    #[test]
    fn non_public_v4_addresses_are_rejected() {
        // loopback, RFC-1918 private (×3), link-local, CGNAT, this-network,
        // broadcast, TEST-NET documentation, reserved 240/4, multicast 224/4,
        // and benchmarking 198.18/15.
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.5.6",
            "192.168.1.1",
            "169.254.10.20",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "192.0.2.5",
            "240.0.0.1",
            "224.0.0.1",
            "239.255.255.250",
            "198.18.0.1",
            "198.19.255.255",
        ] {
            assert!(!ip_is_public(&ip(s)), "{s} must be rejected as non-public");
        }
    }

    #[test]
    fn non_public_v6_addresses_are_rejected() {
        // loopback, unspecified, unique-local (fc00::/7), link-local (fe80::/10),
        // multicast (ff00::/8), documentation (2001:db8::/32), and an
        // IPv4-mapped loopback that must be judged by its v4 address.
        for s in [
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!ip_is_public(&ip(s)), "{s} must be rejected as non-public");
        }
    }

    #[test]
    fn ipv4_mapped_v6_public_is_public() {
        assert!(ip_is_public(&ip("::ffff:1.1.1.1")));
    }

    #[test]
    fn ip_literal_host_is_rejected_without_fetch() {
        // IP-literal hosts never reach DNS or HTTP — rejected purely on shape.
        for s in ["127.0.0.1", "10.0.0.1", "192.168.0.1", "::1", "8.8.8.8"] {
            assert!(
                assert_host_is_public(s).is_err(),
                "IP literal {s} must be rejected (no fetch)"
            );
        }
    }

    #[test]
    fn loopback_resolving_host_is_rejected_without_fetch() {
        // `localhost` resolves to a loopback address → rejected before any HTTP
        // egress. (Local DNS resolution only; no network round-trip.)
        let err = assert_host_is_public("localhost")
            .expect_err("localhost resolves to loopback and must be rejected");
        assert!(
            err.contains("non-public") || err.contains("did not resolve"),
            "unexpected rejection reason: {err}"
        );
    }
}
