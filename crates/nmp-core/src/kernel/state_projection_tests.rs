//! Behavioral coverage for the kernel state-projection layer.
//!
//! ## What this file covers vs. what already exists
//!
//! `kernel/ingest_tests.rs` verifies the *in-memory* effect of ingest: after a
//! kind:0 / kind:3 / kind:10002 / kind:1, the right HashMap / VecDeque is
//! mutated. That is the reducer half of the kernel.
//!
//! This file covers the OTHER half — the **projection boundary**. The kernel's
//! `make_update()` serializes internal state into the JSON snapshot the FFI
//! returns to the Swift / Kotlin shell. A field that the reducer updates but the
//! projection never reads is invisible to users; a field the projection reads
//! from the wrong place shows stale state. Both are silent bugs that the
//! state-level ingest tests cannot catch.
//!
//! Every test here drives a real ingest / lifecycle transition, then calls
//! `kernel.make_update(true)` and asserts on the parsed `serde_json::Value` —
//! i.e. exactly the bytes that cross the C-ABI. `KernelUpdate` is `Serialize`
//! only (no `Deserialize`), so the assertions parse the JSON dynamically rather
//! than round-tripping the typed struct.

use super::*;
use crate::publish::{InMemoryPublishStore, PerRelayState, PublishRecord, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{SignedEvent, UnsignedEvent};
use std::sync::Arc;

// 64-char hex pubkeys / ids — the kernel's `is_hex_pubkey` / `is_hex_id`
// gates require exactly 64 ascii hex digits.
const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FOLLOW_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const FOLLOW_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const NOTE_ID: &str = "e1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2";

/// Drive `make_update` and parse the emitted JSON snapshot.
fn snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let json = kernel.make_update(true);
    serde_json::from_str(&json).expect("kernel snapshot must be valid JSON")
}

/// Ingest a kind:1 note through the `diag-firehose-` test path so it lands in
/// both the `events` read-cache and the `timeline` ordering projection without
/// needing the author to be a followed `timeline_authors` member.
fn ingest_note(kernel: &mut Kernel, id: &str, author: &str, created_at: u64, content: &str) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw),
    );
    kernel.sort_timeline_deferred();
}

// ─── schema_version projection ───────────────────────────────────────────────

/// Every emitted snapshot MUST carry a `schema_version` field equal to the
/// canonical `SNAPSHOT_SCHEMA_VERSION`. Without it a version mismatch between a
/// shipped `.a` and the host fails silently — the host decodes renamed/removed
/// fields, gets wrong/null data, and shows a broken UI with no diagnostic
/// signal. This pins the field's presence on the actual on-wire bytes.
#[test]
fn snapshot_carries_schema_version() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let snap = snapshot(&mut kernel);
    assert_eq!(
        snap["schema_version"].as_u64(),
        Some(u64::from(crate::update_envelope::SNAPSHOT_SCHEMA_VERSION)),
        "every snapshot must stamp the canonical schema_version",
    );
}

// ─── last_tick_ms liveness heartbeat projection ──────────────────────────────

/// Every emitted snapshot MUST carry a non-zero `last_tick_ms` (Unix-epoch
/// milliseconds), and the value MUST advance across successive emissions. A
/// shell watches this field to detect actor-thread death: a `dispatch_command`
/// panic is deliberately not caught, so it manifests as the update channel
/// going permanently silent. A frozen `last_tick_ms` is the only observable
/// signal of that otherwise-invisible freeze. This pins both the field's
/// presence on the on-wire bytes and its monotonic advance.
#[test]
fn snapshot_carries_advancing_last_tick_ms() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let first = snapshot(&mut kernel);
    let first_tick = first["last_tick_ms"]
        .as_u64()
        .expect("every snapshot must stamp a numeric last_tick_ms");
    assert!(
        first_tick > 0,
        "last_tick_ms must be a real Unix-epoch millisecond stamp, not zero",
    );

    let second = snapshot(&mut kernel);
    let second_tick = second["last_tick_ms"]
        .as_u64()
        .expect("every snapshot must stamp a numeric last_tick_ms");
    assert!(
        second_tick >= first_tick,
        "last_tick_ms must advance (or hold) across emissions, never regress; \
         a frozen value is the actor-thread-death signal",
    );
}

// ─── timeline events → items[] projection ────────────────────────────────────

/// A kind:1 ingest must surface in the snapshot's `items[]` array — the list the
/// UI renders as the timeline. Before ingest the array is empty; after ingest it
/// carries exactly one item carrying the ingested id and content.
#[test]
fn timeline_event_appears_in_snapshot_items_after_ingest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Cold snapshot: no notes ingested yet → empty timeline projection.
    let before = snapshot(&mut kernel);
    assert_eq!(
        before["items"].as_array().map(Vec::len),
        Some(0),
        "a fresh kernel must project an empty `items[]` timeline",
    );

    ingest_note(
        &mut kernel,
        NOTE_ID,
        ACCOUNT,
        1_700_000_000,
        "hello timeline",
    );

    let after = snapshot(&mut kernel);
    let items = after["items"]
        .as_array()
        .expect("`items` must be a JSON array");
    assert_eq!(
        items.len(),
        1,
        "an ingested kind:1 must project exactly one timeline item",
    );
    assert_eq!(
        items[0]["id"].as_str(),
        Some(NOTE_ID),
        "the projected timeline item must carry the ingested event id",
    );
    assert_eq!(
        items[0]["content"].as_str(),
        Some("hello timeline"),
        "the projected timeline item must carry the ingested content",
    );
    // The diagnostic metrics block must agree with the projected list.
    assert_eq!(
        after["metrics"]["note_events"].as_u64(),
        Some(1),
        "metrics.note_events must count the ingested kind:1",
    );
    assert_eq!(
        after["metrics"]["visible_items"].as_u64(),
        Some(1),
        "metrics.visible_items must agree with the projected items[] length",
    );
}

/// The `visible_limit` cap must be honoured by the projection: ingesting more
/// notes than the limit projects at most `visible_limit` items, never the full
/// cache. A projection that ignored the cap would blow the snapshot payload.
#[test]
fn timeline_projection_respects_visible_limit() {
    const LIMIT: usize = 3;
    let mut kernel = Kernel::new(LIMIT);

    for i in 0..(LIMIT + 4) {
        // Distinct 64-hex ids per note (vary the leading byte).
        let id = format!("{:0<64}", format!("{:02x}note", i));
        ingest_note(&mut kernel, &id, ACCOUNT, 1_700_000_000 + i as u64, "n");
    }

    let after = snapshot(&mut kernel);
    assert_eq!(
        after["items"].as_array().map(Vec::len),
        Some(LIMIT),
        "the projection must clamp items[] to visible_limit, not dump the cache",
    );
}

// ─── kind:0 profile metadata → profile card projection ───────────────────────

/// A kind:0 ingest for the active account must refine the snapshot's `profile`
/// card in place: `display`, `picture_url`, and the `source` discriminator all
/// flip from placeholder defaults to the kind:0 values.
#[test]
fn profile_metadata_appears_in_snapshot_after_kind0_ingest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // `profile_card()` keys off `active_account`; without it the card stays a
    // pubkey-less placeholder no matter what kind:0 arrives.
    kernel.active_account = Some(ACCOUNT.to_string());

    // Cold snapshot: no kind:0 → placeholder card with the identicon source.
    let before = snapshot(&mut kernel);
    assert_eq!(
        before["profile"]["source"].as_str(),
        Some("placeholder"),
        "before any kind:0 the profile card source must be `placeholder`",
    );

    // Ingest a kind:0 carrying real metadata.
    let event = nostr::NostrEvent {
        id: "0000000000000000000000000000000000000000000000000000000000000010".to_string(),
        pubkey: ACCOUNT.to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: vec![],
        content: r#"{"display_name":"Satoshi","nip05":"sat@example.com","about":"hi there","picture":"https://example.com/sat.png"}"#
            .to_string(),
        sig: String::new(),
    };
    kernel.ingest_profile(event);

    let after = snapshot(&mut kernel);
    let card = &after["profile"];
    assert_eq!(
        card["display"].as_str(),
        Some("Satoshi"),
        "kind:0 display_name must be projected into profile.display",
    );
    assert_eq!(
        card["picture_url"].as_str(),
        Some("https://example.com/sat.png"),
        "kind:0 picture must be projected into profile.picture_url",
    );
    assert_eq!(
        card["nip05"].as_str(),
        Some("sat@example.com"),
        "kind:0 nip05 must be projected into profile.nip05",
    );
    assert_eq!(
        card["source"].as_str(),
        Some("kind0"),
        "with a real picture the card source must flip to `kind0` (ADR-0017)",
    );
    // The diagnostic profile counter must agree.
    assert_eq!(
        after["metrics"]["profile_events"].as_u64(),
        Some(1),
        "metrics.profile_events must count the cached kind:0",
    );
}

#[test]
fn profile_card_does_not_project_metadata_source() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    assert!(
        snap["profile"].get("metadata_source").is_none(),
        "profile cards must not expose a second metadata-source discriminator"
    );
}

#[test]
fn profile_card_projects_pending_kind0_publish_intent_after_restart() {
    let publish_store = Arc::new(InMemoryPublishStore::new());
    publish_store
        .upsert(&PublishRecord {
            handle: "pending-profile".to_string(),
            event: SignedEvent {
                id: "0000000000000000000000000000000000000000000000000000000000000040".to_string(),
                sig: "a".repeat(128),
                unsigned: UnsignedEvent {
                    pubkey: ACCOUNT.to_string(),
                    kind: 0,
                    tags: Vec::new(),
                    content: r#"{"display_name":"Pending Profile"}"#.to_string(),
                    created_at: 1_700_000_200,
                },
            },
            per_relay: vec![("wss://relay.test".to_string(), PerRelayState::Pending)],
            pending_retries: Vec::new(),
        })
        .expect("seed pending profile intent");
    let mut kernel = Kernel::with_publish_store(
        DEFAULT_VISIBLE_LIMIT,
        Arc::clone(&publish_store) as Arc<dyn PublishStore>,
    );
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    assert_eq!(
        snap["profile"]["display"].as_str(),
        Some("Pending Profile"),
        "pending kind:0 publish intent must survive kernel reconstruction"
    );
    assert_eq!(
        snap["metrics"]["profile_events"].as_u64(),
        Some(0),
        "pending profile intent is not a relay-ingested kind:0"
    );
}

#[test]
fn author_view_projects_edit_action_for_active_profile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.open_author(ACCOUNT.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("edit_profile"));
    assert_eq!(action["label"].as_str(), Some("Edit"));
    assert_eq!(action["target_pubkey"].as_str(), Some(ACCOUNT));
}

#[test]
fn author_view_projects_follow_action_for_non_active_profile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.open_author(FOLLOW_A.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("follow"));
    assert_eq!(action["label"].as_str(), Some("Follow"));
    assert_eq!(action["target_pubkey"].as_str(), Some(FOLLOW_A));
}

#[test]
fn author_view_projects_unfollow_when_active_contacts_include_author() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.prepopulate_seed_contacts(ACCOUNT.to_string(), vec![FOLLOW_A.to_string()]);
    kernel.open_author(FOLLOW_A.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("unfollow"));
    assert_eq!(action["label"].as_str(), Some("Unfollow"));
    assert_eq!(action["target_pubkey"].as_str(), Some(FOLLOW_A));
}

/// A kind:0 author's note in the timeline must show the kind:0 display/avatar
/// in its `items[]` row — proving the profile join happens at projection time,
/// not just on the standalone `profile` card. Order of ingest must not matter:
/// here the note is ingested BEFORE the kind:0 (the realistic relay race).
#[test]
fn timeline_item_picks_up_profile_after_later_kind0_ingest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Note arrives first, with no profile yet → placeholder avatar source.
    ingest_note(&mut kernel, NOTE_ID, ACCOUNT, 1_700_000_000, "a note");
    let before = snapshot(&mut kernel);
    assert_eq!(
        before["items"][0]["author_avatar_source"].as_str(),
        Some("placeholder"),
        "before kind:0 the timeline item avatar source must be `placeholder`",
    );

    // kind:0 for that author arrives later.
    let event = nostr::NostrEvent {
        id: "0000000000000000000000000000000000000000000000000000000000000020".to_string(),
        pubkey: ACCOUNT.to_string(),
        created_at: 1_700_000_100,
        kind: 0,
        tags: vec![],
        content: r#"{"display_name":"Late Profile","picture":"https://example.com/p.png"}"#
            .to_string(),
        sig: String::new(),
    };
    kernel.ingest_profile(event);

    let after = snapshot(&mut kernel);
    assert_eq!(
        after["items"][0]["author_display"].as_str(),
        Some("Late Profile"),
        "the timeline item must pick up the kind:0 display name in-place",
    );
    assert_eq!(
        after["items"][0]["author_avatar_source"].as_str(),
        Some("kind0"),
        "the timeline item avatar source must refine to `kind0` after kind:0",
    );
}

// ─── kind:3 contacts → metrics projection ────────────────────────────────────

/// A kind:3 ingest for the active account must surface its follow count in the
/// snapshot. There is no top-level `contacts` field — the projection is
/// `metrics.contacts_authors` (every cached kind:3's follows summed) and, for
/// the active account, `metrics.timeline_authors` (the follow-feed author set).
#[test]
fn contact_list_appears_in_snapshot_metrics_after_kind3_ingest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    // Cold snapshot: no kind:3 → zero followed authors projected.
    let before = snapshot(&mut kernel);
    assert_eq!(
        before["metrics"]["contacts_authors"].as_u64(),
        Some(0),
        "before any kind:3 the projected contacts_authors count must be zero",
    );

    let event = nostr::NostrEvent {
        id: "0000000000000000000000000000000000000000000000000000000000000030".to_string(),
        pubkey: ACCOUNT.to_string(),
        created_at: 1_700_000_000,
        kind: 3,
        tags: vec![
            vec!["p".to_string(), FOLLOW_A.to_string()],
            vec!["p".to_string(), FOLLOW_B.to_string()],
        ],
        content: String::new(),
        sig: String::new(),
    };
    kernel.ingest_contacts(event);

    let after = snapshot(&mut kernel);
    assert_eq!(
        after["metrics"]["contacts_authors"].as_u64(),
        Some(2),
        "metrics.contacts_authors must project the two kind:3 follows",
    );
    // Active-account kind:3 also rebuilds the follow-feed author set: the two
    // follows plus the active account itself (so the user's own notes show).
    assert_eq!(
        after["metrics"]["timeline_authors"].as_u64(),
        Some(3),
        "active-account kind:3 must project the follows + self into \
         metrics.timeline_authors",
    );
}

// ─── relay connection events → relay status projection ───────────────────────

/// A relay connection transition must surface in the snapshot's `relay_status`
/// (the headline content relay) and `relay_statuses[]` (every lane). A
/// projection that read a stale field would show "disconnected" after a real
/// connect — exactly the kind of display bug this layer must not have.
#[test]
fn relay_status_appears_in_snapshot_after_connection_events() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // `start()` seeds `started_at` so `elapsed_ms` (and thus
    // `last_connected_at_ms`) can resolve a real timestamp.
    kernel.start();

    // Default lane state: not connected.
    let before = snapshot(&mut kernel);
    assert_ne!(
        before["relay_status"]["connection"].as_str(),
        Some("connected"),
        "a fresh content relay lane must not project as connected",
    );

    // Drive the connecting → connected transition on the content lane.
    kernel.relay_connecting(RelayRole::Content);
    let connecting = snapshot(&mut kernel);
    assert_eq!(
        connecting["relay_status"]["connection"].as_str(),
        Some("connecting"),
        "relay_connecting must project `connecting` onto relay_status",
    );

    kernel.relay_connected(RelayRole::Content);
    let connected = snapshot(&mut kernel);
    assert_eq!(
        connected["relay_status"]["connection"].as_str(),
        Some("connected"),
        "relay_connected must project `connected` onto relay_status",
    );
    assert!(
        connected["relay_status"]["last_connected_at_ms"].is_u64(),
        "a connected relay must project a numeric last_connected_at_ms",
    );

    // The content lane must also be present (and connected) in relay_statuses[].
    let statuses = connected["relay_statuses"]
        .as_array()
        .expect("relay_statuses must be a JSON array");
    let content = statuses
        .iter()
        .find(|s| s["role"].as_str() == Some("content"))
        .expect("relay_statuses must include the content lane");
    assert_eq!(
        content["connection"].as_str(),
        Some("connected"),
        "the content lane in relay_statuses[] must agree with relay_status",
    );

    // A subsequent close must project back to a non-connected state — a
    // projection stuck on the stale `connected` value is the bug under test.
    // (`relay_closed_all` — the global-teardown path — projects the lane
    // `closed` regardless of per-URL socket bookkeeping.)
    kernel.relay_closed_all(RelayRole::Content);
    let closed = snapshot(&mut kernel);
    assert_eq!(
        closed["relay_status"]["connection"].as_str(),
        Some("closed"),
        "relay_closed must project `closed`, never a stale `connected`",
    );
}

// ─── NIP-47 wallet status → wallet projection (appear / disappear) ───────────

/// The `wallet_status` snapshot key must appear when a wallet connects and
/// disappear (`null`) when it disconnects. NIP-47 NWC is an app noun gated
/// behind the `wallet` Cargo feature (on by default), so this test is gated
/// the same way — under `--no-default-features` the key does not exist.
#[cfg(feature = "wallet")]
#[test]
fn wallet_status_appears_and_clears_in_snapshot_on_connect_disconnect() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // No wallet connected → the projected key is null.
    let before = snapshot(&mut kernel);
    assert!(
        before
            .get("wallet_status")
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "with no wallet connected the wallet_status key must be null",
    );

    // Connect a wallet.
    kernel.set_wallet_status(Some(super::WalletStatus {
        status: "ready".to_string(),
        relay_url: "wss://wallet.example/".to_string(),
        wallet_npub: "npub1walletexample".to_string(),
        balance_msats: Some(21_000),
    }));
    let connected = snapshot(&mut kernel);
    let wallet = &connected["wallet_status"];
    assert_eq!(
        wallet["status"].as_str(),
        Some("ready"),
        "a connected wallet must project status=ready",
    );
    assert_eq!(
        wallet["relay_url"].as_str(),
        Some("wss://wallet.example/"),
        "the wallet relay URL must be projected",
    );
    assert_eq!(
        wallet["balance_msats"].as_u64(),
        Some(21_000),
        "the wallet balance must be projected when known",
    );

    // Disconnect → the projection must clear back to null, not retain a stale
    // `ready` card after the wallet is gone.
    kernel.set_wallet_status(None);
    let disconnected = snapshot(&mut kernel);
    assert!(
        disconnected["wallet_status"].is_null(),
        "after disconnect the wallet_status projection must clear to null",
    );
}
