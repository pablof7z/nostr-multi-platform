//! Relay routing primitives.
//!
//! # T105 — outbox is the routing authority
//!
//! Wire relay selection is driven by the planner-resolved per-author write/read
//! relays (NIP-65), NOT by a hardcoded constant. Every [`OutboundMessage`]
//! carries an explicit `relay_url` — the resolved target the transport dials.
//! [`RelayRole`] survives ONLY as a transport-lane + diagnostics discriminator
//! (relay-health rows, NIP-42 driver buckets, `wire_subs` grouping); it is no
//! longer a routing source.
//!
//! [`BOOTSTRAP_DISCOVERY_RELAYS`] is the explicit, documented cold-start seed
//! used for the *very first* kind:10002 (NIP-65 relay-list) discovery fetch
//! when nothing is cached. It is NEVER the routing default — once an author's
//! kind:10002 is cached, the resolver routes to their declared relays and the
//! bootstrap seed is no longer consulted for that author (D3: outbox routing
//! automatic — `docs/product-spec/overview-and-dx.md` §1.5).

pub(crate) const DEFAULT_VISIBLE_LIMIT: usize = 80;
pub(crate) const DEFAULT_EMIT_HZ: u32 = 4;
pub(crate) const TIMELINE_AUTHOR_LIMIT: usize = 500;

#[cfg(any(test, feature = "test-support"))]
pub(crate) const BOOTSTRAP_DISCOVERY_RELAYS: &[&str] =
    &["wss://relay.damus.io", "wss://purplepag.es"];

#[cfg(any(test, feature = "test-support"))]
pub(crate) const CONTENT_RELAY_URL: &str = BOOTSTRAP_DISCOVERY_RELAYS[0];
#[cfg(any(test, feature = "test-support"))]
pub(crate) const INDEXER_RELAY_URL: &str = BOOTSTRAP_DISCOVERY_RELAYS[1];

#[cfg(test)]
pub(crate) const TEST_NPUB: &str =
    "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";
#[cfg(test)]
pub(crate) const TEST_PUBKEY: &str =
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const FIATJAF_PUBKEY: &str =
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const JB55_PUBKEY: &str =
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245";

/// Transport-lane + diagnostics discriminator.
///
/// **Not a routing source (T105).** The actual wire target is the resolved
/// `OutboundMessage::relay_url`. `RelayRole` only buckets relay-health rows,
/// NIP-42 driver state, and `wire_subs` for the diagnostic surface. The first
/// connection of each lane bootstraps on [`BOOTSTRAP_DISCOVERY_RELAYS`] purely
/// so the cold-start kind:10002 discovery fetch has a socket to leave on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RelayRole {
    Content,
    Indexer,
    /// NIP-47 Nostr Wallet Connect relay. Spawned on demand when a wallet is
    /// connected; NOT included in `all()` so it does not block the startup
    /// bootstrap gate or appear in the standard relay-statuses projection.
    Wallet,
    /// NIP-46 bunker relay. Spawned on demand when a bunker is configured;
    /// NOT included in `all()` so it does not block the startup bootstrap gate.
    // Pre-wiring: not yet constructed — bunker relays are managed by
    // nmp-signer-broker directly; relay_mgmt integration is future work.
    #[allow(dead_code)]
    Bunker,
}

impl RelayRole {
    /// Bootstrap-only roles (spawned at start, gate for startup REQs).
    /// `Wallet` is excluded: it spawns on demand, not at startup.
    pub(crate) fn all() -> [Self; 2] {
        [Self::Content, Self::Indexer]
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Indexer => "indexer",
            Self::Wallet => "wallet",
            Self::Bunker => "bunker",
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn bootstrap_url(self) -> &'static str {
        match self {
            Self::Content => BOOTSTRAP_DISCOVERY_RELAYS[0],
            Self::Indexer => BOOTSTRAP_DISCOVERY_RELAYS[1],
            Self::Wallet => "",
            Self::Bunker => "",
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn url(self) -> &'static str {
        self.bootstrap_url()
    }
}

/// One outbound wire frame addressed to a concrete, resolved relay.
///
/// `relay_url` is the routing authority (T105): the planner-resolved per-author
/// write relay (content/profile/thread), the active-account read relay
/// (hashtag firehose), the NIP-65 outbox fan-out target (publish), or the
/// cold-start [`BOOTSTRAP_DISCOVERY_RELAYS`] seed (first kind:10002 discovery).
/// `role` is retained only for the diagnostics/transport lane it belongs to.
#[derive(Clone, Debug)]
pub(crate) struct OutboundMessage {
    pub(crate) role: RelayRole,
    /// Resolved wire target. The transport dials this URL.
    pub(crate) relay_url: String,
    pub(crate) text: String,
}

impl OutboundMessage {
    /// Construct an outbound frame for a resolved relay URL on the given lane.
    #[allow(dead_code)] // T105 transition shim — used as fan-out matures.
    pub(crate) fn to_relay(role: RelayRole, relay_url: impl Into<String>, text: String) -> Self {
        Self {
            role,
            relay_url: relay_url.into(),
            text,
        }
    }
}

/// Canonicalize a relay URL so that all call sites (add, remove, pool-key)
/// agree on the same string key.
///
/// # Rules (per URL semantics + NIP-01 relay URL conventions)
/// - Lowercase scheme and authority (host[:port]).
/// - Strip a single trailing `/` **only when the path is empty** (i.e.
///   `wss://r.ex/` → `wss://r.ex`). Non-empty paths are preserved verbatim
///   including any trailing slash (`wss://r.ex/nostr/` is unchanged).
/// - Reject any URL whose scheme is not `ws` or `wss` (after lowercasing).
/// - Preserve path, query, and fragment exactly as given (only scheme+host
///   are lowercased).
/// - Leading/trailing ASCII whitespace is stripped before parsing.
///
/// Returns `None` when the URL cannot be canonicalized (bad scheme, missing
/// authority, etc.). The caller MUST NOT spawn a relay worker in that case.
pub(crate) fn canonical_relay_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    // Find the scheme separator "://".
    let sep = s.find("://")?;
    let scheme = s[..sep].to_ascii_lowercase();
    if scheme != "ws" && scheme != "wss" {
        return None;
    }
    // Everything after "://" — split authority from path+query+fragment.
    let rest = &s[sep + 3..];
    if rest.is_empty() {
        return None; // no authority
    }
    // Authority ends at the first '/', '?', or '#'.
    let (authority, path_etc) = if let Some(pos) = rest.find(['/', '?', '#']) {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };
    if authority.is_empty() {
        return None; // e.g. "wss:///path" — no host
    }
    let authority_lower = authority.to_ascii_lowercase();
    // Strip trailing '/' only when path is exactly "/" (empty path notation).
    let path_etc_norm = if path_etc == "/" { "" } else { path_etc };
    Some(format!("{scheme}://{authority_lower}{path_etc_norm}"))
}

#[cfg(test)]
mod canonical_url_tests {
    use super::canonical_relay_url;

    #[test]
    fn t_canonicalize_lowercase_scheme_and_host() {
        assert_eq!(
            canonical_relay_url("WSS://R.Ex"),
            Some("wss://r.ex".to_string()),
            "scheme and host must be lowercased"
        );
    }

    #[test]
    fn t_canonicalize_strip_empty_path_trailing_slash() {
        assert_eq!(
            canonical_relay_url("wss://r.ex/"),
            Some("wss://r.ex".to_string()),
            "trailing slash on empty path must be stripped"
        );
    }

    #[test]
    fn t_canonicalize_case_and_trailing_slash_combined() {
        assert_eq!(
            canonical_relay_url("WSS://R.Ex/"),
            Some("wss://r.ex".to_string()),
            "uppercase scheme+host AND empty-path trailing slash"
        );
    }

    #[test]
    fn t_canonicalize_preserve_nonempty_path() {
        assert_eq!(
            canonical_relay_url("wss://r.ex/nostr"),
            Some("wss://r.ex/nostr".to_string()),
            "non-empty path must be preserved"
        );
    }

    #[test]
    fn t_canonicalize_preserve_nonempty_path_with_trailing_slash() {
        // A relay with a real path retains its trailing slash.
        assert_eq!(
            canonical_relay_url("wss://r.ex/nostr/"),
            Some("wss://r.ex/nostr/".to_string()),
            "trailing slash on non-empty path must be preserved"
        );
    }

    #[test]
    fn t_canonicalize_path_distinctness() {
        // A relay with a real path is distinct from the no-path form.
        let with_path = canonical_relay_url("wss://r.ex/nostr");
        let no_path = canonical_relay_url("wss://r.ex");
        assert_ne!(with_path, no_path, "wss://r.ex/nostr must be distinct from wss://r.ex");
    }

    #[test]
    fn t_canonicalize_preserve_port() {
        assert_eq!(
            canonical_relay_url("wss://r.ex:7777/"),
            Some("wss://r.ex:7777".to_string()),
            "port must be preserved, empty-path slash stripped"
        );
    }

    #[test]
    fn t_canonicalize_preserve_query() {
        assert_eq!(
            canonical_relay_url("WSS://R.Ex?foo=bar"),
            Some("wss://r.ex?foo=bar".to_string()),
            "query string must be preserved, scheme+host lowercased"
        );
    }

    #[test]
    fn t_canonicalize_ws_scheme() {
        assert_eq!(
            canonical_relay_url("ws://r.ex/"),
            Some("ws://r.ex".to_string()),
            "ws:// scheme is valid"
        );
    }

    #[test]
    fn t_canonicalize_reject_http() {
        assert_eq!(
            canonical_relay_url("http://r.ex"),
            None,
            "http scheme must be rejected"
        );
    }

    #[test]
    fn t_canonicalize_reject_https() {
        assert_eq!(
            canonical_relay_url("https://r.ex"),
            None,
            "https scheme must be rejected"
        );
    }

    #[test]
    fn t_canonicalize_reject_empty() {
        assert_eq!(canonical_relay_url(""), None, "empty string must be rejected");
    }

    #[test]
    fn t_canonicalize_trims_whitespace() {
        assert_eq!(
            canonical_relay_url("  wss://r.ex/  "),
            // Note: only leading/trailing whitespace is stripped from the raw
            // input. The trailing "  " is after the full URL so it's part of
            // path_etc — we do NOT strip inner whitespace. In practice relay
            // URLs do not contain embedded spaces, and `trim()` on the whole
            // input handles the common FFI/copy-paste case.
            // After trim → "wss://r.ex/" → empty path → strip slash.
            Some("wss://r.ex".to_string()),
            "leading/trailing whitespace must be stripped"
        );
    }
}
