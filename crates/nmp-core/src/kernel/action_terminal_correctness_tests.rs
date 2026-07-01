//! #1676 BUG-A regression — exactly one `action_results` row per
//! correlation_id, across every terminal-recording path.
//!
//! New file (not appended to `publish_terminal_status_tests.rs`) because that
//! file already sits at the file-size baseline; the gate rejects growing an
//! over-cap file.
//!
//! The invariant the issue states is "one terminal verdict per dispatch". This
//! test drives all three terminal sources into ONE kernel under three distinct
//! correlation_ids, drains `action_results` once, and asserts three rows with
//! three distinct ids — no duplicate / spurious second terminal:
//!
//!   1. engine-ack          — a real publish that settles after relay OK acks.
//!   2. FFI-failure fan-in   — `record_action_failure` (a sign-step failure
//!      the FFI layer fans in for a dispatch that enqueued nothing).
//!   3. NWC off-band success — `record_action_success` (the kind:23195 wallet
//!      response settling a `pay_invoice` off-band from the publish engine).

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const WRITE_R1: &str = "wss://i1676-write-r1.test";
const WRITE_R2: &str = "wss://i1676-write-r2.test";

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

fn ok_payload<'a>(event_id: &'a str, accepted: bool, reason: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message: reason,
    }
}

/// Seed a kind:10002 so `Nip65OutboxResolver` routes the publish to
/// `write_urls` (without it the resolver returns NoTargets).
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
    kernel.seed_mailbox_relay_list(
        author_pubkey,
        Vec::new(),
        write_urls.iter().map(|url| (*url).to_string()).collect(),
        Vec::new(),
    );
}

/// Read + drain `projections.action_results` from a fresh wire snapshot.
fn action_results(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// S11 slice 2 (#1758): the dispatched `NoTargets` path emits TWO
/// `action_results` rows under the same correlation_id — the engine's
/// `emit_no_targets` verdict (carrying `event_id`) FOLLOWED BY the engine-error
/// broken-promise verdict from `record_action_failure`. This pre-existing
/// two-row shape and its PRODUCER ORDER must be byte-identical after the source
/// inversion: the engine terminal is folded into the ledger at its production
/// boundary (in the `run_publish_engine_at` Err arm, before the off-band
/// record), so the off-band row never jumps ahead of it.
#[test]
fn dispatched_notargets_emits_engine_then_offband_row_in_order() {
    let author = "f9".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No kind:10002 seeded → resolver returns NoTargets.
    let signed = fake_signed(&"e9".repeat(32), &author, 1, "notargets");
    let cid = "corr-notargets".to_string();
    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(cid.clone()),
        0,
    );
    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .cloned()
        .expect("dispatched NoTargets surfaces action_results rows");
    let rows: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|r| r.get("correlation_id").and_then(|v| v.as_str()) == Some(cid.as_str()))
        .collect();
    assert_eq!(rows.len(), 2, "two rows for dispatched NoTargets (engine + off-band): {arr:#?}");
    // Row 0 is the engine `emit_no_targets` verdict — it carries the signed
    // event id; row 1 is the off-band engine-error broken-promise verdict.
    assert_eq!(
        rows[0].get("event_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str()),
        "the engine emit_no_targets row (with event_id) must come FIRST"
    );
    assert_eq!(
        rows[0].get("error").and_then(|v| v.as_str()),
        Some("no relays resolved for publish target"),
    );
    assert!(
        rows[1].get("event_id").is_none(),
        "the off-band engine-error row (no event_id) must come SECOND"
    );
}

#[test]
fn exactly_one_action_result_row_per_correlation_id_across_terminal_paths() {
    let author = "c7".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);

    // (1) engine-ack: a real publish that settles after both relays ack OK.
    let signed = fake_signed(&"e7".repeat(32), &author, 1, "engine ack");
    let engine_cid = signed.id.clone();
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    // (2) FFI-failure fan-in: a sign-step failure recorded directly (the same
    //     entry point the FFI dispatch layer fans in via RecordActionFailure
    //     when an executor failed having enqueued nothing).
    let failure_cid = "fa11".repeat(8); // 32 hex chars
    kernel.record_action_failure(failure_cid.clone(), "sign step failed".to_string());

    // (3) NWC off-band success: the kind:23195 response settling a pay_invoice.
    let offband_cid = "0ffb".repeat(8); // 32 hex chars
    kernel.record_action_success(offband_cid.clone(), None);

    // One drain surfaces all three terminals at once.
    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array when terminals settled");

    let cids: Vec<&str> = arr
        .iter()
        .filter_map(|row| row.get("correlation_id").and_then(|v| v.as_str()))
        .collect();
    let unique: std::collections::BTreeSet<&str> = cids.iter().copied().collect();

    assert_eq!(
        arr.len(),
        3,
        "exactly three terminals — one per path, no double terminal; got {arr:#?}"
    );
    assert_eq!(
        unique.len(),
        arr.len(),
        "no correlation_id may appear twice (one terminal per dispatch): {cids:?}"
    );
    for cid in [engine_cid.as_str(), failure_cid.as_str(), offband_cid.as_str()] {
        assert!(
            unique.contains(cid),
            "missing the single terminal for correlation_id {cid}: {cids:?}"
        );
    }

    // Drain semantics: a second read carries no `action_results` key — each
    // terminal is surfaced exactly once.
    assert!(
        action_results(&mut kernel).is_null(),
        "action_results is drained per tick — a second read is absent"
    );
}
