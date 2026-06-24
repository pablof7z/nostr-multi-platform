use super::*;

// ── Inbound ingest seam (IngestParser — raw-tap PR-2) ────────────────────

/// Simulate the kernel `IngestParser` delivering a signed kind:1059
/// gift-wrap welcome: it must reach `MarmotService` via
/// `ingest_signed_event_core`, and Bob's snapshot must then show a pending
/// welcome — with NO Swift / dispatch call (the existing snapshot read surfaces
/// the new state). Builds a real gift-wrap via the two-party in-memory pattern
/// (the `nmp_nip59` path), exactly as `crates/nmp-marmot/src/tests.rs` does.
#[test]
fn ingest_parser_kind_1059_welcome_reaches_service_and_snapshot() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory(alice_keys.clone());
    let bob_service = in_memory(bob_keys.clone());

    // Bob publishes a KeyPackage so Alice can invite him.
    let bob_kp = bob_service
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp");

    // Alice creates the group inviting Bob, then gift-wraps the kind:444
    // welcome rumor to Bob (real NIP-59 path → signed kind:1059).
    let config = NostrGroupConfigData::new(
        "Parser Ingest Test".to_string(),
        "inbound".to_string(),
        None,
        None,
        None,
        vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()],
        vec![alice_keys.public_key()],
    );
    let (_group, pending) = alice
        .create_group(vec![bob_kp.event_30443.clone()], config)
        .expect("alice creates group");
    let welcome_rumor = pending.welcome_rumors[0].clone();
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), welcome_rumor)
        .expect("alice gift-wraps welcome to bob");
    pending.commit().expect("alice merges create commit");
    let gift_json = gift.as_json();
    let gift_id_hex = gift.id.to_hex();

    // Bob's projection + the IngestParser the FFI register path would install.
    let bob_proj = Arc::new(MarmotProjection::new(bob_service, None));
    let parser = MarmotIngestParser::new(Arc::clone(&bob_proj));

    // Pre-condition: no pending welcomes yet.
    assert!(bob_proj.snapshot(0).pending_welcomes.is_empty());

    // Kernel dispatcher delivers the VerifiedEvent to the parser.
    let verified = gift_wrap_to_verified(&gift_json);
    parser.parse(&verified);

    // The snapshot read (unchanged, no Swift call) now surfaces it.
    let snap = bob_proj.snapshot(1);
    assert_eq!(
        snap.pending_welcomes.len(),
        1,
        "parser-delivered welcome must surface in snapshot: {snap:?}"
    );
    let row = &snap.pending_welcomes[0];
    assert_eq!(row.id_hex, gift_id_hex);
    assert_eq!(row.group_name, "Parser Ingest Test");
    assert_eq!(row.inviter_npub, alice_keys.public_key().to_hex());

    // Idempotent / D6: a duplicate relay echo of the same gift-wrap is a
    // silent no-op on the parser (never panics, snapshot stays consistent).
    parser.parse(&verified);
    assert_eq!(bob_proj.snapshot(2).pending_welcomes.len(), 1);

    // A direct core call against the SAME projection (its key store has Bob's
    // key package; a separate service would not — KP state is per-storage)
    // is idempotent, so re-ingesting succeeds and the row is still present.
    let r = bob_proj
        .with_inner(|h| ingest_signed_event_core(h, &gift, 3))
        .unwrap();
    let r = r.expect("direct core re-ingest should succeed");
    assert_eq!(r.as_ref().and_then(|v| v.get("kind")), Some(&json!(1059)));
    assert_eq!(bob_proj.snapshot(3).pending_welcomes.len(), 1);
}

/// D6: the parser silently no-ops on a malformed / non-reconstructable event.
/// A `VerifiedEvent` already passed Schnorr verification so the JSON serialization
/// and nostr::Event parse always succeed in practice; D6 degrades on kind:444
/// (admitted by filter, deliberately skipped by the core).
#[test]
fn ingest_parser_unsupported_kind_is_silent() {
    let proj = Arc::new(MarmotProjection::new(in_memory(Keys::generate()), None));
    let parser = MarmotIngestParser::new(Arc::clone(&proj));

    // kind:444 is in TAP_KINDS (admitted by the per-kind registrations) but
    // is a deliberate skip in `ingest_signed_event_core` — the parser must
    // produce no snapshot side-effects and never panic.
    let kind444_json = nostr::EventBuilder::new(nostr::Kind::Custom(444), "x")
        .sign_with_keys(&Keys::generate())
        .unwrap()
        .as_json();
    let raw: RawEvent =
        serde_json::from_str(&kind444_json).expect("kind:444 event must deserialize to RawEvent");
    let verified = VerifiedEvent::try_from_raw(raw).expect("kind:444 must pass verification");
    parser.parse(&verified);

    let snap = proj.snapshot(0);
    assert!(snap.pending_welcomes.is_empty());
    assert!(snap.groups.is_empty());
}

/// PR-2 coexistence test: both the NIP-17 DM inbox parser (slot
/// `"nip17.dm_inbox"`) and the Marmot parser (slot `"marmot"`) are
/// registered for kind:1059 and both fire when a gift-wrap event arrives.
///
/// Uses `EventIngestDispatcher` directly (no kernel actor needed) to prove
/// the slot-keyed coexistence. A separate end-to-end test confirms Marmot
/// state mutation through the full ingest path.
#[test]
fn ingest_parser_kind_1059_coexistence_both_parsers_fire() {
    use nmp_core::substrate::EventIngestDispatcher;

    let mut dispatcher = EventIngestDispatcher::new();

    // Slot "nip17.dm_inbox" — a capturing parser to stand in for
    // DmInboxProjection (avoids the circular dep on nmp-nip17).
    struct CapturingParser {
        fired: Mutex<bool>,
    }
    impl IngestParser for CapturingParser {
        fn parse(&self, _evt: &VerifiedEvent) {
            *self.fired.lock().unwrap() = true;
        }
    }
    let dm_parser = Arc::new(CapturingParser {
        fired: Mutex::new(false),
    });
    let marmot_parser = Arc::new(CapturingParser {
        fired: Mutex::new(false),
    });

    dispatcher.replace_kind_parser(1059, "nip17.dm_inbox", dm_parser.clone());
    dispatcher.replace_kind_parser(1059, MARMOT_INGEST_SLOT, marmot_parser.clone());
    assert_eq!(dispatcher.registration_count(), 2, "both slots registered");

    // Build a real signed kind:1059 event.
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let (gift_json, _) = {
        use nmp_nip59::gift_wrap_local;
        use nostr::{EventBuilder, Kind, Tag, Timestamp};
        let rumor = EventBuilder::new(Kind::from_u16(14), "coexistence test")
            .tags(vec![Tag::public_key(receiver.public_key())])
            .custom_created_at(Timestamp::from(1_700_000_000u64))
            .build(sender.public_key());
        let envelope = gift_wrap_local(
            &sender,
            &receiver.public_key(),
            &rumor,
            Timestamp::from(1_700_000_000u64),
        )
        .expect("gift_wrap_local succeeds with local keys");
        let tag_vecs: Vec<Vec<String>> = envelope
            .tags
            .iter()
            .map(|t: &nostr::Tag| t.as_slice().to_vec())
            .collect();
        let json = serde_json::json!({
            "id": envelope.id.to_hex(),
            "pubkey": envelope.pubkey.to_hex(),
            "created_at": envelope.created_at.as_secs(),
            "kind": envelope.kind.as_u16(),
            "tags": tag_vecs,
            "content": envelope.content.clone(),
            "sig": envelope.sig.to_string(),
        })
        .to_string();
        let id = envelope.id.to_hex();
        (json, id)
    };

    let verified = gift_wrap_to_verified(&gift_json);
    dispatcher.dispatch(&verified);

    assert!(
        *dm_parser.fired.lock().unwrap(),
        "NIP-17 DM inbox parser must fire for kind:1059 (slot 'nip17.dm_inbox')"
    );
    assert!(
        *marmot_parser.fired.lock().unwrap(),
        "Marmot parser must fire for kind:1059 (slot 'marmot')"
    );
}
