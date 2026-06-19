//! Canonical Nostr kind-integer registry for the NMP workspace.
//!
//! # Why a separate crate
//!
//! `nmp-core` (Layer 3) holds the kernel substrate; `nmp-nip59` (Layer 4)
//! holds the gift-wrap primitive. The kernel depends on `nmp-nip59`
//! (`nmp-core/Cargo.toml`), so `nmp-nip59` CANNOT import from `nmp-core`
//! without creating a compile-time cycle.  Both crates need `KIND_GIFT_WRAP`
//! = 1059.  The same cycle blocks every other NIP-4 crate that wants the
//! constants from `nmp-core::kinds`.
//!
//! Moving the integer registry one layer down — to this zero-dependency
//! Layer-0 crate, using `nmp-nip42-types` as structural precedent — resolves
//! the cycle: `nmp-kinds` has NO workspace dependencies, so nothing can cycle
//! through it.  `nmp-core::kinds` re-exports everything with `pub use
//! nmp_kinds::*` so ALL existing `nmp_core::kinds::KIND_*` call sites compile
//! unchanged.  `nmp-nip59`, `nmp-marmot`, and any other NIP crate simply add
//! `nmp-kinds` to their `[dependencies]` and import from here.
//!
//! # Precedent
//!
//! `nmp-nip42-types` is the direct structural precedent: a tiny zero-dep
//! Layer-0 vocabulary crate that exists specifically to break the cycle
//! between the kernel FSM and the NIP-42 protocol module.  `nmp-kinds` is
//! identical in purpose — vocabulary that two layers need, with no deps of
//! its own.
//!
//! # Future: `nmp-proto`
//!
//! `docs/architecture/crate-boundaries.md` plans a `nmp-proto` crate (Layer 0)
//! that re-exports all of upstream `nostr`.  If `nmp-proto` lands, it can
//! re-export or absorb `nmp-kinds`; that migration is orthogonal to this one.
//!
//! # Scope
//!
//! This crate is the workspace's canonical *integer* registry only.  Per-NIP
//! event-shape, parser, builder, and routing logic still lives in the
//! protocol crates; nothing about a constant being declared here implies
//! the kernel knows how to read or write the corresponding event.

// ─── NIP-01 — basic event kinds ────────────────────────────────────────────

/// NIP-01 profile metadata (kind:0).
pub const KIND_PROFILE_METADATA: u32 = 0;

/// NIP-01 short text note (kind:1).
pub const KIND_SHORT_TEXT_NOTE: u32 = 1;

/// NIP-02 contact list / follow set (kind:3).
pub const KIND_CONTACT_LIST: u32 = 3;

/// NIP-25 reaction (kind:7).
pub const KIND_REACTION: u32 = 7;

/// NIP-17 chat message rumor (kind:14). The unencrypted inner payload of a
/// gift-wrap envelope.
pub const KIND_CHAT_MESSAGE: u32 = 14;

/// NIP-28 channel metadata (kind:41). Replaceable per `nostr::Kind`
/// (NIP-28 special case) — declared here so the canonical
/// [`is_replaceable`] predicate matches upstream `nostr` semantics
/// without the call sites carrying a magic literal.
pub const KIND_CHANNEL_METADATA: u32 = 41;

// ─── Marmot (MLS over Nostr, MIP-00..03) — group-messaging kinds ───────────

/// Marmot KeyPackage event (kind:30443, NIP-33 addressable). Current spec.
/// The addressable event a peer publishes so others can invite them into an
/// MLS group; keyed by `(pubkey, kind, d-tag)`.
pub const KIND_MARMOT_KEY_PACKAGE: u32 = 30443;

/// Marmot KeyPackage legacy event (kind:443). Dual-published alongside
/// kind:30443 through the migration window; readers accept both.
pub const KIND_MARMOT_KEY_PACKAGE_LEGACY: u32 = 443;

/// Marmot Welcome rumor (kind:444). The inner, unsigned MLS Welcome carried
/// inside a kind:1059 gift-wrap; the tap admits it defensively even though the
/// wire Welcome is always the gift-wrap (the shared core skips a bare 444).
pub const KIND_MARMOT_WELCOME: u32 = 444;

/// Marmot group message / commit / proposal (kind:445, MLS + MIP-03 outer
/// layer). Relay-pinned to the group's advertised relays.
pub const KIND_MARMOT_GROUP_MESSAGE: u32 = 445;

// ─── NIP-59 — sealed gift-wrap chain ──────────────────────────────────────

/// NIP-59 gift-wrap envelope (kind:1059). The outer event minted by the
/// gift-wrap builder; the kernel's `publish_signed_event` D10 guard refuses
/// to Auto-route this kind to the author's NIP-65 outbox (the unlinkability
/// the construction exists to provide depends on the explicit relay pin).
pub const KIND_GIFT_WRAP: u32 = 1059;

// ─── NIP-65 — relay list metadata ─────────────────────────────────────────

/// NIP-65 relay list (kind:10002). The replaceable event each user
/// publishes to advertise their preferred read/write relays — the source
/// of truth the outbox resolver reads when routing a publish through
/// `PublishTarget::Auto`.
pub const KIND_RELAY_LIST: u32 = 10002;

// ─── NIP-57 — lightning zaps ──────────────────────────────────────────────

/// NIP-57 zap request (kind:9734). Built by the client and embedded in the
/// LNURL-pay flow; the LN provider reads it to mint the zap receipt.
pub const KIND_ZAP_REQUEST: u32 = 9734;

/// NIP-57 zap receipt (kind:9735). Minted by the LN provider after the
/// invoice settles. Decode-only — clients never construct kind:9735 directly.
pub const KIND_ZAP_RECEIPT: u32 = 9735;

// ─── NIP-78 — arbitrary custom app data ───────────────────────────────────

/// NIP-78 arbitrary custom app data (kind:30078). Addressable by
/// `(pubkey, kind, d-tag)`; content and non-`d` tags are app-defined.
pub const KIND_APP_DATA: u32 = 30078;

// ─── NIP-17 — DM relay list ───────────────────────────────────────────────

/// NIP-17 § 2 DM-relay list (kind:10050). The relays a user wants to
/// receive gift-wrapped DMs at. Each tag is `["relay", <wss-url>]`.
pub const KIND_DM_RELAY_LIST: u32 = 10050;

// ─── NIP-51 — curated lists ───────────────────────────────────────────────

/// NIP-51 public mute list (kind:10000). The active account's hard-muted
/// pubkeys (`p` tags) and event ids (`e` tags).
pub const KIND_MUTE_LIST: u32 = 10000;

/// NIP-51 global bookmark list (kind:10003). Public items are raw bookmark
/// references such as `["e", <event-id>]` and `["a", <kind:pubkey:d>]`.
pub const KIND_BOOKMARK_LIST: u32 = 10_003;

// ─── NIP-51 § kind:10006 — blocked relays ────────────────────────────────

/// NIP-51 blocked relays list (kind:10006). The relays a user explicitly
/// refuses to publish to or receive events from. Tag shape: `["relay", <wss-url>]`.
pub const KIND_BLOCKED_RELAYS: u32 = 10_006;

/// NIP-51 search relays list (kind:10007). The relays a user prefers for
/// NIP-50 search requests. Tag shape: `["relay", <wss-url>]`.
pub const KIND_SEARCH_RELAYS: u32 = 10_007;

// ─── Blossom (BUD-02) — blob-server upload authorization ───────────────────

/// Blossom BUD-01/BUD-02 authorization event (kind:24242). A short-lived,
/// signed Nostr event placed in an `Authorization: Nostr <base64(event)>`
/// header to authorise a blob PUT/GET/DELETE against a Blossom blob server.
/// Tag shape for an upload: `["t","upload"]`, `["x",<sha256-hex>]`,
/// `["expiration",<unix-secs>]`. The kind constant lives here (Layer-0
/// vocabulary); all Blossom build/transport logic lives in `nmp-blossom`.
pub const KIND_BLOSSOM_AUTH: u32 = 24242;

// ─── NIP-60 / NIP-61 — Cashu wallet + nutzap ──────────────────────────────

/// NIP-60 Cashu wallet event (kind:17375). Encrypted wallet config
/// (privkey + mints). Replaceable.
pub const KIND_NIP60_WALLET: u32 = 17375;

/// NIP-60 Cashu unspent-proof token event (kind:7375). Encrypted proofs.
pub const KIND_NIP60_TOKEN: u32 = 7375;

/// NIP-60 Cashu spending-history event (kind:7376).
pub const KIND_NIP60_HISTORY: u32 = 7376;

/// NIP-60 Cashu deposit-quote event (kind:7374). Deposit in-progress.
pub const KIND_NIP60_QUOTE: u32 = 7374;

/// NIP-61 Cashu nutzap informational event (kind:10019). Advertises accepted
/// mints + the recipient pubkey. Replaceable.
pub const KIND_NIP61_NUTZAP_INFO: u32 = 10019;

/// NIP-61 Cashu nutzap event (kind:9321). Sends ecash proofs to a recipient.
pub const KIND_NIP61_NUTZAP: u32 = 9321;

// ─── NIP-88 — Cashu mint announcement ──────────────────────────────────────

/// NIP-88 mint announcement (kind:38172, addressable). A mint publishes its
/// metadata to Nostr.
pub const KIND_MINT_ANNOUNCE: u32 = 38172;

// ─── NIP-01 replaceable / addressable kind predicates ──────────────────────

/// Whether a kind is a *regular replaceable* event (NIP-01).
///
/// Replaceable means that, for each `(pubkey, kind)` combination, only the
/// latest event MUST be stored; older versions MAY be discarded.
///
/// Per NIP-01 the replaceable set is kind `0` (metadata), kind `3` (contacts),
/// and the range `10000..=19999`. Kind `41` (NIP-28 channel metadata) is the
/// one special case `nostr::Kind::is_replaceable` adds beyond the NIP-01 range
/// text, included here so this predicate matches the upstream `nostr` crate
/// bit-for-bit — `nmp-store` / `nmp-nostr-lmdb` delegate to `nostr::Kind`, and
/// the two must NEVER disagree for the same kind.
///
/// This is the strict NIP-01 meaning and does NOT include addressable kinds:
/// callers wanting "replaceable in the broad sense" must test
/// `is_replaceable(k) || is_parameterized_replaceable(k)`.
#[inline]
#[must_use]
pub fn is_replaceable(kind: u32) -> bool {
    matches!(kind, 0 | 3 | 41) || (10_000..20_000).contains(&kind)
}

/// Whether a kind is *addressable* (a.k.a. parameterized replaceable, NIP-01).
///
/// Addressable means that, for each `(pubkey, kind, d-tag)` combination, only
/// the latest event MUST be stored. The addressable range is `30000..=39999`.
///
/// The historical name "parameterized replaceable" is retained for the public
/// symbol, but the semantics are exactly `nostr::Kind::is_addressable`: the
/// ephemeral `20000..=29999` range is NOT addressable (the prior hand-rolled
/// `nmp-core` copy wrongly included it).
#[inline]
#[must_use]
pub fn is_parameterized_replaceable(kind: u32) -> bool {
    (30_000..40_000).contains(&kind)
}

/// Whether an event's `#p` tags denote message **recipients** (people to
/// notify) rather than **subjects** (list members / follows / mutes).
///
/// Only recipient-kinds get recipient-inbox fan-out on the outbox publish path.
/// Replaceable and addressable events use `#p` tags to identify list members,
/// followees, or mute targets — NOT to address a message to those pubkeys. Routing
/// a kind:3 contact list to every followee's inbox relay is wrong; the followees
/// are subjects of the list, not its intended receivers.
///
/// The predicate is deliberately derived from the NIP-01 replaceable / addressable
/// classification: all replaceable (kind:0, kind:3, kind:41, kind:10000–19999) and
/// all addressable (kind:30000–39999) events carry subject-p-tags, not recipient-p-tags.
/// This avoids a hardcoded kind allowlist that would bit-rot as new list/set kinds
/// are defined by future NIPs.
///
/// Regular (non-replaceable, non-addressable) events — kind:1, kind:7, kind:9321,
/// kind:1059, etc. — use `#p` tags to mention or address people, so they still
/// fan out to those people's inbox relays.
#[inline]
#[must_use]
pub fn ptags_are_recipients(kind: u32) -> bool {
    !(is_replaceable(kind) || is_parameterized_replaceable(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaceable_matches_nip01_and_nostr() {
        // Regular replaceable per NIP-01 + nostr's kind:41 special case.
        assert!(is_replaceable(0), "kind:0 metadata");
        assert!(is_replaceable(3), "kind:3 contacts");
        assert!(is_replaceable(41), "kind:41 NIP-28 channel metadata");
        assert!(is_replaceable(10_000), "kind:10000 mute list");
        assert!(is_replaceable(10_002), "kind:10002 relay list");
        assert!(is_replaceable(10_007), "kind:10007 search relays");
        assert!(is_replaceable(19_999), "kind:19999 end of range");

        // The DIVERGENT-bug cases: notes/reposts/reactions are NOT replaceable.
        assert!(!is_replaceable(1), "kind:1 short text note is regular");
        assert!(!is_replaceable(6), "kind:6 repost is regular");
        assert!(!is_replaceable(7), "kind:7 reaction is regular");
        assert!(!is_replaceable(9_999), "kind:9999 end of regular range");

        // Ephemeral + addressable are not regular-replaceable.
        assert!(!is_replaceable(20_000), "kind:20000 ephemeral");
        assert!(!is_replaceable(29_999), "kind:29999 ephemeral");
        assert!(!is_replaceable(30_000), "kind:30000 addressable");
        assert!(!is_replaceable(40_000), "kind:40000 above addressable");
    }

    #[test]
    fn parameterized_replaceable_is_addressable_range() {
        assert!(is_parameterized_replaceable(30_000), "start of range");
        assert!(is_parameterized_replaceable(30_023), "long-form article");
        assert!(is_parameterized_replaceable(39_999), "end of range");

        // Ephemeral is NOT addressable (prior copy wrongly said it was).
        assert!(!is_parameterized_replaceable(20_000), "ephemeral start");
        assert!(!is_parameterized_replaceable(29_999), "ephemeral end");

        // Neither regular nor regular-replaceable kinds are addressable.
        assert!(!is_parameterized_replaceable(0));
        assert!(!is_parameterized_replaceable(3));
        assert!(!is_parameterized_replaceable(10_000));
        assert!(!is_parameterized_replaceable(40_000));
    }

    #[test]
    fn ptags_are_recipients_classifies_lists_as_subjects() {
        // Replaceable list/discovery kinds — p-tags are SUBJECTS, not recipients.
        assert!(
            !ptags_are_recipients(3),
            "kind:3 contact list p-tags are followees (subjects)"
        );
        assert!(
            !ptags_are_recipients(0),
            "kind:0 profile (no p-tags, but still replaceable)"
        );
        assert!(
            !ptags_are_recipients(10_000),
            "kind:10000 mute list p-tags are muted pubkeys (subjects)"
        );
        assert!(
            !ptags_are_recipients(10_002),
            "kind:10002 relay list — replaceable, no recipient p-tags"
        );
        assert!(
            !ptags_are_recipients(41),
            "kind:41 NIP-28 channel metadata — replaceable"
        );

        // Addressable list/set kinds — p-tags are SUBJECTS, not recipients.
        assert!(
            !ptags_are_recipients(30_000),
            "kind:30000 follow set p-tags are list members (subjects)"
        );
        assert!(
            !ptags_are_recipients(30_023),
            "kind:30023 long-form article — addressable"
        );
        assert!(!ptags_are_recipients(39_999), "end of addressable range");

        // Regular (non-replaceable, non-addressable) events — p-tags ARE recipients.
        assert!(
            ptags_are_recipients(1),
            "kind:1 short text note mentions are recipients"
        );
        assert!(
            ptags_are_recipients(7),
            "kind:7 reaction — recipient fan-out enabled"
        );
        assert!(
            ptags_are_recipients(1059),
            "kind:1059 gift-wrap — recipient routing (via Explicit, but semantics correct)"
        );
    }
}
