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
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
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

// V-112 (ADR-0042): d5_view_dependent_keys_absent_when_no_view_open deleted —
// author_view / thread_view projection bounding is removed with those projections.
// The open_author / open_thread methods and AuthorViewState / ThreadViewState are
// deleted from the kernel; per-app FlatFeed owns the view lifecycle.

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
        content: r#"{"name":"sat","display_name":"Satoshi","displayName":"Satoshi Camel","nip05":"sat@example.com","about":"hi there","picture":"https://example.com/sat.png","banner":"https://example.com/banner.png","website":"https://satoshi.example","lud16":"sat@ln.example","lud06":"lnurl1sat"}"#
            .to_string(),
        sig: String::new(),
    };
    kernel.inject_profile(event);

    let after = snapshot(&mut kernel);
    let card = &after["projections"]["profile"];
    assert_eq!(
        card["display_name"].as_str(),
        Some("Satoshi"),
        "kind:0 display_name must be projected into profile.display_name",
    );
    assert_eq!(card["name"].as_str(), Some("sat"));
    assert_eq!(card["raw_display_name"].as_str(), Some("Satoshi"));
    assert_eq!(card["display_name_camel"].as_str(), Some("Satoshi Camel"));
    assert_eq!(
        card["picture_url"].as_str(),
        Some("https://example.com/sat.png"),
        "kind:0 picture must be projected into profile.picture_url",
    );
    assert_eq!(
        card["banner"].as_str(),
        Some("https://example.com/banner.png")
    );
    assert_eq!(card["website"].as_str(), Some("https://satoshi.example"));
    assert_eq!(
        card["nip05"].as_str(),
        Some("sat@example.com"),
        "kind:0 nip05 must be projected into profile.nip05",
    );
    assert_eq!(card["lud16"].as_str(), Some("sat@ln.example"));
    assert_eq!(card["lud06"].as_str(), Some("lnurl1sat"));
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

// `profile_card_projects_pending_kind0_publish_intent_after_restart` was
// deleted with the `local_profile_intents` overlay (#1193, ADR-0045 Rev 2
// single-mechanism). The overlay used to rehydrate an unsent pending kind:0
// from the publish store on kernel reconstruction; the retired architecture
// deliberately drops that publish-store-rehydration path. Read-your-writes for
// a locally-published kind:0 is now served immediately at publish time by
// `verify_and_persist` + `ingest_profile` into the canonical event store /
// `profiles` cache (covered by `local_kind0_publish_fans_out_to_event_observers`
// in `local_publish_intent_tests.rs`), not by a separate restart-restore overlay.

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
    // ADR-0032 / aim.md §2 #4: `title`, `preview`, `system_image`, `status_label`
    // removed from the projection — shells own all presentation formatting.
    assert!(
        outbox[0].get("title").is_none(),
        "title must be absent from projection (aim.md §2 #4)"
    );
    assert!(
        outbox[0].get("preview").is_none(),
        "preview must be absent from projection (aim.md §2 #4)"
    );
    assert!(
        outbox[0].get("system_image").is_none(),
        "system_image must be absent from projection (aim.md §2 #4)"
    );
    assert!(
        outbox[0].get("status_label").is_none(),
        "status_label must be absent from projection (aim.md §2 #4)"
    );
    // Raw content is emitted so shells can render their own kind-appropriate preview.
    assert_eq!(
        outbox[0]["content"].as_str(),
        Some("This note is still waiting for relays")
    );
    assert_eq!(outbox[0]["status"].as_str(), Some("sending"));
    // RMP bible commandment #4: a row currently sending cannot be retried.
    // The kernel emits the decision; the shell binds it directly (no Swift
    // `if status != "sending"` branch).
    assert_eq!(outbox[0]["can_retry"].as_bool(), Some(false));
    // V-115 / ADR-0032: `target_summary` removed — shell composes
    // "N relays · <time>" from `target_relays` + `created_at` (raw Unix secs).
    assert!(
        outbox[0].get("target_summary").is_none(),
        "target_summary must be absent from projection (V-115)"
    );
    assert_eq!(
        outbox[0]["target_relays"].as_u64(),
        Some(1),
        "target_relays carries the raw count the shell uses to compose the summary"
    );
    // Raw Unix-seconds timestamp — shell formats with its own locale/TZ.
    assert!(
        outbox[0]["created_at"].as_u64().is_some(),
        "created_at must carry raw Unix seconds (V-115 / ADR-0032)"
    );
    assert!(
        outbox[0].get("created_at_display").is_none(),
        "created_at_display must be absent from projection (V-115)"
    );
    assert_eq!(
        outbox[0]["relays"][0]["relay_url"].as_str(),
        Some("wss://outbox.test")
    );
    // ADR-0032 / aim.md §2 #4: `status_label` and `attempt_label` removed —
    // shells compute these from the raw `status` token and `attempt` counter.
    assert!(
        outbox[0]["relays"][0].get("status_label").is_none(),
        "relay status_label must be absent (aim.md §2 #4)"
    );
    assert!(
        outbox[0]["relays"][0].get("attempt_label").is_none(),
        "relay attempt_label must be absent (aim.md §2 #4)"
    );
    assert_eq!(outbox[0]["relays"][0]["status"].as_str(), Some("sending"));
    assert_eq!(outbox[0]["relays"][0]["attempt"].as_u64(), Some(1));
}

/// Per-relay rationale ("why was this relay targeted?") threads from the
/// outbox resolver all the way through to the JSON projection that crosses
/// the C-ABI. Apps parse `relay_reason` as a machine token and format it
/// locally. This test pins the raw token so a regression that drops the
/// value (or stops serializing it) is caught at the projection boundary.
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
    // `TestKind10002OutboxResolver`) emits `RelaySelectionReason::Explicit`
    // which is formatted as the raw token `"explicit"`.
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
        Some("explicit"),
        "kernel projection must surface the raw reason token (shells format it)",
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

/// `outbox_summary` projects raw per-status counters when nothing is pending.
/// ADR-0032 / aim.md §2 #4: `title` / `subtitle` pre-formatted strings are
/// NOT emitted from the kernel — shells compute display strings from counters.
#[test]
fn outbox_summary_projects_empty_state_strings() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let snap = snapshot(&mut kernel);
    let summary = &snap["projections"]["outbox_summary"];
    // aim.md §2 #4: title and subtitle must NOT appear in the kernel projection.
    assert!(
        summary.get("title").is_none(),
        "title must be absent from outbox_summary projection (aim.md §2 #4)"
    );
    assert!(
        summary.get("subtitle").is_none(),
        "subtitle must be absent from outbox_summary projection (aim.md §2 #4)"
    );
    assert_eq!(summary["total"].as_u64(), Some(0));
    assert_eq!(summary["sending"].as_u64(), Some(0));
    assert_eq!(summary["retrying"].as_u64(), Some(0));
    assert_eq!(summary["queued"].as_u64(), Some(0));
    assert_eq!(summary["failed"].as_u64(), Some(0));
}

/// `outbox_summary` projects raw per-status counters when rows are in flight.
/// ADR-0032 / aim.md §2 #4: `title` / `subtitle` pre-formatted strings are
/// NOT emitted from the kernel — shells compute display strings from counters.
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
    // aim.md §2 #4: title and subtitle must NOT appear in the kernel projection.
    assert!(
        summary.get("title").is_none(),
        "title must be absent from outbox_summary projection (aim.md §2 #4)"
    );
    assert!(
        summary.get("subtitle").is_none(),
        "subtitle must be absent from outbox_summary projection (aim.md §2 #4)"
    );
    assert_eq!(summary["total"].as_u64(), Some(1));
    assert_eq!(summary["sending"].as_u64(), Some(1));
    assert_eq!(summary["retrying"].as_u64(), Some(0));
}

// V-112 (ADR-0042): author_view_projects_edit_action_for_active_profile,
// author_view_projects_follow_action_for_non_active_profile,
// author_view_projects_unfollow_when_active_contacts_include_author,
// profile_action_follow_carries_nmp_follow_dispatch_spec,
// profile_action_unfollow_carries_nmp_unfollow_dispatch_spec,
// profile_action_edit_profile_has_no_dispatch_spec,
// author_view_carries_note_count_display_string — all deleted.
// author_view projection and profile_action_for() removed from kernel.

/// V-115 / ADR-0032: projection sends raw hex pubkey only; shells encode
/// bech32 and any abbreviation host-side. `npub` must be ABSENT from the
/// JSON projection.
#[test]
fn profile_card_carries_raw_pubkey_without_npub() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    let profile = &snap["projections"]["profile"];
    assert_eq!(
        profile["pubkey"].as_str(),
        Some(ACCOUNT),
        "profile.pubkey must carry the raw hex (aim.md §2)"
    );
    // ADR-0032 / V-115: `npub` bech32 field removed from projection.
    assert!(
        profile.get("npub").is_none(),
        "profile.npub must be absent — shells encode bech32 themselves"
    );
    assert!(
        profile.get("npub_short").is_none(),
        "npub_short field was removed by aim.md §2 — shells own abbreviation"
    );
}

// ADR-0063 Lane H: mention_profiles_projection_empty_when_no_visible_items_or_views
// deleted — mention_profiles projection removed entirely (replaced by refs.profile).
// ADR-0063 Lane H: claimed_profiles_projection_refines_claimed_pubkey deleted —
// claimed_profiles projection removed entirely (replaced by refs.profile KPRF
// row-delta sidecar). Profile refinement tests live in the refs integration suite.

// ─── kind:3 contacts → metrics projection ────────────────────────────────────

/// Active kind:3 ingest surfaces follow counts in snapshot metrics.
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
    kernel.inject_contacts(event);

    let after = snapshot(&mut kernel);
    assert_eq!(
        after["metrics"]["contacts_authors"].as_u64(),
        Some(2),
        "metrics.contacts_authors must project the two kind:3 follows",
    );
    // Core contact ingest no longer owns feed author expansion; reduced feed
    // sources compile that dynamic author set above the generic interest seam.
    assert_eq!(
        after["metrics"]["timeline_authors"].as_u64(),
        Some(0),
        "active-account kind:3 must not mutate metrics.timeline_authors directly",
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
