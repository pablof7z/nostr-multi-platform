//! Generated-builder wire round-trip tests for the relay-list action namespaces
//! (M14-1 / #2145).
//!
//! Sibling to `dispatch_integration.rs` (which covers the fail-closed
//! `schema_version` gate + Rust typed-`encode()` positives). This file proves
//! that bytes shaped EXACTLY as the generated Swift/Kotlin action-builders emit
//! (`crates/nmp-codegen/src/action_builders`) decode back field-for-field and
//! dispatch END TO END through `ActionRegistry::start_bytes` — the production
//! byte path (S2 `DispatchEnvelope` decode → typed decode + fail-closed gate →
//! `start()`). Each test carries a wrong-namespace twin so a passing positive is
//! a real signal. Split out from `dispatch_integration.rs` to keep both files
//! under the 500 LOC ceiling (AGENTS.md).

use std::sync::Arc;

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRegistrar, ActionRejection};
use nmp_router::publish_relay_list::{RelayListEntry, RelayMarker};
use nmp_router::{
    BlockRelayAction, BlockRelayInput, InMemoryBlockedRelayCache, PublishRelayListAction,
    PublishRelayListInput, UnblockRelayAction, UnblockRelayInput,
};

const PUBKEY: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

// ---- M14-1 / #2145: generated-builder wire round-trip (RelayListEntryVec) ----
//
// The tests above feed the Rust typed `.encode()` through `start_bytes`. They do
// NOT prove that bytes shaped EXACTLY as the generated Swift/Kotlin
// `publishRelayList` builder emits (`crates/nmp-codegen/src/action_builders`)
// decode back correctly — and that emitter introduces a wire shape no other
// namespace uses: a vector of `RelayListEntry { url:string; marker:ubyte }`
// tables whose `marker` byte is computed host-side by `relayMarkerByte(role)`
// from a free-form role string. This is the authoritative guard the codegen
// emitter unit tests cannot provide (that crate has no nmp-core/nmp-router dep):
// it proves the `relayMarkerByte` ordinal mapping (Both=0, Read=1, Write=2,
// Indexer=3) the emitter bakes in matches what `marker_from_wire` decodes, and
// that a COMPOSITE role such as `"both,indexer"` resolves to `Both` (NOT
// `Indexer`) on the wire.

/// Mirror of the generated `relayMarkerByte` helper emitted into
/// ActionBuilders.{swift,kt,ts} — host shells pass a raw relay-row role string
/// and the builder folds it to the NIP-65 `RelayMarker` ordinal. Replicated
/// here so the hand-rolled buffer is byte-identical to generated output.
///
/// IMPORTANT: This must stay in exact semantic lock-step with the three
/// emitters in `crates/nmp-codegen/src/action_builders/{kotlin,swift,ts}.rs`
/// AND with `RelayMarker::from_role_string` in `crates/nmp-router/src/publish_relay_list.rs`.
/// Unknown tokens or no-flag input return 255 (out-of-range sentinel) so the
/// Rust decoder (`marker_from_wire`) fails closed rather than silently becoming Both.
fn relay_marker_byte(role: &str) -> u8 {
    let (mut has_both, mut has_read, mut has_write, mut has_indexer) = (false, false, false, false);
    let mut invalid = false;
    for part in role.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "" => {}
            "both" => has_both = true,
            "read" => has_read = true,
            "write" => has_write = true,
            "indexer" => has_indexer = true,
            _ => invalid = true,
        }
    }
    if invalid {
        return 255;
    }
    if has_both || (has_read && has_write) {
        0
    } else if has_read {
        1
    } else if has_write {
        2
    } else if has_indexer {
        3
    } else {
        255
    }
}

/// Build a raw N65P FlatBuffers payload (schema_version=1, one relay entry with
/// URL `"wss://relay.test"` and the given raw `marker_byte`) WITHOUT routing
/// through `relay_marker_byte`. Used by the anti-drift table test to probe
/// `PublishRelayListInput::decode` with a precise marker byte.
fn build_n65p_payload_with_raw_marker(marker_byte: u8) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    const N65P_IDENTIFIER: &str = "N65P";
    let mut fbb = FlatBufferBuilder::new();
    let url_off = fbb.create_string("wss://relay.test");
    let entry_start = fbb.start_table();
    fbb.push_slot_always::<WIPOffset<&str>>(4 as VOffsetT, url_off); // slot 0: url
    fbb.push_slot::<u8>(6 as VOffsetT, marker_byte, 0); // slot 1: marker
    let entry = fbb.end_table(entry_start);
    let relays_vec = fbb.create_vector(&[entry]);
    let payload_start = fbb.start_table();
    fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
    fbb.push_slot_always::<WIPOffset<_>>(6 as VOffsetT, relays_vec); // slot 1: relays
    let root = fbb.end_table(payload_start);
    fbb.finish(root, Some(N65P_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// Build a `nmp.nip65.publish_relay_list` `DispatchEnvelope` EXACTLY as the
/// generated `publishRelayList(correlationId:relays:)` builder does: encode each
/// `RelayListEntry` (url at slot 0 / vt 4, `relayMarkerByte(role)` at slot 1 /
/// vt 6), the `PublishRelayListPayload` (N65P; schema_version slot 0, relays
/// vector slot 1), then stamp it into the `NMPD` envelope via the shared encoder.
fn build_publish_relay_list_envelope(correlation_id: &str, entries: &[(&str, &str)]) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    const N65P_IDENTIFIER: &str = "N65P";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let entry_offsets: Vec<WIPOffset<_>> = entries
            .iter()
            .map(|(url, role)| {
                let url_off = fbb.create_string(url);
                let start = fbb.start_table();
                fbb.push_slot_always::<WIPOffset<&str>>(4 as VOffsetT, url_off); // slot 0: url
                fbb.push_slot::<u8>(6 as VOffsetT, relay_marker_byte(role), 0); // slot 1: marker
                fbb.end_table(start)
            })
            .collect();
        let relays_vec = fbb.create_vector(&entry_offsets);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<_>>(6 as VOffsetT, relays_vec); // slot 1: relays
        let root = fbb.end_table(start);
        fbb.finish(root, Some(N65P_IDENTIFIER));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, "nmp.nip65.publish_relay_list", 1, &payload)
}

/// `publishRelayList` builder bytes decode field-for-field to the expected
/// `PublishRelayListInput`, proving the host `relayMarkerByte` role→ordinal fold
/// (incl. the COMPOSITE `"both,indexer"` → `Both` case) matches the Rust decode,
/// then dispatch through `start_bytes` to `nmp.nip65.publish_relay_list`. The
/// wrong-namespace twin proves the route is real (the same bytes routed as
/// `nmp.nip51.block_relay` mis-decode and fail closed).
#[test]
fn publish_relay_list_builder_bytes_composite_role_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(PublishRelayListAction);
    let _ = registry.register_action(BlockRelayAction::new(Arc::new(
        InMemoryBlockedRelayCache::new(),
    )));

    // Composite "both,indexer" must fold to Both (NOT Indexer); "read,write" must
    // also fold to Both; bare markers map 1:1; "indexer" stays Indexer.
    let entries = [
        ("wss://relay.a", "both,indexer"),
        ("wss://relay.b", "read"),
        ("wss://relay.c", "write"),
        ("wss://relay.d", "read,write"),
        ("wss://relay.e", "indexer"),
    ];
    let bytes = build_publish_relay_list_envelope("corr-n65", &entries);

    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip65.publish_relay_list");
    assert_eq!(decoded.correlation_id, "corr-n65");

    let input = PublishRelayListInput::decode(&decoded.payload)
        .expect("the opaque payload must decode via PublishRelayListInput");
    let expected = vec![
        RelayListEntry { url: "wss://relay.a".to_string(), marker: RelayMarker::Both },
        RelayListEntry { url: "wss://relay.b".to_string(), marker: RelayMarker::Read },
        RelayListEntry { url: "wss://relay.c".to_string(), marker: RelayMarker::Write },
        RelayListEntry { url: "wss://relay.d".to_string(), marker: RelayMarker::Both },
        RelayListEntry { url: "wss://relay.e".to_string(), marker: RelayMarker::Indexer },
    ];
    assert_eq!(
        input.relays, expected,
        "composite role 'both,indexer' must decode to Both, 'read,write' to Both, \
         and the marker ordinals must round-trip exactly"
    );

    // POSITIVE: routed to the right namespace, payload decodes + start() OK
    // (the Both/Read/Write entries yield non-empty kind:10002 tags).
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("publishRelayList builder bytes must dispatch + validate via start_bytes");

    // LOAD-BEARING twin: the SAME N65P bytes routed as block_relay mis-decode and
    // fail closed, proving the positive above is not vacuous.
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.block_relay",
            &decoded.payload,
        )
        .expect_err("an N65P payload routed as block_relay must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

/// Build a flat `{schema_version, url, account_pubkey}` `DispatchEnvelope` shaped
/// as the generated `blockRelay`/`unblockRelay` builders emit (slots 0/1/2 at vt
/// 4/6/8) and stamp it into the `NMPD` envelope under `namespace`.
fn build_relay_url_pubkey_envelope(
    correlation_id: &str,
    namespace: &str,
    file_id: &str,
    url: &str,
    account_pubkey: &str,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let url_off = fbb.create_string(url);
        let pubkey_off = fbb.create_string(account_pubkey);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, url_off); // slot 1: url
        fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, pubkey_off); // slot 2: account_pubkey
        let root = fbb.end_table(start);
        fbb.finish(root, Some(file_id));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, namespace, 1, &payload)
}

/// `blockRelay` builder bytes decode field-for-field to `BlockRelayInput` and
/// dispatch through `start_bytes` to `nmp.nip51.block_relay`. Wrong-namespace
/// twin (routed as publish_relay_list) proves the route is real.
#[test]
fn block_relay_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(BlockRelayAction::new(Arc::new(
        InMemoryBlockedRelayCache::new(),
    )));
    let _ = registry.register_action(PublishRelayListAction);

    let bytes = build_relay_url_pubkey_envelope(
        "corr-blk",
        "nmp.nip51.block_relay",
        "NBLK",
        "wss://relay.example",
        PUBKEY,
    );
    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip51.block_relay");
    assert_eq!(
        BlockRelayInput::decode(&decoded.payload).expect("payload must decode via BlockRelayInput"),
        BlockRelayInput {
            url: "wss://relay.example".to_string(),
            account_pubkey: PUBKEY.to_string(),
        },
        "blockRelay builder bytes must decode field-for-field"
    );
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("blockRelay builder bytes must dispatch + validate via start_bytes");
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip65.publish_relay_list",
            &decoded.payload,
        )
        .expect_err("an NBLK payload routed as publish_relay_list must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

/// Anti-drift table: assert that `relay_marker_byte` (the Rust mirror of the
/// generated emitter helper) produces the correct byte for every role string
/// that may arrive from a host shell, AND that `PublishRelayListInput::decode`
/// agrees — valid roles decode successfully to the expected `RelayMarker`,
/// invalid/empty roles encode as the 255 sentinel and the Rust decoder FAILS
/// CLOSED (Err) rather than silently producing Both.
///
/// This is the load-bearing guard that the three codegen emitters
/// (`kotlin.rs` / `swift.rs` / `ts.rs`) stay honest against
/// `RelayMarker::from_role_string` — the single SSOT in `nmp-router`.
#[test]
fn relay_marker_byte_anti_drift_table() {
    // (role string, expected wire byte, expected decoded RelayMarker)
    let valid_cases: &[(&str, u8, RelayMarker)] = &[
        ("both", 0, RelayMarker::Both),
        ("read", 1, RelayMarker::Read),
        ("write", 2, RelayMarker::Write),
        ("indexer", 3, RelayMarker::Indexer),
        ("both,indexer", 0, RelayMarker::Both),   // composite → Both wins
        ("read,write", 0, RelayMarker::Both),      // read+write → Both
        ("Read", 1, RelayMarker::Read),            // case-insensitive
        ("read,", 1, RelayMarker::Read),           // trailing comma → empty part is no-op
    ];
    let invalid_cases: &[&str] = &["", "content", "foo,bar", "read,bogus"];

    for &(role, expected_byte, expected_marker) in valid_cases {
        let byte = relay_marker_byte(role);
        assert_eq!(
            byte, expected_byte,
            "relay_marker_byte({role:?}) expected {expected_byte} got {byte}"
        );
        // Prove the byte round-trips through the REAL Rust decoder.
        let payload = build_n65p_payload_with_raw_marker(byte);
        let decoded = PublishRelayListInput::decode(&payload)
            .unwrap_or_else(|e| panic!("valid role {role:?} byte={byte} must decode: {e:?}"));
        assert_eq!(
            decoded.relays[0].marker, expected_marker,
            "role {role:?} byte={byte} must decode to {expected_marker:?}"
        );
    }

    for &role in invalid_cases {
        let byte = relay_marker_byte(role);
        assert_eq!(
            byte, 255,
            "invalid/empty role {role:?} must encode as sentinel 255, got {byte}"
        );
        // Prove 255 causes the REAL Rust decoder to FAIL CLOSED (not silently Both).
        let payload = build_n65p_payload_with_raw_marker(255);
        let result = PublishRelayListInput::decode(&payload);
        assert!(
            result.is_err(),
            "sentinel 255 for role {role:?} must cause decode to fail closed, \
             got Ok({:?})",
            result.ok()
        );
    }
}

/// `unblockRelay` builder bytes decode field-for-field to `UnblockRelayInput`
/// and dispatch through `start_bytes` to `nmp.nip51.unblock_relay` (the cache is
/// pre-seeded so `start` does not reject an already-unblocked relay). The
/// wrong-namespace twin proves the route is real.
#[test]
fn unblock_relay_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    cache.upsert(PUBKEY.to_string(), vec!["wss://relay.example".to_string()]);
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(UnblockRelayAction::new(cache));
    let _ = registry.register_action(PublishRelayListAction);

    let bytes = build_relay_url_pubkey_envelope(
        "corr-ublk",
        "nmp.nip51.unblock_relay",
        "NUBL",
        "wss://relay.example",
        PUBKEY,
    );
    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip51.unblock_relay");
    assert_eq!(
        UnblockRelayInput::decode(&decoded.payload)
            .expect("payload must decode via UnblockRelayInput"),
        UnblockRelayInput {
            url: "wss://relay.example".to_string(),
            account_pubkey: PUBKEY.to_string(),
        },
        "unblockRelay builder bytes must decode field-for-field"
    );
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("unblockRelay builder bytes must dispatch + validate via start_bytes");
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip65.publish_relay_list",
            &decoded.payload,
        )
        .expect_err("an NUBL payload routed as publish_relay_list must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}
