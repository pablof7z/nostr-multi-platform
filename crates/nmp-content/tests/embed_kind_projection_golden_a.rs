//! Golden JSON serde tests for `EmbedKindProjection` — Part A (F-CR-12).
//!
//! Covers: ShortNote, Article, Highlight.
//! Part B (`embed_kind_projection_golden_b.rs`) covers Profile, Unknown, and
//! the variant-tag contract test.
//!
//! The enum is serialised with:
//!   `#[serde(tag = "variant", content = "data", rename_all = "camelCase")]`
//! so JSON looks like:
//!   `{ "variant": "shortNote", "data": { … } }`

use nmp_content::embed_projection::{
    ArticleProjection, EmbedKindProjection, HighlightProjection, ShortNoteProjection,
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
    assert_eq!(
        actual, expected,
        "{label}: EmbedKindProjection JSON golden drifted"
    );
}

fn check_roundtrip(label: &str, value: &EmbedKindProjection) {
    let json =
        serde_json::to_string(value).unwrap_or_else(|e| panic!("{label}: serialize failed: {e}"));
    let back: EmbedKindProjection =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{label}: deserialize failed: {e}"));
    let json2 = serde_json::to_string(&back)
        .unwrap_or_else(|e| panic!("{label}: re-serialize failed: {e}"));
    assert_eq!(json, json2, "{label}: serde roundtrip mismatch");
}

// ── ShortNote ──────────────────────────────────────────────────────────────

#[test]
fn short_note_golden_json() {
    // "aabbccdd" × 8 = 64 chars, "11223344" × 8 = 64 chars
    let id = "aabbccdd".repeat(8);
    let pk = "11223344".repeat(8);
    let proj = EmbedKindProjection::ShortNote(ShortNoteProjection {
        id: id.clone(),
        author_pubkey: pk.clone(),
        created_at: 1_710_000_000,
        content_tree: empty_tree(),
        media_urls: vec![],
    });
    let expected = format!(
        concat!(
            r#"{{"variant":"shortNote","data":{{"id":"{id}","authorPubkey":"{pk}","#,
            r#""createdAt":1710000000,"#,
            r#""contentTree":{{"nodes":[],"roots":[],"mode":"Auto"}},"mediaUrls":[]}}}}"#,
        ),
        id = id,
        pk = pk,
    );
    check_golden("ShortNote", &proj, &expected);
    check_roundtrip("ShortNote", &proj);
}

#[test]
fn short_note_with_media_golden_json() {
    let id = "deadbeef".repeat(8); // 64 chars
    let pk = "cafebabe".repeat(8); // 64 chars
    let proj = EmbedKindProjection::ShortNote(ShortNoteProjection {
        id: id.clone(),
        author_pubkey: pk.clone(),
        created_at: 1_720_000_000,
        content_tree: empty_tree(),
        media_urls: vec!["https://nmp.test/photo.jpg".to_string()],
    });
    let expected = format!(
        concat!(
            r#"{{"variant":"shortNote","data":{{"id":"{id}","authorPubkey":"{pk}","#,
            r#""createdAt":1720000000,"contentTree":{{"nodes":[],"roots":[],"mode":"Auto"}},"#,
            r#""mediaUrls":["https://nmp.test/photo.jpg"]}}}}"#,
        ),
        id = id,
        pk = pk,
    );
    check_golden("ShortNote/media", &proj, &expected);
    check_roundtrip("ShortNote/media", &proj);
}

// ── Article ────────────────────────────────────────────────────────────────

#[test]
fn article_golden_json() {
    let proj = EmbedKindProjection::Article(ArticleProjection {
        id: "a1b2c3d4".repeat(8),            // 64 chars
        author_pubkey: "e5f6a7b8".repeat(8), // 64 chars
        created_at: 1_700_000_000,
        title: Some("Backpressure Is A Feature".to_string()),
        summary: Some("Why your relay should push back.".to_string()),
        hero_image_url: None,
        d_tag: "backpressure-is-a-feature".to_string(),
        content_tree: empty_tree(),
    });
    let expected = concat!(
        r#"{"variant":"article","data":{"id":"a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4a1b2c3d4","#,
        r#""authorPubkey":"e5f6a7b8e5f6a7b8e5f6a7b8e5f6a7b8e5f6a7b8e5f6a7b8e5f6a7b8e5f6a7b8","#,
        r#""createdAt":1700000000,"#,
        r#""title":"Backpressure Is A Feature","summary":"Why your relay should push back.","#,
        r#""heroImageUrl":null,"dTag":"backpressure-is-a-feature","#,
        r#""contentTree":{"nodes":[],"roots":[],"mode":"Auto"}}}"#,
    );
    check_golden("Article", &proj, expected);
    check_roundtrip("Article", &proj);
}

#[test]
fn article_with_hero_image_golden_json() {
    let proj = EmbedKindProjection::Article(ArticleProjection {
        id: "11223344".repeat(8),            // 64 chars
        author_pubkey: "55667788".repeat(8), // 64 chars
        created_at: 1_710_500_000,
        title: Some("Relays Are CDNs".to_string()),
        summary: None,
        hero_image_url: Some("https://nmp.test/hero.png".to_string()),
        d_tag: "relays-are-cdns".to_string(),
        content_tree: empty_tree(),
    });
    let expected = concat!(
        r#"{"variant":"article","data":{"id":"1122334411223344112233441122334411223344112233441122334411223344","#,
        r#""authorPubkey":"5566778855667788556677885566778855667788556677885566778855667788","#,
        r#""createdAt":1710500000,"title":"Relays Are CDNs","summary":null,"#,
        r#""heroImageUrl":"https://nmp.test/hero.png","dTag":"relays-are-cdns","#,
        r#""contentTree":{"nodes":[],"roots":[],"mode":"Auto"}}}"#,
    );
    check_golden("Article/hero", &proj, expected);
    check_roundtrip("Article/hero", &proj);
}

// ── Highlight ──────────────────────────────────────────────────────────────

#[test]
fn highlight_minimal_golden_json() {
    let proj = EmbedKindProjection::Highlight(HighlightProjection {
        id: "fedcba98".repeat(8),            // 64 chars
        author_pubkey: "01234567".repeat(8), // 64 chars
        created_at: 1_715_000_000,
        highlighted_text: "the honest answer is: slow down".to_string(),
        source_event_id: None,
        source_event_addr: None,
        source_url: None,
        context: None,
    });
    let expected = concat!(
        r#"{"variant":"highlight","data":{"id":"fedcba98fedcba98fedcba98fedcba98fedcba98fedcba98fedcba98fedcba98","#,
        r#""authorPubkey":"0123456701234567012345670123456701234567012345670123456701234567","#,
        r#""createdAt":1715000000,"#,
        r#""highlightedText":"the honest answer is: slow down","#,
        r#""sourceEventId":null,"sourceEventAddr":null,"sourceUrl":null,"context":null}}"#,
    );
    check_golden("Highlight/minimal", &proj, expected);
    check_roundtrip("Highlight/minimal", &proj);
}

#[test]
fn highlight_with_source_golden_json() {
    let pk = "ccccdddd".repeat(8); // 64 chars
    let source_addr = format!("30023:{pk}:bp-feature");
    let proj = EmbedKindProjection::Highlight(HighlightProjection {
        id: "aaaabbbb".repeat(8), // 64 chars
        author_pubkey: pk.clone(),
        created_at: 1_716_000_000,
        highlighted_text: "backpressure is a feature".to_string(),
        source_event_id: None,
        source_event_addr: Some(source_addr.clone()),
        source_url: Some("https://blog.nmp.test/bp-feature".to_string()),
        context: Some("surrounding prose context".to_string()),
    });
    let expected = format!(
        concat!(
            r#"{{"variant":"highlight","data":{{"id":"aaaabbbbaaaabbbbaaaabbbbaaaabbbbaaaabbbbaaaabbbbaaaabbbbaaaabbbb","#,
            r#""authorPubkey":"{pk}","createdAt":1716000000,"#,
            r#""highlightedText":"backpressure is a feature","sourceEventId":null,"#,
            r#""sourceEventAddr":"{sa}","sourceUrl":"https://blog.nmp.test/bp-feature","#,
            r#""context":"surrounding prose context"}}}}"#,
        ),
        pk = pk,
        sa = source_addr,
    );
    check_golden("Highlight/source", &proj, &expected);
    check_roundtrip("Highlight/source", &proj);
}
