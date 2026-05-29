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
//! `kernel.make_update_json_for_test(true)` and asserts on the parsed `serde_json::Value` —
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
    let json = kernel.make_update_json_for_test(true);
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
    // D0: the timeline is no longer a typed `KernelSnapshot.items` field —
    // it is a built-in entry in the `projections` map under the key
    // `"timeline"`.
    let before = snapshot(&mut kernel);
    assert_eq!(
        before["projections"]["timeline"].as_array().map(Vec::len),
        Some(0),
        "a fresh kernel must project an empty `timeline[]`",
    );

    ingest_note(
        &mut kernel,
        NOTE_ID,
        ACCOUNT,
        1_700_000_000,
        "hello timeline",
    );

    let after = snapshot(&mut kernel);
    let items = after["projections"]["timeline"]
        .as_array()
        .expect("`projections.timeline` must be a JSON array");
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
        // The old format embedded literal "note" (non-hex chars); V-70
        // strengthened `is_structurally_valid()` to reject non-hex ids.
        let id = format!("{:064x}", i);
        ingest_note(&mut kernel, &id, ACCOUNT, 1_700_000_000 + i as u64, "n");
    }

    let after = snapshot(&mut kernel);
    assert_eq!(
        after["projections"]["timeline"].as_array().map(Vec::len),
        Some(LIMIT),
        "the projection must clamp timeline[] to visible_limit, not dump the cache",
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

    // Cold snapshot: no kind:0 → display_name / picture_url are `null`
    // (aim.md §2 — presentation layer owns the missing-kind:0 fallback).
    let before = snapshot(&mut kernel);
    assert!(
        before["projections"]["profile"]["display_name"].is_null(),
        "before any kind:0 the profile card display_name must be null",
    );
    assert!(
        before["projections"]["profile"]["picture_url"].is_null(),
        "before any kind:0 the profile card picture_url must be null",
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
    let card = &after["projections"]["profile"];
    assert_eq!(
        card["display_name"].as_str(),
        Some("Satoshi"),
        "kind:0 display_name must be projected into profile.display_name",
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
        snap["projections"]["profile"]
            .get("metadata_source")
            .is_none(),
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
            relay_reasons: Vec::new(),
        })
        .expect("seed pending profile intent");
    let mut kernel = Kernel::with_publish_store(
        DEFAULT_VISIBLE_LIMIT,
        Arc::clone(&publish_store) as Arc<dyn PublishStore>,
    );
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["profile"]["display_name"].as_str(),
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
fn publish_outbox_projects_pending_event_details_and_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = SignedEvent {
        id: "f".repeat(64),
        sig: "a".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "This note is still waiting for relays".to_string(),
            created_at: 1_700_000_000,
        },
    };

    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Explicit {
            relays: vec!["wss://outbox.test".to_string()],
        },
        None,
        0,
    );
    assert_eq!(outbound.len(), 1);

    let snap = snapshot(&mut kernel);
    // D0: the publish cluster is no longer a typed `KernelSnapshot` field —
    // `publish_outbox` is a built-in entry in the host-extensible `projections`
    // map.
    let outbox = snap["projections"]["publish_outbox"]
        .as_array()
        .expect("projections.publish_outbox must be an array");
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0]["handle"].as_str(), Some(signed.id.as_str()));
    assert_eq!(outbox[0]["kind"].as_u64(), Some(1));
    assert_eq!(outbox[0]["title"].as_str(), Some("Note"));
    assert_eq!(
        outbox[0]["preview"].as_str(),
        Some("This note is still waiting for relays")
    );
    assert_eq!(outbox[0]["status"].as_str(), Some("sending"));
    assert_eq!(outbox[0]["status_label"].as_str(), Some("Sending"));
    // aim.md §4.4 / §6 anti-pattern: the SF Symbol name is pre-classified in
    // Rust so the iOS shell never `switch`es on `kind` (a Nostr protocol
    // concept). Kind 1 (text note) → `"text.bubble"`.
    assert_eq!(outbox[0]["system_image"].as_str(), Some("text.bubble"));
    // RMP bible commandment #4: a row currently sending cannot be retried.
    // The kernel emits the decision; the shell binds it directly (no Swift
    // `if status != "sending"` branch).
    assert_eq!(outbox[0]["can_retry"].as_bool(), Some(false));
    // §6 anti-pattern #1: pluralization lives in Rust. Single relay → "1 relay";
    // the shell never reconstructs the plural with a ternary.
    assert!(
        outbox[0]["target_summary"]
            .as_str()
            .map(|s| s.starts_with("1 relay · "))
            .unwrap_or(false),
        "target_summary must pluralize server-side: got {:?}",
        outbox[0]["target_summary"]
    );
    assert_eq!(
        outbox[0]["relays"][0]["relay_url"].as_str(),
        Some("wss://outbox.test")
    );
    // Per-relay status label is pre-formatted (no Swift `.capitalized`).
    assert_eq!(
        outbox[0]["relays"][0]["status_label"].as_str(),
        Some("Sending")
    );
    // attempt == 1 on first send → "try 1" badge text comes from Rust.
    assert_eq!(
        outbox[0]["relays"][0]["attempt_label"].as_str(),
        Some("try 1")
    );
}

/// Per-relay rationale ("why was this relay targeted?") threads from the
/// outbox resolver all the way through to the JSON projection that crosses
/// the C-ABI. Apps render `relay_reason` verbatim — this test pins the field
/// to the resolver's exact string so a regression that drops the value (or
/// stops serializing it) is caught at the projection boundary.
///
/// Pairs with `relay_reasons_are_threaded_from_resolver_through_snapshot` in
/// `tests/publish_engine_relay_reasons.rs`, which pins the engine surface.
/// This test pins the *kernel projection* surface: the JSON the C-ABI emits.
#[test]
fn publish_outbox_projects_relay_reason_from_resolver() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = SignedEvent {
        id: "e".repeat(64),
        sig: "a".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "Why is this relay being targeted?".to_string(),
            created_at: 1_700_000_000,
        },
    };

    // `PublishTarget::Explicit` exercises the resolver's short-circuit lane —
    // the kernel's installed resolver (`Nip65OutboxResolver` /
    // `TestKind10002OutboxResolver`) returns
    // `ResolvedRelay { reason: "Explicit relay", .. }` for each URL.
    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Explicit {
            relays: vec!["wss://reason.test".to_string()],
        },
        None,
        0,
    );
    assert_eq!(outbound.len(), 1);

    let snap = snapshot(&mut kernel);
    let outbox = snap["projections"]["publish_outbox"]
        .as_array()
        .expect("projections.publish_outbox must be an array");
    assert_eq!(outbox.len(), 1);
    let relay = &outbox[0]["relays"][0];
    assert_eq!(relay["relay_url"].as_str(), Some("wss://reason.test"));
    assert_eq!(
        relay["relay_reason"].as_str(),
        Some("Explicit relay"),
        "kernel projection must surface the resolver's reason verbatim",
    );
}

/// `skip_serializing_if = "String::is_empty"` on `PublishOutboxRelay.relay_reason`
/// drops the field from the JSON payload when the engine has no reason on
/// file (older persisted rows resumed from disk, defaulted to empty). This
/// keeps the JSON shape backwards-compatible for apps that have not yet been
/// rebuilt against the new schema.
#[test]
fn publish_outbox_omits_empty_relay_reason_from_json() {
    // Seed a persisted publish row WITHOUT relay_reasons — the engine's
    // resume path defaults the rationale to empty for older serialised rows.
    let publish_store = Arc::new(InMemoryPublishStore::new());
    publish_store
        .upsert(&PublishRecord {
            handle: "legacy-row".to_string(),
            event: SignedEvent {
                id: "d".repeat(64),
                sig: "a".repeat(128),
                unsigned: UnsignedEvent {
                    pubkey: ACCOUNT.to_string(),
                    kind: 1,
                    tags: Vec::new(),
                    content: "Resumed from an older schema".to_string(),
                    created_at: 1_700_000_000,
                },
            },
            per_relay: vec![("wss://legacy.test".to_string(), PerRelayState::Pending)],
            pending_retries: Vec::new(),
            // Deliberately empty — simulates a record persisted before the
            // `relay_reasons` field existed.
            relay_reasons: Vec::new(),
        })
        .expect("seed legacy publish row");

    let mut kernel = Kernel::with_publish_store(
        DEFAULT_VISIBLE_LIMIT,
        Arc::clone(&publish_store) as Arc<dyn PublishStore>,
    );
    kernel.active_account = Some(ACCOUNT.to_string());
    // `with_publish_store` does NOT auto-resume; the kernel's actor entry
    // point calls `resume_publish_engine` separately. Mirror that flow so
    // the seeded row reaches the engine's in-flight set and surfaces on
    // the `publish_outbox` projection.
    let _ = kernel.resume_publish_engine();

    let snap = snapshot(&mut kernel);
    let outbox = snap["projections"]["publish_outbox"]
        .as_array()
        .expect("projections.publish_outbox must be an array");
    assert_eq!(outbox.len(), 1);
    let relay = &outbox[0]["relays"][0];
    assert_eq!(relay["relay_url"].as_str(), Some("wss://legacy.test"));
    // `skip_serializing_if = "String::is_empty"` MUST drop the field entirely
    // — not emit an empty string. Apps that haven't been recompiled against
    // the new schema rely on this to keep their existing Codable definitions
    // working unchanged.
    assert!(
        relay.get("relay_reason").is_none(),
        "empty relay_reason must NOT appear in the JSON (skip_serializing_if): \
         got {relay:?}",
    );
}

/// `outbox_summary` projects an empty-outbox headline + subtitle when nothing
/// is pending. §6 anti-pattern #1: the shell binds `title` / `subtitle`
/// strings directly — it never `.filter`-counts `publish_outbox` to derive
/// them.
#[test]
fn outbox_summary_projects_empty_state_strings() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let snap = snapshot(&mut kernel);
    let summary = &snap["projections"]["outbox_summary"];
    assert_eq!(summary["title"].as_str(), Some("Nothing waiting"));
    assert_eq!(
        summary["subtitle"].as_str(),
        Some("Your local outbox is clear.")
    );
    assert_eq!(summary["total"].as_u64(), Some(0));
    assert_eq!(summary["sending"].as_u64(), Some(0));
    assert_eq!(summary["retrying"].as_u64(), Some(0));
    assert_eq!(summary["queued"].as_u64(), Some(0));
    assert_eq!(summary["failed"].as_u64(), Some(0));
}

/// `outbox_summary` projects an "N pending publish(es)" headline and a per-status
/// subtitle when rows are in flight. Pins the strings the kernel emits so a
/// Swift refactor cannot quietly resurrect the §6 anti-pattern.
#[test]
fn outbox_summary_projects_sending_counters_and_strings() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = SignedEvent {
        id: "f".repeat(64),
        sig: "a".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "single sending row".to_string(),
            created_at: 1_700_000_000,
        },
    };

    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Explicit {
            relays: vec!["wss://outbox.test".to_string()],
        },
        None,
        0,
    );
    assert_eq!(outbound.len(), 1);

    let snap = snapshot(&mut kernel);
    let summary = &snap["projections"]["outbox_summary"];
    assert_eq!(summary["title"].as_str(), Some("1 pending publish"));
    assert_eq!(summary["subtitle"].as_str(), Some("1 currently sending."));
    assert_eq!(summary["total"].as_u64(), Some(1));
    assert_eq!(summary["sending"].as_u64(), Some(1));
    assert_eq!(summary["retrying"].as_u64(), Some(0));
}

#[test]
fn author_view_projects_edit_action_for_active_profile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.open_author(ACCOUNT.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["projections"]["author_view"]["primary_action"];
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
    let action = &snap["projections"]["author_view"]["primary_action"];
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
    let action = &snap["projections"]["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("unfollow"));
    assert_eq!(action["label"].as_str(), Some("Unfollow"));
    assert_eq!(action["target_pubkey"].as_str(), Some(FOLLOW_A));
}

/// Profile-action dispatch shape: follow/unfollow must carry the registered
/// ActionModule namespace + pre-serialised body so the shell wires the button
/// straight into `nmp_app_dispatch_action`. Mirrors aim.md §4.4 — writes flow
/// through registered actions, never through a Swift `switch action.kind`.
#[test]
fn profile_action_follow_carries_nmp_follow_dispatch_spec() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.open_author(FOLLOW_A.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["projections"]["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("follow"));
    assert_eq!(action["icon_name"].as_str(), Some("person.badge.plus"));
    let dispatch = &action["dispatch"];
    assert!(
        !dispatch.is_null(),
        "follow action must carry a dispatch spec"
    );
    assert_eq!(dispatch["namespace"].as_str(), Some("nmp.follow"));
    let body_json = dispatch["body_json"]
        .as_str()
        .expect("body_json must be a string");
    let body: serde_json::Value = serde_json::from_str(body_json).expect("body_json must parse");
    assert_eq!(body["pubkey"].as_str(), Some(FOLLOW_A));
}

#[test]
fn profile_action_unfollow_carries_nmp_unfollow_dispatch_spec() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.prepopulate_seed_contacts(ACCOUNT.to_string(), vec![FOLLOW_A.to_string()]);
    kernel.open_author(FOLLOW_A.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["projections"]["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("unfollow"));
    assert_eq!(action["icon_name"].as_str(), Some("person.badge.minus"));
    let dispatch = &action["dispatch"];
    assert_eq!(dispatch["namespace"].as_str(), Some("nmp.unfollow"));
    let body_json = dispatch["body_json"]
        .as_str()
        .expect("body_json must be a string");
    let body: serde_json::Value = serde_json::from_str(body_json).expect("body_json must parse");
    assert_eq!(body["pubkey"].as_str(), Some(FOLLOW_A));
}

/// `edit_profile` is the only local-UI intent — it opens a sheet, it is not a
/// write — so `dispatch` is explicitly absent. The shell branches on
/// presence-of-dispatch, not on `kind`, killing the Swift `switch action.kind`.
#[test]
fn profile_action_edit_profile_has_no_dispatch_spec() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.open_author(ACCOUNT.to_string(), false);

    let snap = snapshot(&mut kernel);
    let action = &snap["projections"]["author_view"]["primary_action"];
    assert_eq!(action["kind"].as_str(), Some("edit_profile"));
    assert_eq!(action["icon_name"].as_str(), Some("square.and.pencil"));
    assert!(
        action["dispatch"].is_null(),
        "edit_profile is a local-UI intent — must not carry a dispatch spec"
    );
}

/// `author_view.note_count_display` is the Rust-formatted post-count string
/// the shell binds verbatim — killing the `Text("\(items.count)")` Swift
/// interpolation that derived display state from the items array.
#[test]
fn author_view_carries_note_count_display_string() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    kernel.open_author(ACCOUNT.to_string(), false);

    let snap = snapshot(&mut kernel);
    let av = &snap["projections"]["author_view"];
    assert_eq!(av["note_count"].as_u64(), Some(0));
    assert_eq!(av["note_count_display"].as_str(), Some("0"));
}

/// `profile.npub_short` is the truncated copy-button form — Rust owns the
/// truncation policy (`<first10>…<last8>`), no Swift `truncatedNpub` helper.
#[test]
fn profile_card_carries_raw_pubkey_and_npub() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    let profile = &snap["projections"]["profile"];
    assert_eq!(
        profile["pubkey"].as_str(),
        Some(ACCOUNT),
        "profile.pubkey must carry the raw hex (aim.md §2)"
    );
    // npub stays for shells without a bech32 encoder; presentation owns
    // the abbreviation policy.
    assert!(
        profile["npub"].as_str().is_some(),
        "profile.npub must carry the bech32 encoding"
    );
    assert!(
        profile.get("npub_short").is_none(),
        "npub_short field was removed by aim.md §2 — shells own abbreviation"
    );
}

/// `projections.mention_profiles` mirrors the per-author display fields the
/// open author-view items carry — replacing the Swift `Dictionary(items.map …
/// MentionProfile(...))` derivation at `ProfileView.swift:28-40`. Empty `{}`
/// when no author view is open (D1: key always present).
#[test]
fn mention_profiles_projection_carries_each_author_in_author_view() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    ingest_note(&mut kernel, NOTE_ID, ACCOUNT, 1_700_000_000, "hello world");
    kernel.open_author(ACCOUNT.to_string(), false);

    let snap = snapshot(&mut kernel);
    let mp = &snap["projections"]["mention_profiles"];
    assert!(mp.is_object(), "mention_profiles must be a JSON object");
    let entry = &mp[ACCOUNT];
    assert!(
        !entry.is_null(),
        "mention_profiles must carry an entry for the author of the open author-view"
    );
    // Raw fields per aim.md §2: pubkey (hex), display_name + picture_url as Option<String>.
    assert_eq!(entry["pubkey"].as_str(), Some(ACCOUNT));
    assert!(
        entry["display_name"].is_null(),
        "no kind:0 → display_name null"
    );
    assert!(
        entry["picture_url"].is_null(),
        "no kind:0 → picture_url null"
    );
    assert!(entry.get("avatar_initials").is_none());
    assert!(entry.get("avatar_color").is_none());
}

#[test]
fn mention_profiles_projection_empty_when_no_visible_items_or_views() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    let mp = &snap["projections"]["mention_profiles"];
    assert!(mp.is_object(), "mention_profiles must always be present");
    assert_eq!(
        mp.as_object().map(|m| m.len()),
        Some(0),
        "mention_profiles must be empty when no events are visible and no view is open"
    );
}

/// `claim_profile` is the registry-component lifecycle path. A component that
/// only knows a pubkey must see a stable projection slot immediately, then the
/// real profile fields after kind:0 arrives, without opening an author view or
/// building a screen-local profile map.
#[test]
fn claimed_profiles_projection_refines_claimed_pubkey() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let _ = kernel.claim_profile(ACCOUNT.to_string(), "avatar".to_string(), false);

    let before = snapshot(&mut kernel);
    let entry = &before["projections"]["claimed_profiles"][ACCOUNT];
    assert!(
        !entry.is_null(),
        "claimed_profiles must carry a placeholder for every claimed pubkey"
    );
    assert_eq!(entry["pubkey"].as_str(), Some(ACCOUNT));
    let expected_npub = crate::display::to_npub(ACCOUNT);
    assert_eq!(entry["npub"].as_str(), Some(expected_npub.as_str()));
    assert_eq!(entry["has_profile"].as_bool(), Some(false));
    assert!(entry["display_name"].is_null());
    assert!(entry["picture_url"].is_null());

    let event = nostr::NostrEvent {
        id: "0000000000000000000000000000000000000000000000000000000000000021".to_string(),
        pubkey: ACCOUNT.to_string(),
        created_at: 1_700_000_100,
        kind: 0,
        tags: vec![],
        content: r#"{"display_name":"Claimed Profile","picture":"https://example.com/claimed.png","nip05":"claimed@example.com","about":"profile from claim"}"#.to_string(),
        sig: String::new(),
    };
    kernel.ingest_profile(event);

    let after = snapshot(&mut kernel);
    let entry = &after["projections"]["claimed_profiles"][ACCOUNT];
    assert_eq!(entry["has_profile"].as_bool(), Some(true));
    assert_eq!(entry["display_name"].as_str(), Some("Claimed Profile"));
    assert_eq!(
        entry["picture_url"].as_str(),
        Some("https://example.com/claimed.png")
    );
    assert_eq!(entry["nip05"].as_str(), Some("claimed@example.com"));

    let _ = kernel.release_profile(ACCOUNT, "avatar");
    let released = snapshot(&mut kernel);
    assert!(
        released["projections"]["claimed_profiles"][ACCOUNT].is_null(),
        "released profile claims must leave the claimed_profiles projection"
    );
}

/// V-31 — `mention_profiles` MUST carry an entry for every author in the
/// home `timeline` even when no author-view / thread-view is open, so
/// HomeFeedView resolves authors via `model.mentionProfiles` rather than
/// reconstructing the dict in Swift (replaces `HomeFeedView.swift:187-197`).
#[test]
fn mention_profiles_projection_covers_home_timeline_when_no_view_open() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());
    ingest_note(
        &mut kernel,
        NOTE_ID,
        ACCOUNT,
        1_700_000_000,
        "hello home feed",
    );

    let snap = snapshot(&mut kernel);
    let mp = &snap["projections"]["mention_profiles"];
    assert!(mp.is_object(), "mention_profiles must be a JSON object");
    let entry = &mp[ACCOUNT];
    assert!(
        !entry.is_null(),
        "mention_profiles must cover the home-timeline author with no author/thread view open"
    );
    assert_eq!(entry["pubkey"].as_str(), Some(ACCOUNT));
    assert!(
        entry["display_name"].is_null(),
        "no kind:0 → display_name null"
    );
    assert!(
        entry["picture_url"].is_null(),
        "no kind:0 → picture_url null"
    );
}

/// A kind:0 author's note in the timeline must show the kind:0 display/avatar
/// in its `projections.timeline[]` row — proving the profile join happens at
/// projection time, not just on the standalone `profile` card. Order of ingest
/// must not matter: here the note is ingested BEFORE the kind:0 (the realistic
/// relay race).
#[test]
fn timeline_item_picks_up_profile_after_later_kind0_ingest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Note arrives first, with no profile yet → author_picture_url is null.
    ingest_note(&mut kernel, NOTE_ID, ACCOUNT, 1_700_000_000, "a note");
    let before = snapshot(&mut kernel);
    assert!(
        before["projections"]["timeline"][0]["author_picture_url"].is_null(),
        "before kind:0 the timeline item author_picture_url must be null (aim.md §2)",
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
    // The TimelineItem no longer carries `author_display` directly — the
    // kind:0 display name surfaces via `mention_profiles` (the per-author
    // join map).
    assert_eq!(
        after["projections"]["mention_profiles"][ACCOUNT]["display_name"].as_str(),
        Some("Late Profile"),
        "the mention_profiles join must pick up the kind:0 display name in-place",
    );
    assert_eq!(
        after["projections"]["timeline"][0]["author_picture_url"].as_str(),
        Some("https://example.com/p.png"),
        "the timeline item author_picture_url must refine to the kind:0 picture after kind:0",
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
    // Declare the host kinds {1, 6} the contact-list-authors subscription REQs
    // for (D0: the substrate no longer hardcodes a kind set).
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
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

// ─── NIP-47 wallet status ───────────────────────────────────────────────────
//
// D0: NIP-47 NWC is an app noun — wallet state is NO LONGER a typed
// `KernelSnapshot` field. It is surfaced through the `"wallet"` host-registered
// snapshot projection. The connect / disconnect lifecycle proof lives with the
// other snapshot-projection tests in `snapshot_registry_tests.rs`
// (`wallet_projection_appears_and_clears_through_make_update`), since it now
// exercises the projection seam rather than a kernel-owned field.
