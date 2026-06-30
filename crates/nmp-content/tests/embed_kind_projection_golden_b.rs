//! Golden JSON serde tests for `EmbedKindProjection` — Part B (F-CR-12).
//!
//! Covers: Profile, Unknown, and the variant-tag camelCase contract.
//! Part A (`embed_kind_projection_golden_a.rs`) covers ShortNote, Article,
//! and Highlight.

use nmp_content::embed_projection::{
    ArticleProjection, EmbedKindProjection, HighlightProjection, ProfileProjection,
    ShortNoteProjection, UnknownProjection,
};
use nmp_content::wire::ContentTreeWire;
use nmp_content::RenderMode;

fn empty_tree() -> ContentTreeWire {
    ContentTreeWire {
        nodes: vec![],
        roots: vec![],
        mode: RenderMode::Auto,
    }
}

fn check_golden(label: &str, value: &EmbedKindProjection, expected: &str) {
    let actual = serde_json::to_string(value)
        .unwrap_or_else(|e| panic!("{label}: serde_json::to_string failed: {e}"));
    if actual != expected {
        eprintln!("{label} actual:\n{actual}");
    }
    assert_eq!(actual, expected, "{label}: EmbedKindProjection JSON golden drifted");
}

fn check_roundtrip(label: &str, value: &EmbedKindProjection) {
    let json = serde_json::to_string(value)
        .unwrap_or_else(|e| panic!("{label}: serialize failed: {e}"));
    let back: EmbedKindProjection = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("{label}: deserialize failed: {e}"));
    let json2 = serde_json::to_string(&back)
        .unwrap_or_else(|e| panic!("{label}: re-serialize failed: {e}"));
    assert_eq!(json, json2, "{label}: serde roundtrip mismatch");
}

// ── Profile ────────────────────────────────────────────────────────────────

#[test]
fn profile_minimal_golden_json() {
    let proj = EmbedKindProjection::Profile(ProfileProjection {
        pubkey: "eeeeffff".repeat(8), // 64 chars
        display_name: None,
        picture_url: None,
        about: None,
        nip05: None,
        lud16: None,
        banner_url: None,
    });
    // Structural assertions: all nullable fields must serialise as null.
    let json = serde_json::to_string(&proj).expect("serialize");
    assert!(json.starts_with(r#"{"variant":"profile","data":{"#), "profile variant tag");
    assert!(json.contains(r#""displayName":null"#), "displayName null");
    assert!(json.contains(r#""pictureUrl":null"#), "pictureUrl null");
    assert!(json.contains(r#""about":null"#), "about null");
    assert!(json.contains(r#""nip05":null"#), "nip05 null");
    assert!(json.contains(r#""lud16":null"#), "lud16 null");
    assert!(json.contains(r#""bannerUrl":null"#), "bannerUrl null");
    check_roundtrip("Profile/minimal", &proj);
}

#[test]
fn profile_full_golden_json() {
    let pk = "aabbaabb".repeat(8); // 64 chars
    let proj = EmbedKindProjection::Profile(ProfileProjection {
        pubkey: pk.clone(),
        display_name: Some("Alice NMP".to_string()),
        picture_url: Some("https://nmp.test/alice.png".to_string()),
        about: Some("Building NMP".to_string()),
        nip05: Some("alice@nmp.test".to_string()),
        lud16: Some("alice@getalby.test".to_string()),
        banner_url: Some("https://nmp.test/alice-banner.jpg".to_string()),
    });
    let expected = format!(
        concat!(
            r#"{{"variant":"profile","data":{{"pubkey":"{pk}","displayName":"Alice NMP","#,
            r#""pictureUrl":"https://nmp.test/alice.png","about":"Building NMP","#,
            r#""nip05":"alice@nmp.test","lud16":"alice@getalby.test","#,
            r#""bannerUrl":"https://nmp.test/alice-banner.jpg"}}}}"#,
        ),
        pk = pk,
    );
    check_golden("Profile/full", &proj, &expected);
    check_roundtrip("Profile/full", &proj);
}

// ── Unknown ────────────────────────────────────────────────────────────────

#[test]
fn unknown_minimal_golden_json() {
    let proj = EmbedKindProjection::Unknown(UnknownProjection {
        kind: 40,
        author_pubkey: "11112222".repeat(8), // 64 chars
        created_at: 1_700_100_000,
        content: r#"{"name":"nmp-dev"}"#.to_string(),
        content_tree: empty_tree(),
        tags: vec![],
        alt_text: None,
    });
    // Structural assertions: content has JSON-in-JSON escaping which is
    // fiddly to write as a static golden string.
    let json = serde_json::to_string(&proj).expect("serialize");
    assert!(json.starts_with(r#"{"variant":"unknown","data":{"#), "unknown variant tag");
    assert!(json.contains(r#""kind":40"#), "kind field");
    // Display separation (#2514): no author display fields on the wire.
    assert!(!json.contains("authorDisplayName"), "no authorDisplayName field");
    assert!(!json.contains("authorPictureUrl"), "no authorPictureUrl field");
    assert!(json.contains(r#""createdAt":1700100000"#), "createdAt");
    assert!(json.contains(r#""tags":[]"#), "tags empty");
    assert!(json.contains(r#""altText":null"#), "altText null");
    check_roundtrip("Unknown/minimal", &proj);
}

#[test]
fn unknown_with_tags_and_alt_golden_json() {
    let pk = "33334444".repeat(8); // 64 chars
    let proj = EmbedKindProjection::Unknown(UnknownProjection {
        kind: 30402,
        author_pubkey: pk.clone(),
        created_at: 1_700_200_000,
        content: "Classified ad".to_string(),
        content_tree: empty_tree(),
        tags: vec![
            vec!["price".to_string(), "42".to_string()],
            vec!["location".to_string(), "online".to_string()],
        ],
        alt_text: Some("a classified listing".to_string()),
    });
    let json = serde_json::to_string(&proj).expect("serialize");
    assert!(json.starts_with(r#"{"variant":"unknown","data":{"#), "variant tag");
    assert!(json.contains(r#""kind":30402"#), "kind 30402");
    // Display separation (#2514): no author display fields on the wire.
    assert!(!json.contains("authorDisplayName"), "no authorDisplayName field");
    assert!(json.contains(r#""altText":"a classified listing""#), "altText");
    assert!(json.contains(r#"["price","42"]"#), "price tag");
    assert!(json.contains(r#"["location","online"]"#), "location tag");
    check_roundtrip("Unknown/tags+alt", &proj);
}

// ── Variant tag shape contract ─────────────────────────────────────────────

/// Pin the exact camelCase variant tag strings that native decoders switch on.
///
/// Changing a variant name (e.g. "shortNote" → "short_note") is a breaking
/// wire change for every deployed iOS/Android/web client. This test fires loud
/// and fast if a refactor accidentally renames a serde tag.
#[test]
fn variant_tags_are_camel_case() {
    let cases: &[(&EmbedKindProjection, &str)] = &[
        (
            &EmbedKindProjection::ShortNote(ShortNoteProjection {
                id: "a".repeat(64),
                author_pubkey: "b".repeat(64),
                created_at: 0,
                content_tree: empty_tree(),
                media_urls: vec![],
            }),
            "shortNote",
        ),
        (
            &EmbedKindProjection::Article(ArticleProjection {
                id: "a".repeat(64),
                author_pubkey: "b".repeat(64),
                created_at: 0,
                title: None,
                summary: None,
                hero_image_url: None,
                d_tag: String::new(),
                content_tree: empty_tree(),
            }),
            "article",
        ),
        (
            &EmbedKindProjection::Highlight(HighlightProjection {
                id: "a".repeat(64),
                author_pubkey: "b".repeat(64),
                created_at: 0,
                highlighted_text: String::new(),
                source_event_id: None,
                source_event_addr: None,
                source_url: None,
                context: None,
            }),
            "highlight",
        ),
        (
            &EmbedKindProjection::Profile(ProfileProjection {
                pubkey: "a".repeat(64),
                display_name: None,
                picture_url: None,
                about: None,
                nip05: None,
                lud16: None,
                banner_url: None,
            }),
            "profile",
        ),
        (
            &EmbedKindProjection::Unknown(UnknownProjection {
                kind: 99,
                author_pubkey: "b".repeat(64),
                created_at: 0,
                content: String::new(),
                content_tree: empty_tree(),
                tags: vec![],
                alt_text: None,
            }),
            "unknown",
        ),
    ];

    for (proj, expected_tag) in cases {
        let json = serde_json::to_string(proj).expect("serialize");
        let tag_needle = format!(r#""variant":"{}""#, expected_tag);
        assert!(
            json.contains(&tag_needle),
            "expected variant tag `{expected_tag}` in: {json}"
        );
    }
}
