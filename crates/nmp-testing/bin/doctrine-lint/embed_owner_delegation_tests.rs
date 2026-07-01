//! Ratchet for #2594: `nmp-content` may dispatch embed kinds, but it must not
//! parse protocol-owner artifact semantics for owned protocol kinds locally.

use std::fs;

use super::workspace_root;

#[test]
fn nmp_content_embed_projection_delegates_owned_protocol_kinds() {
    let root = workspace_root();
    let path = root.join("crates/nmp-content/src/embed_projection/mod.rs");
    let body = fs::read_to_string(&path).expect("read embed projection dispatch");

    assert!(
        body.contains("nmp_nip01::profile_metadata_projection_from_event"),
        "kind:0 embed projection must call the NIP-01 owner adapter"
    );
    assert!(
        body.contains("nmp_nip84::highlight_projection_from_event"),
        "kind:9802 embed projection must call the NIP-84 owner adapter"
    );
    assert!(
        body.contains("ARTICLE_PROJECTION_ADAPTER"),
        "kind:30023 embed projection must use the registered NIP-23 owner adapter"
    );

    for banned in [
        "parse_profile_metadata",
        "struct ProfileContent",
        "serde_json::from_str::<ProfileContent>",
        "let source_event_id = tag_value(\"e\")",
        "let source_event_addr = tag_value(\"a\")",
        "let source_url = tag_value(\"r\")",
        "let context = tag_value(\"context\")",
        "let title = tag_value(\"title\")",
        "let summary = tag_value(\"summary\")",
        "let hero_image_url = tag_value(\"image\")",
        "let d_tag = tag_value(\"d\")",
    ] {
        assert!(
            !body.contains(banned),
            "`nmp-content` must not reintroduce local owner-artifact parsing: {banned}"
        );
    }
}
