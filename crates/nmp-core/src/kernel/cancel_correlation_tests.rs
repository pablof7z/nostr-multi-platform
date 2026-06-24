//! Cancel-by-`correlation_id` acceptance tests (S7, #1754 — PD-036).
//!
//! These drive the real publish-engine path (`run_publish_engine_at` →
//! `cancel_publish`) and assert that a user-initiated cancel records the
//! DISTINCT `cancelled` terminal under the ORIGINAL dispatch `correlation_id`,
//! never the publish handle/event id. Split into its own file (rather than
//! grown onto `publish_terminal_status_tests.rs`) to keep both modules under the
//! file-size baseline (AGENTS.md §file-size).

use std::sync::Arc;

use crate::kernel::Kernel;
use crate::publish::{PublishHandle, PublishRecord, PublishStore, PublishStoreError};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const WRITE_R1: &str = "wss://s7-cancel-r1.test";
const WRITE_R2: &str = "wss://s7-cancel-r2.test";

fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    let tags: Vec<Vec<String>> = write_urls
        .iter()
        .map(|url| vec!["r".to_string(), url.to_string(), "write".to_string()])
        .collect();
    let raw = RawEvent {
        id: author_pubkey.to_string(),
        pubkey: author_pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel
        .store
        .insert(verified, &"wss://seed".to_string(), 1_700_000_000_000)
        .expect("seed_kind10002 insert");
}

/// Drain `action_results` from a fresh wire snapshot and return the single
/// terminal that settled this tick.
fn single_action_result(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    let results = parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .expect("action_results present when a terminal settled");
    let arr = results.as_array().expect("action_results is an array");
    assert_eq!(arr.len(), 1, "exactly one terminal settled this tick");
    arr[0].clone()
}

#[test]
fn cancel_by_correlation_id_records_terminal_under_original_id_not_handle() {
    // ACCEPTANCE — PD-036. A publish is dispatched under a DISTINCT
    // `correlation_id` (the registry-minted id the host's spinner is keyed on),
    // separate from the publish handle (== event id). Cancelling by that
    // `correlation_id` must record the `Cancelled` terminal under the ORIGINAL
    // correlation_id — NOT the handle/event id (the prior defect).
    let author = "c7".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("c8".repeat(32).as_str(), &author, 1, "cancel by corr id");
    let correlation_id = "op-correlation-7777".to_string();
    assert_ne!(
        correlation_id, signed.id,
        "the fixture must use a correlation_id distinct from the event id/handle"
    );

    // Dispatch path: the engine carries the override so the in-flight row knows
    // the original correlation_id, and the durable handle↔correlation index is
    // populated at the single engine-entry site.
    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        0,
    );
    assert_eq!(kernel.publish_status_snapshot().in_flight.len(), 1);

    // Cancel by the ORIGINAL correlation_id (the only id the host knows).
    kernel.cancel_publish(&correlation_id);

    // `action_results` terminal lands under the ORIGINAL correlation_id, never
    // the handle.
    let result = single_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("cancelled"),
        "user cancel reports status `cancelled`"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(correlation_id.as_str()),
        "the Cancelled terminal must key on the ORIGINAL correlation_id (PD-036)"
    );
    assert_ne!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str()),
        "the Cancelled terminal must NOT key on the publish handle/event id"
    );
    // The event id is still surfaced as output DATA (#1702), distinct from the
    // operation identity.
    assert_eq!(
        result.get("event_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str()),
        "the signed event id is carried as output data, not as the correlation_id"
    );

    // And the in-flight publish is gone.
    assert!(
        kernel.publish_status_snapshot().in_flight.is_empty(),
        "cancel must remove the in-flight publish row"
    );
}

#[test]
fn cancel_records_distinct_cancelled_lifecycle_stage_under_correlation_id() {
    // The `action_lifecycle` display projection carries a DISTINCT `cancelled`
    // terminal stage (user-initiated), keyed on the ORIGINAL correlation_id —
    // never `failed`, never the handle.
    let author = "c9".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("ca".repeat(32).as_str(), &author, 1, "cancel lifecycle");
    let correlation_id = "op-lifecycle-9999".to_string();
    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        0,
    );

    kernel.cancel_publish(&correlation_id);

    let snapshot_json = kernel.make_update_json_for_test(true);
    let snap: serde_json::Value = serde_json::from_str(&snapshot_json).expect("update JSON parses");
    let lifecycle = snap
        .get("projections")
        .and_then(|p| p.get("action_lifecycle"))
        .expect("action_lifecycle projection present after cancel");
    let recent = lifecycle["recent_terminal"]
        .as_array()
        .expect("recent_terminal is an array");
    let row = recent
        .iter()
        .find(|r| r["correlation_id"] == correlation_id.as_str())
        .expect("a recent_terminal row keyed on the ORIGINAL correlation_id");
    assert_eq!(
        row["stage"], "cancelled",
        "the lifecycle terminal is the DISTINCT `cancelled` stage, not `failed`"
    );
    assert!(
        !recent
            .iter()
            .any(|r| r["correlation_id"] == signed.id.as_str()),
        "no lifecycle terminal may be keyed on the publish handle/event id (PD-036)"
    );
}

#[test]
fn cancel_by_raw_handle_still_resolves_for_internal_publish() {
    // An internal publish (no distinct dispatch correlation_id) self-maps the
    // handle; cancelling by the raw handle still records the `cancelled`
    // terminal under that handle.
    let author = "cb".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("cc".repeat(32).as_str(), &author, 1, "internal cancel");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

    kernel.cancel_publish(&signed.id);

    let result = single_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("cancelled")
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str()),
        "an internal publish reports the handle as the correlation_id (self-map)"
    );
}

/// A publish store whose `delete` ALWAYS fails — used to prove the cancel
/// terminal is recorded BEFORE (and independent of) the best-effort durable
/// delete (codex review: a delete failure must never orphan the host spinner).
#[derive(Default)]
struct FailingDeleteStore;

impl PublishStore for FailingDeleteStore {
    fn upsert(&self, _record: &PublishRecord) -> Result<(), PublishStoreError> {
        Ok(())
    }
    fn delete(&self, _handle: &PublishHandle) -> Result<(), PublishStoreError> {
        Err(PublishStoreError::Backend(
            "simulated durable delete failure".to_string(),
        ))
    }
    fn load_pending(&self) -> Result<Vec<PublishRecord>, PublishStoreError> {
        Ok(Vec::new())
    }
}

#[test]
fn cancel_records_terminal_even_when_store_delete_fails() {
    // BLOCKING-FIX regression (codex): `cancel_by_handle` records the
    // `cancelled` terminal BEFORE the best-effort store delete, so a durable
    // delete failure can NEVER orphan the host spinner. With a store that always
    // fails `delete`, the cancel must STILL surface a `cancelled` terminal under
    // the ORIGINAL correlation_id.
    let store: Arc<dyn PublishStore> = Arc::new(FailingDeleteStore);
    let author = "cd".repeat(32);
    let mut kernel = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, store);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("ce".repeat(32).as_str(), &author, 1, "store-fail cancel");
    let correlation_id = "op-store-fail-1234".to_string();
    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        0,
    );
    assert_eq!(kernel.publish_status_snapshot().in_flight.len(), 1);

    kernel.cancel_publish(&correlation_id);

    let result = single_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("cancelled"),
        "a store-delete failure must NOT prevent the cancelled terminal"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(correlation_id.as_str()),
        "the terminal still lands under the ORIGINAL correlation_id despite the delete failure"
    );
    assert!(
        kernel.publish_status_snapshot().in_flight.is_empty(),
        "the in-flight row is still removed even though the durable delete failed"
    );
}

/// S10 (#1757) G2 kernel gate — `action_lifecycle.recent_terminal` carries a
/// `"cancelled"` entry under the dispatch `correlation_id` after `cancel_publish`.
/// Closes the chain from the FFI-level `send_cmd_count` probe in
/// `nmp-ffi/src/action/s10_gates_tests.rs`: that probe proves the FFI enqueues
/// the command; this proves the kernel records the terminal.
#[test]
fn s10_gate_cancel_action_lifecycle_shows_cancelled_under_dispatch_correlation_id() {
    let author = "cf".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("d0".repeat(32).as_str(), &author, 1, "s10-g2-cancel-terminal");
    let corr_id = "s10-g2-corr-7b2e".to_string();
    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(corr_id.clone()),
        0,
    );
    kernel.cancel_publish(&corr_id);
    let snap: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).unwrap();
    let recent = snap["projections"]["action_lifecycle"]["recent_terminal"]
        .as_array()
        .expect("recent_terminal present after cancel");
    let row = recent
        .iter()
        .find(|r| r["correlation_id"] == corr_id.as_str())
        .expect("S10 G2: recent_terminal must be keyed on the dispatch correlation_id");
    assert_eq!(row["stage"], "cancelled", "S10 G2: stage must be `cancelled`");
}
