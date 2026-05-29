//! Regression tests for publish relay URL identity normalization.

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{SignedEvent, UnsignedEvent};

const RAW_NIP65_RELAY: &str = "wss://Relay.Ex/";
const CANONICAL_RELAY: &str = "wss://relay.ex";

fn fake_signed(id: &str, author: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "relay identity regression".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_url: &str) {
    let raw = RawEvent {
        // Use the author pubkey as the event id — guaranteed valid hex (64
        // hex chars).  The old string "relayidentity" is not valid hex;
        // V-70 strengthened `is_structurally_valid()` to check hex chars,
        // so that synthetic event was rejected as Malformed.
        id: author_pubkey.to_string(),
        pubkey: author_pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags: vec![vec![
            "r".to_string(),
            write_url.to_string(),
            "write".to_string(),
        ]],
        content: String::new(),
        sig: "0".repeat(128),
    };
    let source = "wss://seed".to_string();
    kernel
        .store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &source,
            1_700_000_000_000,
        )
        .expect("seed kind:10002");
}

fn ok_payload<'a>(event_id: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: true,
        message: "",
    }
}

#[test]
fn publish_ok_settles_mixed_case_nip65_relay_after_transport_canonicalizes() {
    let author = "12".repeat(32);
    let event_id = "34".repeat(32);
    let signed = fake_signed(&event_id, &author);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, RAW_NIP65_RELAY);

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(outbound.len(), 1);
    assert_eq!(
        outbound[0].relay_url, CANONICAL_RELAY,
        "transport send URL must use the canonical publish identity"
    );

    let in_flight = kernel.publish_status_snapshot().in_flight.clone();
    assert_eq!(in_flight.len(), 1);
    assert_eq!(
        in_flight[0].per_relay[0].0, CANONICAL_RELAY,
        "publish tracking must key per_relay by the canonical relay identity"
    );

    let retry = kernel.handle_publish_ok_at(CANONICAL_RELAY, ok_payload(&signed.id), 1_010);
    assert!(retry.is_empty(), "a clean OK should not schedule a retry");

    let snap = kernel.publish_status_snapshot();
    assert!(
        snap.in_flight.is_empty(),
        "canonical OK ack must settle and evict the publish row"
    );
    assert_eq!(snap.recent_ok.len(), 1);
    assert_eq!(
        snap.recent_ok[0].accepted_by,
        vec![CANONICAL_RELAY.to_string()]
    );
    assert!(snap.recent_errors.is_empty());
}
