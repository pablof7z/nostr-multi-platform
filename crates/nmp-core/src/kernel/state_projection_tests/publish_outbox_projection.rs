//! `projections.publish_outbox` and `projections.outbox_summary`: pending
//! event details, per-relay rationale, `skip_serializing_if` empty-reason
//! omission, and the aggregate status counters. ADR-0032 / aim.md §2 #4: no
//! pre-formatted title/preview/status_label/target_summary strings — shells
//! compute display strings from the raw fields.

use std::sync::Arc;

use super::support::{snapshot, ACCOUNT};
use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PerRelayState, PublishRecord, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

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
        crate::publish::PublishTarget::explicit(vec!["wss://outbox.test".to_string()], crate::publish::PublishRouteClass::ManualOverride),
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
    // which is formatted as the raw token `"explicit:manual_override"`.
    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::explicit(vec!["wss://reason.test".to_string()], crate::publish::PublishRouteClass::ManualOverride),
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
        Some("explicit:manual_override"),
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
        crate::publish::PublishTarget::explicit(vec!["wss://outbox.test".to_string()], crate::publish::PublishRouteClass::ManualOverride),
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
