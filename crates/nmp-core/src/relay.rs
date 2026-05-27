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

pub const DEFAULT_VISIBLE_LIMIT: usize = 80;
pub const DEFAULT_EMIT_HZ: u32 = 4;
pub(crate) const TIMELINE_AUTHOR_LIMIT: usize = 500;

/// A `wss://`/`ws://` URL for a relay, in plain (non-canonicalized) string
/// form.
///
/// This is the single definition of the `RelayUrl` alias for the whole crate.
/// `planner`, `publish`, and `store` each re-export it (`pub use
/// crate::relay::RelayUrl`) so their existing import paths are unchanged —
/// previously each of the three module families defined its own identical
/// `pub type RelayUrl = String`, which made a "what is a `RelayUrl`" search
/// return three competing answers.
///
/// It stays a transparent `String` alias (grep-able, swappable): the routing
/// keys that need the canonicalization *invariant* use [`CanonicalRelayUrl`]
/// instead — that is the type to reach for when a value indexes the transport
/// pool or the kernel's `wire_subs` / `persistent_subs` maps.
pub type RelayUrl = String;

/// Fallback relay URLs used when no host-configured relays are available.
/// Compiled in unconditionally so cold-start sign-ins have discovery relays.
pub(crate) const FALLBACK_INDEXER_RELAY: &str = "wss://purplepag.es";
pub(crate) const FALLBACK_CONTENT_RELAY: &str = "wss://relay.primal.net";

/// Substrate-level relay bootstrap entry: a relay URL paired with its role
/// string (e.g. `"both,indexer"`, `"indexer"`).
pub(crate) struct BootstrapRelayEntry {
    pub url: &'static str,
    pub role: &'static str,
}

const RELAY_BOOTSTRAP_DEFAULTS: &[BootstrapRelayEntry] = &[
    BootstrapRelayEntry {
        url: FALLBACK_CONTENT_RELAY,
        role: "both,indexer",
    },
    BootstrapRelayEntry {
        url: FALLBACK_INDEXER_RELAY,
        role: "indexer",
    },
];

/// Substrate-level default relay bootstrap set used when a host passes no
/// relay configuration during account creation or cold-start sign-in.
pub(crate) fn default_relay_bootstrap() -> &'static [BootstrapRelayEntry] {
    RELAY_BOOTSTRAP_DEFAULTS
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) const BOOTSTRAP_DISCOVERY_RELAYS: &[&str] =
    &[FALLBACK_CONTENT_RELAY, FALLBACK_INDEXER_RELAY];

#[cfg(test)]
pub(crate) const CONTENT_RELAY_URL: &str = BOOTSTRAP_DISCOVERY_RELAYS[0];
#[cfg(test)]
pub(crate) const INDEXER_RELAY_URL: &str = BOOTSTRAP_DISCOVERY_RELAYS[1];

#[cfg(test)]
pub(crate) const TEST_PUBKEY: &str =
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
#[cfg(test)]
pub(crate) const FIATJAF_PUBKEY: &str =
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
#[cfg(test)]
pub(crate) const JB55_PUBKEY: &str =
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245";

// Step 8 phase A — `RelayRole` moved to `nmp-network::role` (the transport
// layer owns the lane discriminator). `nmp-core` re-exports it under its
// prior path (`nmp_core::RelayRole`) via `lib.rs` so downstream callers
// keep compiling unchanged. The test-only `bootstrap_url()` / `url()`
// helpers live here as a private extension trait — they reference the
// `BOOTSTRAP_DISCOVERY_RELAYS` constants which are nmp-core-only.
//
// V-38: the `Wallet` variant on `nmp_network::RelayRole` is constructed by
// `nmp-nip47`'s wallet runtime through `Kernel::set_relay_auth_signer(
// RelayRole::Wallet, ...)`. Substrate-grade — `nmp-network` carries no
// app/protocol nouns even though the variant name reads "Wallet".
pub use nmp_network::RelayRole;

#[cfg(any(test, feature = "test-support"))]
pub(crate) trait RelayRoleTestExt {
    fn bootstrap_url(self) -> &'static str;
    fn url(self) -> &'static str;
}

#[cfg(any(test, feature = "test-support"))]
impl RelayRoleTestExt for RelayRole {
    fn bootstrap_url(self) -> &'static str {
        match self {
            RelayRole::Content => BOOTSTRAP_DISCOVERY_RELAYS[0],
            RelayRole::Indexer => BOOTSTRAP_DISCOVERY_RELAYS[1],
            RelayRole::Wallet => "",
        }
    }

    fn url(self) -> &'static str {
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
///
/// V-01 Stage 3 — promoted to `pub` so the wasm32 `BrowserRelayDriver` in
/// `nmp-wasm` can route the kernel's outbound frames over `WebSocket::send_with_str`.
/// Fields stay `pub(crate)` because mutating them is reserved to the kernel's
/// own outbound producers (publish engine, view-request planner, AUTH driver);
/// external callers read via the accessors below. Substrate-grade (D0): the
/// type carries no app/protocol nouns.
#[derive(Clone, Debug)]
pub struct OutboundMessage {
    pub(crate) role: RelayRole,
    /// Resolved wire target. The transport dials this URL.
    pub(crate) relay_url: String,
    pub(crate) text: String,
}

impl OutboundMessage {
    /// Construct an outbound message destined for `relay_url` over `role`.
    ///
    /// `pub` so NIP-crate runtimes (`nmp-nip47` post-V-38) running on the
    /// actor thread can build outbound REQ / EVENT / CLOSE frames the
    /// dispatch arm forwards to the relay worker. The transport is opaque to
    /// this constructor — every frame must already be a valid NIP-01 wire
    /// JSON string.
    #[must_use]
    pub fn new(role: RelayRole, relay_url: String, text: String) -> Self {
        Self {
            role,
            relay_url,
            text,
        }
    }

    /// Diagnostics lane the frame belongs to. Forwarded by the WASM driver
    /// when reporting back through [`crate::KernelReducer::handle_relay_frame`]
    /// for any reply the kernel emits (e.g. AUTH responses).
    #[must_use]
    pub fn role(&self) -> RelayRole {
        self.role
    }

    /// Resolved wire target — the URL the transport dials.
    #[must_use]
    pub fn relay_url(&self) -> &str {
        &self.relay_url
    }

    /// Raw outbound text frame (NIP-01 JSON: `["REQ", …]`, `["EVENT", …]`,
    /// `["CLOSE", …]`, `["AUTH", …]`).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A relay URL that is guaranteed to be in canonical form.
///
/// # Why a newtype
/// Relay URLs key three kernel maps — the transport pool, `wire_subs`, and
/// `persistent_subs`. A REQ registered under one spelling (`wss://Relay.MIXED/`)
/// and an EOSE delivered under another (`wss://relay.mixed`) must hit the same
/// row, so every key MUST be canonical. When the key type was a plain `String`
/// that invariant was enforced only by callers remembering to call
/// `canonical_relay_url()` first — a bug class that required 10+ manual fixes
/// (mixed-case scheme/host, empty-path trailing slash) across past sessions.
///
/// `CanonicalRelayUrl` makes the invariant *unrepresentable to violate*: the
/// only constructor is [`CanonicalRelayUrl::parse`], which runs the
/// canonicalization. There is deliberately **no** `From<String>` /
/// `From<&str>` — those would silently re-admit a non-canonical key.
///
/// `Deref<Target = str>` / `AsRef<str>` / [`Display`] make it a drop-in for
/// the read paths (logging, JSON, substring checks); [`Self::into_string`]
/// hands the inner string to projection types (`RelayStatus.relay_url`, the
/// FFI surface) that stay `String`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct CanonicalRelayUrl(String);

impl CanonicalRelayUrl {
    /// Canonicalize `raw` and wrap it.
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
    #[must_use]
    pub(crate) fn parse(raw: &str) -> Option<Self> {
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
        Some(Self(format!("{scheme}://{authority_lower}{path_etc_norm}")))
    }

    /// Canonicalize `raw`, falling back to wrapping the raw string verbatim
    /// when it does not parse as a `ws`/`wss` URL.
    ///
    /// This is the fail-open contract every pre-newtype kernel call site relied
    /// on: `wire_subs` / `persistent_subs` keys are derived even for malformed
    /// URLs so a lookup against the same malformed input still matches. A truly
    /// non-canonical key can only enter the maps when *every* path agrees on
    /// the identical raw spelling — which is exactly the prior behavior.
    #[must_use]
    pub(crate) fn parse_or_raw(raw: &str) -> Self {
        Self::parse(raw).unwrap_or_else(|| Self(raw.to_string()))
    }

    /// Borrow the canonical URL as a string slice.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the newtype, yielding the inner canonical `String`. Used at the
    /// boundary with projection types (`RelayStatus.relay_url`, FFI) that stay
    /// `String`.
    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl std::ops::Deref for CanonicalRelayUrl {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CanonicalRelayUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CanonicalRelayUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for CanonicalRelayUrl {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for CanonicalRelayUrl {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<CanonicalRelayUrl> for str {
    fn eq(&self, other: &CanonicalRelayUrl) -> bool {
        self == other.0
    }
}

impl PartialEq<CanonicalRelayUrl> for &str {
    fn eq(&self, other: &CanonicalRelayUrl) -> bool {
        *self == other.0
    }
}

/// Canonicalize a relay URL so that all call sites (add, remove, pool-key)
/// agree on the same string key.
///
/// Free-function wrapper over [`CanonicalRelayUrl::parse`], retained for the
/// transport-pool / actor call sites that key their own `HashMap<String, _>`
/// pools on the canonical *string* rather than adopting the newtype. Returns
/// the inner `String` so those sites need no further conversion.
///
/// Also used by out-of-crate NIP builder crates (`nmp-router`, `nmp-nip17`) so
/// they don't each need their own copy of the canonicalization rules.
///
/// New kernel code should prefer [`CanonicalRelayUrl`] directly — the newtype
/// makes the canonicalization invariant compiler-enforced.
#[must_use]
pub fn canonical_relay_url(raw: &str) -> Option<String> {
    CanonicalRelayUrl::parse(raw).map(CanonicalRelayUrl::into_string)
}

#[cfg(test)]
#[path = "relay/tests.rs"]
mod tests;
