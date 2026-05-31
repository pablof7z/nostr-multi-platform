//! Tests for the `resolved_profiles` snapshot projection (additive — no
//! consumer reads it yet; ships the profile-merge precedence once in Rust).
//!
//! Precedence under test (highest → lowest):
//!   1. `claimed_profiles` — full `ProfileCard` (carries `nip05`/`about`/`lnurl`)
//!   2. `author_view.profile` — full `ProfileCard`, only-if-absent
//!   3. `mention_profiles` — lightweight payload, only-if-absent
//!
//! Each precedence test asserts on a field that ONLY the winning source could
//! have produced — never on `display_name`/`picture_url`, which all three
//! sources read from the same kind:0 cache and are therefore identical:
//!   - claimed vs mention: `nip05` (mention's `from_mention` hardcodes `""`;
//!     the claimed card carries the real kind:0 value).
//!   - claimed vs author_view: `npub` (claimed passes `Some(to_npub(pk))` →
//!     bech32; author_view passes `None` → `profile_card_for` falls back to the
//!     raw hex pubkey).
//!
//! Test-support paths:
//!   - Profiles (kind:0) are delivered by calling `ingest_profile` directly with
//!     JSON `content` — the `inject_replaceable_event` helper hardcodes empty
//!     content, so it cannot seed a `nip05`.
//!   - Notes (kind:1) are delivered through the REAL ingest path via
//!     `ingest_timeline_event` with a `diag-firehose-` sub_id, which bypasses the
//!     `timeline_authors` gate (ingest/timeline.rs:210) so the signed note enters
//!     `self.timeline` → `visible_items()` → `mention_profiles`. Real Schnorr
//!     signatures are used (mirrors `clock_injection_tests::signed_note`) so the
//!     note's author pubkey is controlled: the SAME keypair seeds both the note
//!     and the kind:0, so the pubkey genuinely appears in `mention_profiles`.

use super::nostr::NostrEvent;
use super::*;
use crate::display::to_npub;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

const RELAY: &str = "wss://relay.example/";

/// Build one real Schnorr-signed kind:1 event for `keys`. Returns the
/// `NostrEvent` shape the kernel ingest path consumes after JSON decoding
/// (mirrors `clock_injection_tests::signed_note`).
fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: event.kind.as_u16() as u32,
        tags: event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}

/// Deliver a kind:0 profile carrying real metadata by calling `ingest_profile`
/// directly. `parse_profile` JSON-decodes only the `content` field; the ingest
/// method runs post-verification and never reads the signature.
fn ingest_profile_with(
    kernel: &mut Kernel,
    pubkey: &str,
    created_at: u64,
    display_name: &str,
    nip05: &str,
) {
    let content = serde_json::json!({
        "display_name": display_name,
        "nip05": nip05,
        "picture": "https://example.com/avatar.png",
    })
    .to_string();
    kernel.ingest_profile(NostrEvent {
        id: "0".repeat(64),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 0,
        tags: Vec::new(),
        content,
        sig: String::new(),
    });
}

/// Ingest a real signed kind:1 note so its author surfaces in `visible_items()`
/// → `mention_profiles`. The `diag-firehose-` sub_id bypasses the
/// `timeline_authors` gate.
fn inject_note(kernel: &mut Kernel, keys: &::nostr::Keys, content: &str) {
    let event = signed_note(keys, content, 1_700_000_000);
    kernel.ingest_timeline_event(RelayRole::Content, RELAY, "diag-firehose-resolved", event);
}

/// 1. Empty case — a fresh kernel with no claims, no open view, and no visible
/// items emits `resolved_profiles` as a present-but-empty object (D1: never
/// absent).
#[test]
fn resolved_profiles_present_and_empty_on_fresh_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let snapshot = kernel.make_update_value_for_test(true);
    let resolved = &snapshot["projections"]["resolved_profiles"];
    assert!(
        resolved.is_object(),
        "resolved_profiles must always be present as an object (D1) — got {resolved:?}"
    );
    assert_eq!(
        resolved.as_object().map(serde_json::Map::len),
        Some(0),
        "resolved_profiles must be empty `{{}}` on a fresh kernel"
    );
}

/// 2. `claimed_profiles` wins over `mention_profiles` for the same pubkey.
/// The pubkey is both claimed AND surfaced in a visible note (so it appears in
/// `mention_profiles`). The merged entry must carry the claimed card's `nip05`
/// — a field `from_mention` always hardcodes to `""`, so a non-empty value can
/// only have come from the claimed card.
#[test]
fn claimed_profiles_wins_over_mention_profiles() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let pk = keys.public_key().to_hex();

    // kind:0 with a non-empty nip05 — the discriminating field.
    ingest_profile_with(
        &mut kernel,
        &pk,
        1_000,
        "Claimed User",
        "claimed@nip05.example",
    );
    // Surface `pk` in mention_profiles via a visible note (real ingest path).
    inject_note(&mut kernel, &keys, "a note");
    // Claim the profile so it lands in claimed_profiles.
    let _ = kernel.claim_profile(pk.clone(), "view-0".to_string(), true);

    let snapshot = kernel.make_update_value_for_test(true);

    // Precondition: the same pubkey is in BOTH source projections.
    assert!(
        snapshot["projections"]["claimed_profiles"][&pk].is_object(),
        "precondition: pk must be in claimed_profiles"
    );
    assert!(
        snapshot["projections"]["mention_profiles"][&pk].is_object(),
        "precondition: pk must be in mention_profiles — got {:?}",
        snapshot["projections"]["mention_profiles"]
    );

    let entry = &snapshot["projections"]["resolved_profiles"][&pk];
    assert!(entry.is_object(), "resolved_profiles[pk] must be present");
    assert_eq!(
        entry["nip05"], "claimed@nip05.example",
        "resolved entry must carry the CLAIMED card's nip05 — mention's from_mention \
         always hardcodes nip05 to \"\", so a non-empty value proves claimed won"
    );
}

/// 3. `mention_profiles` fills gaps — a pubkey that is NOT claimed and is NOT
/// the author-view subject, but surfaces in a visible note, appears in
/// `resolved_profiles` built via `ProfileCard::from_mention` (empty
/// `nip05`/`about`, `lnurl: None`).
#[test]
fn mention_profiles_fill_gaps() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let pk = keys.public_key().to_hex();

    ingest_profile_with(
        &mut kernel,
        &pk,
        1_000,
        "Mention User",
        "mention@nip05.example",
    );
    inject_note(&mut kernel, &keys, "mention note");

    let snapshot = kernel.make_update_value_for_test(true);

    // Not claimed, not an open author view → only mention_profiles carries it.
    assert!(
        snapshot["projections"]["claimed_profiles"][&pk].is_null(),
        "precondition: pk must NOT be in claimed_profiles"
    );
    assert!(
        snapshot["projections"]["author_view"].is_null(),
        "precondition: no author view is open"
    );
    assert!(
        snapshot["projections"]["mention_profiles"][&pk].is_object(),
        "precondition: pk must be in mention_profiles — got {:?}",
        snapshot["projections"]["mention_profiles"]
    );

    let entry = &snapshot["projections"]["resolved_profiles"][&pk];
    assert!(
        entry.is_object(),
        "resolved_profiles[pk] must be present (filled from mention_profiles)"
    );
    assert_eq!(
        entry["display_name"], "Mention User",
        "the mention-derived card must carry the kind:0 display name"
    );
    assert_eq!(
        entry["nip05"], "",
        "from_mention always emits an empty nip05 — the mention projection never carries it"
    );
    assert!(
        entry["lnurl"].is_null(),
        "from_mention always emits lnurl: None"
    );
    assert_eq!(
        entry["has_profile"], true,
        "has_profile is true when a display field is present"
    );
}

/// 4. `author_view.profile` is only-if-absent — a pubkey that is BOTH claimed
/// AND the open author-view subject resolves to the CLAIMED card, not the
/// author-view card. The discriminating field is `npub`: the claimed loop
/// passes `Some(to_npub(pk))` (bech32), while `author_view` passes `None` and
/// `profile_card_for` falls back to the raw hex pubkey. The subject carries a
/// kind:0 so `has_profile == true` and the author_view branch actually attempts
/// the insert (and is correctly blocked).
#[test]
fn author_view_is_only_if_absent_claimed_wins() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let pk = keys.public_key().to_hex();

    // kind:0 so author_view.profile.has_profile == true (branch executes).
    ingest_profile_with(
        &mut kernel,
        &pk,
        1_000,
        "Author User",
        "author@nip05.example",
    );
    // A note by `pk` so the author view has items.
    inject_note(&mut kernel, &keys, "author's own note");
    // Claim it (highest precedence) and open its author view (second).
    let _ = kernel.claim_profile(pk.clone(), "view-0".to_string(), true);
    let _ = kernel.open_author(pk.clone(), std::collections::BTreeSet::from([1u32, 6u32]), false);

    let snapshot = kernel.make_update_value_for_test(true);

    // Precondition: author_view is open for this pubkey with a real profile,
    // and its card uses the raw-hex npub fallback.
    assert_eq!(
        snapshot["projections"]["author_view"]["pubkey"], pk,
        "precondition: author_view must be open for pk"
    );
    assert_eq!(
        snapshot["projections"]["author_view"]["profile"]["has_profile"], true,
        "precondition: author_view.profile must be real so the only-if-absent branch runs"
    );
    assert_eq!(
        snapshot["projections"]["author_view"]["profile"]["npub"], pk,
        "precondition: author_view card uses the raw-hex npub fallback"
    );

    let entry = &snapshot["projections"]["resolved_profiles"][&pk];
    assert!(entry.is_object(), "resolved_profiles[pk] must be present");
    assert_eq!(
        entry["npub"],
        to_npub(&pk),
        "resolved entry must carry the CLAIMED card's bech32 npub — author_view's card \
         uses the raw-hex fallback, so the bech32 form proves claimed won (only-if-absent)"
    );
}

/// 5. `author_view.profile` FILLS a gap (positive tier-2 coverage) — a pubkey
/// that is NEITHER claimed NOR mentioned, but IS the open author-view subject
/// with a real kind:0, appears in `resolved_profiles` via the author_view tier.
/// The subject has NO notes, so `av.items` is empty and the pubkey never enters
/// `mention_profiles` (projections.rs:222–228) — the only source that can fill
/// the entry is tier 2. The discriminating field is `nip05`: a full
/// `profile_card_for` card carries the kind:0 value, which neither an absent
/// claimed entry nor a `from_mention` card could supply. Guards against the
/// author_view block being deleted without a test noticing.
#[test]
fn author_view_profile_fills_gap_when_unclaimed_and_unmentioned() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let pk = keys.public_key().to_hex();

    // kind:0 ONLY — no note, so `pk` never enters visible_items/mention_profiles.
    ingest_profile_with(
        &mut kernel,
        &pk,
        1_000,
        "Author User",
        "author@nip05.example",
    );
    // Open the author view but do NOT claim — tier 1 stays empty for `pk`.
    let _ = kernel.open_author(pk.clone(), std::collections::BTreeSet::from([1u32, 6u32]), false);

    let snapshot = kernel.make_update_value_for_test(true);

    // Precondition: only the author_view tier carries this pubkey.
    assert!(
        snapshot["projections"]["claimed_profiles"][&pk].is_null(),
        "precondition: pk must NOT be in claimed_profiles"
    );
    assert!(
        snapshot["projections"]["mention_profiles"][&pk].is_null(),
        "precondition: pk must NOT be in mention_profiles (no notes) — got {:?}",
        snapshot["projections"]["mention_profiles"]
    );
    assert_eq!(
        snapshot["projections"]["author_view"]["profile"]["has_profile"], true,
        "precondition: author_view.profile must be real so the tier-2 branch fires"
    );

    let entry = &snapshot["projections"]["resolved_profiles"][&pk];
    assert!(
        entry.is_object(),
        "resolved_profiles[pk] must be present (filled from author_view.profile)"
    );
    assert_eq!(
        entry["nip05"], "author@nip05.example",
        "resolved entry must carry the author_view card's full kind:0 nip05 — only a full \
         ProfileCard (tier 2 here, claimed empty) supplies it; a from_mention card never would"
    );
}
