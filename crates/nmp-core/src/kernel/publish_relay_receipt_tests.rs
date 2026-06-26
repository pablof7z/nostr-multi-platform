//! Structured publish relay receipts surfaced through `action_results`.
//!
//! Kept out of `publish_terminal_status_tests.rs` because that module is
//! already over the legacy hard-cap baseline; these tests cover the typed
//! result payload while the original module keeps terminal status coverage.

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const WRITE_R1: &str = "wss://receipt-write-r1.test";
const WRITE_R2: &str = "wss://receipt-write-r2.test";

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

fn single_action_result(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    let results = parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .and_then(|v| v.as_array())
        .expect("action_results must be a JSON array when an action settled");
    assert_eq!(results.len(), 1, "exactly one terminal settled this tick");
    results[0].clone()
}

#[test]
fn action_results_success_carries_publish_relay_receipt() {
    let author = "a8".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b8".repeat(32).as_str(), &author, 1, "receipt ok");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    let result = single_action_result(&mut kernel);
    let receipt = result
        .get("result")
        .expect("a published result carries a structured relay receipt");
    assert_eq!(
        receipt.get("kind").and_then(|v| v.as_str()),
        Some("publish_relay_receipt")
    );
    assert_eq!(
        receipt.get("event_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str())
    );
    let relays = receipt
        .get("relays")
        .and_then(|v| v.as_array())
        .expect("relay receipt carries per-relay rows");
    assert_eq!(relays.len(), 2, "both relay verdicts are preserved");
    assert!(
        relays.iter().all(|row| {
            row.get("status").and_then(|v| v.as_str()) == Some("ok")
                && row.get("relay_url").and_then(|v| v.as_str()).is_some()
        }),
        "each relay receipt row carries an ok verdict and raw relay URL: {relays:?}"
    );
}

#[test]
fn action_results_failure_carries_publish_relay_receipt() {
    let author = "a9".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b9".repeat(32).as_str(), &author, 1, "receipt fail");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

    let drive_to_giveup = |kernel: &mut Kernel, relay: &str, base_ms: u64| {
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 1"),
            base_ms + 100,
        );
        let _ = kernel.tick_publish_engine(base_ms + 1_500);
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 2"),
            base_ms + 1_600,
        );
        let _ = kernel.tick_publish_engine(base_ms + 6_000);
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 3"),
            base_ms + 6_100,
        );
    };
    drive_to_giveup(&mut kernel, WRITE_R1, 0);
    drive_to_giveup(&mut kernel, WRITE_R2, 100_000);

    let result = single_action_result(&mut kernel);
    let relays = result
        .get("result")
        .and_then(|v| v.get("relays"))
        .and_then(|v| v.as_array())
        .expect("failed publish result carries per-relay failure receipts");
    assert_eq!(relays.len(), 2, "both failed relay verdicts are preserved");
    assert!(
        relays.iter().all(|row| {
            row.get("status").and_then(|v| v.as_str()) == Some("failed")
                && row
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|message| message.contains("transient"))
                    .unwrap_or(false)
        }),
        "each relay receipt row carries the failure reason: {relays:?}"
    );
}
