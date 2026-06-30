//! Golden wire fixtures for [`nmp_nip01::ModularTimelineSnapshot`] (issue #733).
//!
//! These pin the **binary** typed-FlatBuffers wire shape of the timeline
//! snapshot projection. The round-trip tests in `src/typed_wire/tests.rs` prove
//! the codec is lossless; this file additionally freezes the exact bytes so any
//! schema drift in `nmp_timeline.fbs` (or the encoder) becomes an explicit test
//! failure rather than a silent break of the Swift / Kotlin / TypeScript shells.
//!
//! It also asserts the cross-platform identity invariants (`FILE_IDENTIFIER`,
//! `SCHEMA_ID`, `SCHEMA_VERSION`) and an ADR-0037 parity property: the typed
//! binary decode is semantically equivalent to the authoritative serde
//! projection.
//!
//! To regenerate after an intentional schema change: run this test with
//! `--nocapture`, copy the `actual timeline_snapshot_empty_v2 hex:` line into
//! `tests/fixtures/timeline_snapshot_empty_v2.fb.hex`, and re-run.

use nmp_nip01::timeline_projection::RepostAttribution;
use nmp_nip01::typed_wire::{
    decode_modular_timeline_snapshot, encode_modular_timeline_snapshot, FILE_IDENTIFIER, SCHEMA_ID,
    SCHEMA_VERSION,
};
use nmp_nip01::{ModularTimelineSnapshot, TimelineEventCard};
use nmp_threading::{ThreadPointer, TimelineBlock};

/// Minimal representative snapshot: the empty projection. This is the shape the
/// kernel emits before any events land, so it must be byte-stable across schema
/// revisions.
fn golden_snapshot() -> ModularTimelineSnapshot {
    ModularTimelineSnapshot::empty()
}

/// Deterministic 32-byte hex id from a single byte (`0xab` -> "abab...ab").
fn event_id(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

/// A real, tokenized content tree exercising inline text, a hashtag, and a URL
/// — so the card fixture pins the nested `ContentTreeWire` shape too.
fn card_content_tree() -> nmp_content::ContentTreeWire {
    use nmp_content::{tokenize_with_kind, RenderMode};
    tokenize_with_kind("hello #nostr https://example.com", &[], RenderMode::Auto, 1).to_wire()
}

/// A card surfaced via a kind:6 repost. Exercises every load-bearing card path:
/// nested content tree and a `RepostAttribution`. GH #920: the card carries raw
/// protocol data only (no denormalized author display, preview, or embedded-ref
/// render facts).
fn repost_card() -> TimelineEventCard {
    TimelineEventCard {
        id: event_id(0x09),
        author_pubkey: event_id(0x02),
        kind: 6,
        created_at: 1_700_000_000,
        content: "hello world".to_string(),
        content_tree: card_content_tree(),
        reposted_by: Some(RepostAttribution {
            author_pubkey: event_id(0x42),
            note_created_at: 1_699_000_000,
        }),
        relay_provenance: Vec::new(),
    }
}

/// A representative card-bearing snapshot: one timeline block referencing the
/// card, plus the repost card itself. This is the "card" shape the #733
/// acceptance criterion calls for.
fn golden_card_snapshot() -> ModularTimelineSnapshot {
    ModularTimelineSnapshot {
        blocks: vec![TimelineBlock::Standalone {
            id: event_id(0x09),
            root: Some(ThreadPointer::Event {
                id: event_id(0x03),
                relay: Some("wss://relay.example".to_string()),
                kind: Some(1),
            }),
        }],
        cards: vec![repost_card()],
        page: None,
        metrics: None,
    }
}

fn decode_hex(hex: &str) -> Vec<u8> {
    let compact: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(compact.len() % 2, 0, "hex fixture must contain full bytes");
    compact
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("fixture is ascii hex");
            u8::from_str_radix(pair, 16).expect("fixture is valid hex")
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn timeline_snapshot_empty_golden_fixture_is_stable() {
    let snapshot = golden_snapshot();
    let wire = encode_modular_timeline_snapshot(&snapshot);
    let expected = decode_hex(include_str!("fixtures/timeline_snapshot_empty_v2.fb.hex"));
    if wire != expected {
        eprintln!(
            "actual timeline_snapshot_empty_v2 hex:\n{}",
            encode_hex(&wire)
        );
    }
    assert_eq!(
        wire, expected,
        "ModularTimelineSnapshot empty v2 golden fixture drifted"
    );
}

#[test]
fn timeline_snapshot_with_card_golden_fixture_is_stable() {
    let snapshot = golden_card_snapshot();
    let wire = encode_modular_timeline_snapshot(&snapshot);
    let expected = decode_hex(include_str!(
        "fixtures/timeline_snapshot_with_card_v2.fb.hex"
    ));
    if wire != expected {
        eprintln!(
            "actual timeline_snapshot_with_card_v2 hex:\n{}",
            encode_hex(&wire)
        );
    }
    assert_eq!(
        wire, expected,
        "ModularTimelineSnapshot with-card v2 golden fixture drifted"
    );
}

#[test]
fn timeline_snapshot_golden_fixture_has_nfts_identifier() {
    let wire = encode_modular_timeline_snapshot(&golden_snapshot());
    assert_eq!(
        &wire[4..8],
        FILE_IDENTIFIER,
        "buffer must carry the NFTS file identifier at bytes 4..8"
    );
    assert_eq!(FILE_IDENTIFIER, b"NFTS");
}

#[test]
fn schema_id_is_stable() {
    assert_eq!(SCHEMA_ID, "nmp.nip01.timeline");
    assert_eq!(SCHEMA_VERSION, 2);
}

/// ADR-0037 acceptance criterion: parity between typed and generic. The typed
/// encoder must produce bytes that decode back to a shape semantically
/// equivalent to the authoritative serde projection. Asserted on a card-bearing
/// snapshot — where the comparison actually probes every card / render-data /
/// repost field — not just the empty shell.
fn assert_typed_serde_parity(snapshot: &ModularTimelineSnapshot) {
    let typed_bytes = encode_modular_timeline_snapshot(snapshot);
    let decoded = decode_modular_timeline_snapshot(&typed_bytes).expect("must decode");
    let via_json = serde_json::to_value(snapshot).expect("serde");
    let via_typed_json = serde_json::to_value(&decoded).expect("serde");
    assert_eq!(
        via_json, via_typed_json,
        "typed decode must be semantically equivalent to the serde projection"
    );
}

#[test]
fn typed_snapshot_schema_id_matches_adr_0037() {
    // Empty shell: trivial but pins the no-events parity invariant.
    assert_typed_serde_parity(&ModularTimelineSnapshot::empty());
    // Card-bearing: the meaningful parity probe across all card paths.
    assert_typed_serde_parity(&golden_card_snapshot());
}
