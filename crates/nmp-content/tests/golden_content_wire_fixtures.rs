//! Golden wire fixtures for [`nmp_content::wire::ContentTreeWire`] (issue #733).
//!
//! These pin the **binary** typed-FlatBuffers wire shape of a representative
//! content tree. The serde JSON shape is already pinned by
//! `src/wire/tests.rs`; this complements it by freezing the FlatBuffers bytes
//! the Swift / Kotlin / TypeScript shells decode with generated accessors.
//!
//! A drift here means the typed-FB encoder (or the `content_tree.fbs` schema)
//! changed in a way that breaks every cross-platform binary decoder. Update the
//! host decoders BEFORE regenerating this fixture.
//!
//! To regenerate after an intentional schema change: run this test with
//! `--nocapture`, copy the `actual content_tree_v1 hex:` line into
//! `tests/fixtures/content_tree_v1.fb.hex`, and re-run.

use nmp_content::mode::RenderMode;
use nmp_content::wire::{
    encode_content_tree, ContentTreeWire, WireNode, WireNostrUri, WireNostrUriKind, FILE_IDENTIFIER,
};

/// A representative content tree exercising a block node with inline children,
/// a literal text run, and a profile mention carrying a flattened NIP-21 URI.
/// The arena is index-coherent: node 0 (`Paragraph`) points at children 1..=3,
/// which all exist.
fn golden_content_tree() -> ContentTreeWire {
    ContentTreeWire {
        nodes: vec![
            WireNode::Paragraph {
                children: vec![1, 2, 3],
            },
            WireNode::Text {
                text: "Hello ".into(),
            },
            WireNode::Mention {
                uri: WireNostrUri {
                    uri: "nostr:npub1example".into(),
                    kind: WireNostrUriKind::Profile,
                    primary_id: "a".repeat(64),
                    relays: vec![],
                    author: None,
                    event_kind: None,
                },
            },
            WireNode::Text {
                text: " world".into(),
            },
        ],
        roots: vec![0],
        mode: RenderMode::Auto,
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
fn content_tree_golden_fixture_is_stable() {
    let tree = golden_content_tree();
    let wire = encode_content_tree(&tree);
    let expected = decode_hex(include_str!("fixtures/content_tree_v1.fb.hex"));
    if wire != expected {
        eprintln!("actual content_tree_v1 hex:\n{}", encode_hex(&wire));
    }
    assert_eq!(wire, expected, "ContentTreeWire v1 golden fixture drifted");
}

#[test]
fn content_tree_fixture_has_nfct_identifier() {
    let tree = golden_content_tree();
    let wire = encode_content_tree(&tree);
    assert_eq!(
        &wire[4..8],
        FILE_IDENTIFIER,
        "buffer must carry the NFCT file identifier at bytes 4..8"
    );
    assert_eq!(FILE_IDENTIFIER, b"NFCT");
}
