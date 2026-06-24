//! ADR-0063 — the canonical raw event-key parser.
//!
//! The unified `resolve_ref` / `release_ref` seam (refs.rs) addresses an event
//! by the **raw key** the FFI/JNI contract documents (resolve_ref.rs §"Key
//! encoding"):
//!
//! * a 64-char **lowercase** hex event-id (`StoredEvent.id`), or
//! * a `kind:pubkey:d` **coordinate** (the `naddr` primary-id encoding), or
//! * an `i:<external-id>` **NIP-73 external reference** (#1654 — e.g.
//!   `i:podcast:item:guid:<guid>`, `i:isbn:<n>`, `i:doi:<id>`). The `i:` prefix
//!   disambiguates an external ref from a coordinate (which begins with a decimal
//!   kind, never `i`) and from a hex64 id (which carries no colon). `<external-id>`
//!   is the verbatim NIP-73 `i`-tag value the referencing event carries; the
//!   resolver fetches the kind:1111 (NIP-22) / kind:1063 etc. event that tags
//!   that external id with a `["i", <external-id>]` filter. Never replaceable, so
//!   it resolves one-shot like an immutable event-id (no `Live` tailing slot).
//!
//! This is NOT a `nostr:`/NIP-21 URI. A host that starts from a URI must decode
//! it before calling the documented raw-key resolver seam.
//!
//! D6 — a malformed key parses to `None`; the resolver body then no-ops (no
//! claim, no discovery REQ, no panic). Tests in `refs_tests_key.rs` assert each
//! malformed shape fails closed.

use crate::kernel::refs::{EventShape, RefLiveness};
use crate::planner::InterestShape;

/// A parsed, canonical event reference. The kernel's resolver body
/// (`resolve_event_ref`) builds its refcount, interest, and per-key rev off
/// these fields exactly as the former URI-parsing arms did.
pub(in crate::kernel) struct EventTarget {
    /// The projection key — hex64 event-id, `kind:pubkey:d` coordinate, or
    /// `i:<external-id>` NIP-73 external ref. Must match the renderer-side
    /// `WireUri.primary_id`.
    pub primary_id: String,
    /// `Some((kind, pubkey, d_tag))` for an addressable (naddr) coordinate;
    /// `None` for an immutable event-id OR a NIP-73 external ref. Drives the
    /// F-TTL freshness gate and the `Live` tailing path (immutable ids and
    /// external refs degrade to one-shot — neither is replaceable).
    pub replaceable_coord: Option<(u32, String, String)>,
    /// The wire-level REQ filter (`{event_ids,limit:1}` for an id;
    /// `{kinds,authors,#d,limit:1}` for a coordinate; `{#i,limit:1}` for a
    /// NIP-73 external ref).
    pub filter: InterestShape,
    /// The author the claim-expansion tracker seeds its Phase-1 warm filter
    /// with. The coordinate's pubkey for a naddr; `None` for a bare event-id or
    /// an external ref (neither carries an author — a structured caller supplies
    /// relay hints instead).
    pub author: Option<String>,
}

/// A cold-start-parked event claim. Stores the CANONICAL pending target — the
/// raw key plus the shape / liveness / force / URI-decoded metadata the resolver
/// body needs — so the drain (`pending_event_claim_requests`) replays the raw
/// resolver body.
pub(in crate::kernel) struct PendingEventClaim {
    pub key: String,
    pub consumer_id: String,
    pub shape: EventShape,
    pub liveness: RefLiveness,
    pub force: bool,
    pub event_author: Option<String>,
    pub relay_hints: Vec<String>,
}

/// `true` when `s` is exactly 64 **lowercase** hex chars (a canonical NIP-01
/// event-id). The raw-key contract is lowercase-only — deliberately STRICTER
/// than `nostr.rs::is_hex_pubkey` (which accepts uppercase). Coordinate-form
/// keys never match (they contain `:`).
pub(in crate::kernel) fn is_lower_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `true` when `s` is a usable NIP-73 external identifier (#1654): non-empty,
/// free of ASCII control / whitespace bytes that could never appear in a wire
/// `i`-tag value, AND carrying a recognised NIP-73 scheme (fail-CLOSED — a ref
/// whose scheme the kernel does not understand never fabricates a `#i` REQ).
///
/// The earlier shape of this guard accepted ANY non-empty, whitespace-free
/// string, so `i:<anything>` issued a network `#i` REQ for an arbitrary/unknown
/// id (codex lead-gate HIGH 1). NIP-73 defines a CLOSED set of external-id
/// schemes; we mirror that set here. The kernel still does not *interpret* the id
/// (it forwards the verbatim value as a `#i` filter) — it only refuses to fetch a
/// scheme NIP-73 never minted.
pub(in crate::kernel) fn is_valid_external_id(s: &str) -> bool {
    !s.is_empty()
        && !s.bytes().any(|b| b.is_ascii_control() || b == b' ')
        && is_known_nip73_scheme(s)
}

/// `true` when `s` matches one of the external-id schemes defined by NIP-73
/// (<https://github.com/nostr-protocol/nips/blob/master/73.md>). This is the
/// fail-closed allowlist `is_valid_external_id` gates on; an `i:` ref whose value
/// is not one of these forms is rejected (no `#i` REQ, no fabricated preview).
///
/// `s` is the bytes AFTER the `i:` projection-key prefix has been stripped — i.e.
/// the verbatim NIP-73 `i`-tag value. Callers guarantee it is non-empty and
/// whitespace/control-free before this runs, so each arm only has to recognise
/// the scheme shape, not re-validate byte hygiene.
///
/// This recognises the SCHEME SHAPE only — it deliberately does NOT validate the
/// per-scheme value FORMAT (ISBN checksums, geohash alphabet, DOI regex, caip-2
/// chain-id charset beyond non-empty/alnum, …). The safety property is
/// fail-closed AT RESOLUTION: a known-scheme ref with a junk value
/// (e.g. `isbn:garbage`) issues a `#i` REQ that matches no event and renders NO
/// preview — that is acceptable and correct, not a hole. Input-format
/// gatekeeping is out of scope (codex re-gate SCOPE-OUT #2).
fn is_known_nip73_scheme(s: &str) -> bool {
    // Bare web URL — the only NIP-73 form with NO scheme prefix. A `:`-free value
    // can never be a prefixed scheme, so it MUST be a URL to be valid.
    if let Some(rest) = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
    {
        return !rest.is_empty();
    }
    // Hashtag — `#<topic>`.
    if let Some(topic) = s.strip_prefix('#') {
        return !topic.is_empty();
    }
    // Blockchain (NIP-73 generic form) — `<blockchain>:[<chainId>:]tx:<id>` and
    // `<blockchain>:[<chainId>:]address:<addr>`. `<blockchain>` is a lowercase
    // alphanumeric token (bitcoin, ethereum, solana, …); the optional `<chainId>`
    // (caip-2 style) is any non-empty alnum segment — we do NOT over-validate its
    // charset. `bitcoin:tx:<id>` (no chainId) and `ethereum:1:address:<a>` (with
    // chainId) both pass; `bitcoin:nonsense:<v>` (bad middle segment) and a
    // missing `tx`/`address` selector fail closed.
    if is_known_blockchain_scheme(s) {
        return true;
    }
    // Fixed-prefix schemes — the value after the prefix must be non-empty. Ordered
    // longest-prefix-first so `podcast:item:guid:` wins over `podcast:guid:` etc.
    const PREFIXES: &[&str] = &[
        "podcast:item:guid:",
        "podcast:publisher:guid:",
        "podcast:guid:",
        "isbn:",
        "geo:",
        "iso3166:",
        "isan:",
        "doi:",
    ];
    PREFIXES
        .iter()
        .any(|p| s.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

/// `true` when `s` is a NIP-73 blockchain external id:
/// `<blockchain>[:<chainId>]:<selector>:<value>` where `<selector>` is `tx` or
/// `address`. The leading `<blockchain>` token must be a non-empty lowercase
/// alphanumeric string; the optional `<chainId>` is any non-empty alphanumeric
/// segment (caip-2 style — not charset-validated beyond non-empty/alnum). The
/// trailing `<value>` (tx hash / address) must be non-empty.
///
/// Generalises the formerly-hardcoded `bitcoin:`/`ethereum:` arms so any chain
/// NIP-73 mints (`solana:tx:<id>`, …) resolves without a code change, while
/// `<chain>:<garbage>:<value>` (bad selector) still fails closed.
fn is_known_blockchain_scheme(s: &str) -> bool {
    let mut parts = s.split(':');
    let blockchain = parts.next().unwrap_or("");
    // `<blockchain>` — non-empty lowercase alphanumeric (rejects empty, uppercase,
    // and punctuation so a bare URL host or a `#`-tag never lands here).
    if blockchain.is_empty()
        || !blockchain
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9'))
    {
        return false;
    }
    // Next segment is either the selector (`tx`/`address`, no chainId) OR a
    // chainId followed by the selector. After this block `parts` is positioned at
    // the value (the tx hash / address).
    let second = parts.next().unwrap_or("");
    if second != "tx" && second != "address" {
        // `second` must instead be a non-empty alphanumeric chainId, with the
        // selector in the FOLLOWING segment.
        let chain_id_ok = !second.is_empty() && second.bytes().all(|b| b.is_ascii_alphanumeric());
        let selector = parts.next().unwrap_or("");
        if !chain_id_ok || (selector != "tx" && selector != "address") {
            return false;
        }
    }
    // The remaining text (the tx hash / address, which MAY itself contain colons)
    // must be non-empty.
    let value: String = parts.collect::<Vec<_>>().join(":");
    !value.is_empty()
}

/// Recover the verbatim NIP-73 external id from an `i:<external-id>` projection
/// key (#1654), or `None` when `key` is not an external-ref key. Used by the
/// store / cache lookup paths (`lookup_for_primary_id`, `event_already_known`)
/// to match a referencing event's `i`-tag value.
pub(in crate::kernel) fn external_id_from_key(key: &str) -> Option<&str> {
    key.strip_prefix("i:")
        .filter(|external_id| is_valid_external_id(external_id))
}

/// Parse a raw event key into its canonical [`EventTarget`], or `None` if the
/// key is malformed (D6 fail-closed). Accepts exactly the three documented forms:
///
/// * 64-char lowercase hex event-id →
///   `{primary_id: key, replaceable_coord: None, filter: {event_ids:[key], limit:1}}`.
/// * `kind:pubkey:d` coordinate → `splitn(3, ':')` with a **canonical decimal**
///   kind, a **lowercase-hex** pubkey, and a present `d` segment →
///   `{primary_id: key, replaceable_coord: Some((kind,pubkey,d)),
///     filter: {kinds:[kind], authors:[pubkey], "#d":[d], limit:1}}` — the same
///   shape the former naddr arm built.
/// * `i:<external-id>` NIP-73 external ref (#1654) → `{primary_id: key,
///   replaceable_coord: None, filter: {"#i":[<external-id>], limit:1}}`. The
///   `i:` prefix is stripped to recover the verbatim external-id; the wire REQ
///   matches any event tagging that external id with an `i` tag.
pub(in crate::kernel) fn parse_event_key(key: &str) -> Option<EventTarget> {
    // Immutable event-id: the whole key is lowercase-64-hex (no colon).
    if is_lower_hex64(key) {
        let filter = InterestShape {
            event_ids: std::iter::once(key.to_string()).collect(),
            limit: Some(1),
            ..Default::default()
        };
        return Some(EventTarget {
            primary_id: key.to_string(),
            replaceable_coord: None,
            filter,
            author: None,
        });
    }

    // NIP-73 external reference `i:<external-id>` (#1654). The `i:` prefix
    // disambiguates from a coordinate (decimal kind, never `i`) and a hex64 id
    // (no colon). The external id is the verbatim NIP-73 `i`-tag value; a
    // present-but-empty external id (`i:`) fails closed (D6).
    if let Some(external_id) = key.strip_prefix("i:") {
        if !is_valid_external_id(external_id) {
            return None;
        }
        let mut tags: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        tags.insert(
            "i".to_string(),
            std::iter::once(external_id.to_string()).collect(),
        );
        let filter = InterestShape {
            tags,
            limit: Some(1),
            ..Default::default()
        };
        return Some(EventTarget {
            // The PROJECTION key is the full `i:<external-id>` form so it round-
            // trips with the renderer-side `WireUri.primary_id` and never
            // collides with a hex64 id or a coordinate.
            primary_id: key.to_string(),
            replaceable_coord: None,
            filter,
            author: None,
        });
    }

    // Addressable coordinate `kind:pubkey:d`. `d` tags may legally contain `:`
    // (rare, spec-allowed), so split on only the first two colons and keep the
    // remainder verbatim as the d segment.
    let mut parts = key.splitn(3, ':');
    let kind_str = parts.next()?;
    let pubkey = parts.next()?;
    let d_tag = parts.next()?;

    // Canonical decimal kind: reject empty, leading-zero noise, signs, and any
    // non-digit. Round-trip-checking the parsed value rejects non-canonical
    // forms like `030023` so the coordinate matches the renderer-side
    // `primary_id` byte-for-byte.
    if kind_str.is_empty() || !kind_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let kind: u32 = kind_str.parse().ok()?;
    if kind.to_string() != kind_str {
        return None;
    }

    // Lowercase-hex author pubkey; an uppercase or wrong-length pubkey fails
    // closed (the projection key is lowercase-only). A present-but-empty d
    // segment is allowed (a legal addressable identity); a MISSING d segment was
    // already rejected by the `parts.next()?` above.
    if !is_lower_hex64(pubkey) {
        return None;
    }

    let mut tags: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    tags.insert(
        "d".to_string(),
        std::iter::once(d_tag.to_string()).collect(),
    );
    let filter = InterestShape {
        kinds: std::iter::once(kind).collect(),
        authors: std::iter::once(pubkey.to_string()).collect(),
        tags,
        limit: Some(1),
        ..Default::default()
    };
    Some(EventTarget {
        primary_id: key.to_string(),
        replaceable_coord: Some((kind, pubkey.to_string(), d_tag.to_string())),
        filter,
        author: Some(pubkey.to_string()),
    })
}
