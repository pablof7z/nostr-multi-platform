//! ADR-0063 Lane C (#1671) — the TYPED ROW-PAYLOAD decode proof.
//!
//! Lane A built the per-key host cache but left the host surface returning raw
//! `Data` / `ByteArray`: its default decoder accepted any non-empty payload
//! (codex Lane-A review invariant #4). Lane C wires the REAL typed decoder. This
//! module is the Rust-side proof that the contract is byte-for-byte honest:
//!
//! - Each `refs.profile` row payload that `Kernel::ref_profile_row_payload`
//!   (→ `encode_profile`) emits decodes — through the SAME `decode_profile`
//!   reader the generated Swift `KeyedRefCache.decodeProfileRow` calls (it is
//!   the `getCheckedRoot<nmp_kernel_ProfileSnapshot>` twin) — to the expected
//!   `ProfileCard` fields.
//! - `profile.ref` carries the NARROW field set `{pubkey, display_name,
//!   picture_url}` while `profile.card` carries the FULL card (nip05 / about /
//!   banner / website / lud16 / …). This proves the host typed accessor returns
//!   a real shape-narrowed value, not the old raw-bytes passthrough.
//! - Each `refs.event` row payload (`Kernel::ref_event_row_payload` →
//!   `encode_claimed_events` of a single entry) decodes — through
//!   `decode_claimed_events`, the `nmp_kernel_ClaimedEventsSnapshot` reader twin
//!   the generated `decodeEventRow` calls — to the expected single event.
//!
//! Pulled in via `#[path]` from `kernel::update` (same pattern as
//! `refs_glue_integration_tests`) to keep both files under the 500-LOC cap.

use super::super::nostr::NostrEvent;
use super::super::refs::{EventShape, ProfileShape};
use super::super::refs::{RefLiveness, RefNamespace, RefShape};
use super::super::typed_projections::{decode_claimed_events, decode_profile};
use super::super::Kernel;
use crate::refs::RefRowDeltaTracker;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// Inject a RICH kind:0 so the `card` vs `ref` narrowing is provable: it carries
/// every wide-only field (`about` / `nip05` / `banner` / `website` / `lud16`) on
/// top of the narrow `{display_name, picture}` set.
fn inject_rich_kind0(kernel: &mut Kernel, pubkey: &str) {
    let content = serde_json::json!({
        "display_name": "Alice",
        "picture": "https://example.com/a.png",
        "about": "hello from alice",
        "nip05": "alice@example.com",
        "banner": "https://example.com/banner.png",
        "website": "https://alice.example",
        "lud16": "alice@walletofsatoshi.com",
    })
    .to_string();
    kernel.inject_profile(NostrEvent {
        id: "0".repeat(64),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: Vec::new(),
        content,
        sig: String::new(),
    });
}

/// A real signed kind:1 note (passes `verify_and_persist`, lands in `events`).
fn signed_note(keys: &::nostr::Keys, body: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let ev = EventBuilder::text_note(body)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign note");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// The exact per-key row payload bytes the producer emits for one namespace,
/// keyed by row key — a fresh `RefRowDeltaTracker` baseline over the kernel's own
/// `RefRowRevSource` (the SAME path `make_update` ships on the wire).
fn baseline_payloads(
    kernel: &Kernel,
    namespace: &str,
) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut tracker = RefRowDeltaTracker::new();
    tracker
        .build_baseline(namespace, kernel)
        .rows
        .into_iter()
        .map(|row| (row.key, row.payload))
        .collect()
}

/// PROFILE: a `profile.card` row carries the FULL card; a `profile.ref` row
/// carries ONLY `{pubkey, display_name, picture_url}` — and BOTH decode through
/// the production `decode_profile` reader (the host typed-decoder twin), not the
/// old raw-bytes passthrough.
#[test]
fn refs_profile_row_typed_decode_card_full_ref_narrow() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);

    let alice = hex64("a11ce");
    let bob = hex64("b0b");

    // Alice demands the FULL card; Bob demands the narrow feed-avatar ref.
    kernel.resolve_ref(
        RefNamespace::Profile,
        alice.clone(),
        "profile-screen".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Profile,
        bob.clone(),
        "feed-avatar".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    inject_rich_kind0(&mut kernel, &alice);
    inject_rich_kind0(&mut kernel, &bob);

    let payloads = baseline_payloads(&kernel, "profile");

    // The host typed decoder (decode_profile twin) yields a real ProfileCard —
    // proving the typed decode is REAL, not the Lane-A non-empty passthrough.
    let card = decode_profile(payloads.get(&alice).expect("alice row present"))
        .expect("alice row decodes as a ProfileCard (typed, not raw)");
    let card_ref = decode_profile(payloads.get(&bob).expect("bob row present"))
        .expect("bob row decodes as a ProfileCard (typed, not raw)");

    // profile.card → FULL field set populated from kind:0.
    assert_eq!(card.pubkey, alice);
    assert_eq!(card.display_name.as_deref(), Some("Alice"));
    assert_eq!(
        card.picture_url.as_deref(),
        Some("https://example.com/a.png")
    );
    assert_eq!(card.nip05, "alice@example.com");
    assert_eq!(card.about, "hello from alice");
    assert_eq!(
        card.banner.as_deref(),
        Some("https://example.com/banner.png")
    );
    assert_eq!(card.website.as_deref(), Some("https://alice.example"));
    assert_eq!(card.lud16.as_deref(), Some("alice@walletofsatoshi.com"));

    // profile.ref → NARROW: only {pubkey, display_name, picture_url}. Every
    // wide-only field is dropped by the D5 narrowing in `ref_profile_row_payload`
    // and therefore decodes empty/None here.
    assert_eq!(card_ref.pubkey, bob);
    assert_eq!(card_ref.display_name.as_deref(), Some("Alice"));
    assert_eq!(
        card_ref.picture_url.as_deref(),
        Some("https://example.com/a.png")
    );
    assert_eq!(card_ref.nip05, "", "ref narrows away nip05");
    assert_eq!(card_ref.about, "", "ref narrows away about");
    assert_eq!(card_ref.banner, None, "ref narrows away banner");
    assert_eq!(card_ref.website, None, "ref narrows away website");
    assert_eq!(card_ref.lud16, None, "ref narrows away lud16");
    assert_eq!(card_ref.name, None, "ref narrows away name");
}

/// EVENT: a `refs.event` row payload decodes through the production
/// `decode_claimed_events` reader (the host `decodeEventRow` twin) to the single
/// expected event — proving the typed event accessor is real, not raw bytes.
#[test]
fn refs_event_row_typed_decode_single_entry() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);

    let keys = ::nostr::Keys::generate();
    let note = signed_note(&keys, "lane c event row", 1_700_000_500);
    let note_id = note.id.clone();

    kernel.resolve_ref(
        RefNamespace::Event,
        note_id.clone(),
        "embed-1".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example/", "sub", note);

    let payloads = baseline_payloads(&kernel, "event");
    let model = decode_claimed_events(payloads.get(&note_id).expect("event row present"))
        .expect("event row decodes as a ClaimedEvents buffer (typed, not raw)");

    // The single-entry contract the host `refRowEvent` glue relies on.
    assert_eq!(model.entries.len(), 1, "refs.event row carries exactly one entry");
    let (key, row) = &model.entries[0];
    assert_eq!(key, &note_id);
    assert_eq!(row.id, note_id);
    assert_eq!(row.author_pubkey, keys.public_key().to_hex());
    assert_eq!(row.content, "lane c event row");
    assert_eq!(row.kind, 1);
}
