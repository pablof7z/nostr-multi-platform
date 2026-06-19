use super::*;

fn tag(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn parses_full_imeta_tag() {
    let parsed = parse_imeta_tag(&tag(&[
        "imeta",
        "url https://cdn.example/a.jpg",
        "m image/jpeg",
        "x abc123",
        "thumbhash th",
        "blurhash bh",
        "dim 3024x4032",
        "alt Coast",
        "fallback https://mirror.example/a.jpg",
        "annotate-user pubkey::10,20",
        "custom value",
    ]))
    .unwrap();

    assert_eq!(parsed.url, "https://cdn.example/a.jpg");
    assert_eq!(parsed.mime.as_deref(), Some("image/jpeg"));
    assert_eq!(parsed.sha256.as_deref(), Some("abc123"));
    assert_eq!(
        parsed.dimensions,
        Some(MediaDimensions {
            width: 3024,
            height: 4032
        })
    );
    assert_eq!(parsed.alt.as_deref(), Some("Coast"));
    assert_eq!(parsed.fallbacks, vec!["https://mirror.example/a.jpg"]);
    assert_eq!(parsed.annotations, vec!["pubkey::10,20"]);
    assert_eq!(parsed.extra, vec![ImetaField::new("custom", "value")]);
}

#[test]
fn rejects_missing_url_and_url_only_tags() {
    assert!(parse_imeta_tag(&tag(&["imeta", "m image/png"])).is_none());
    assert!(parse_imeta_tag(&tag(&["imeta", "url https://cdn.example/a.png"])).is_none());
}

#[test]
fn accepts_non_image_media_mime() {
    let parsed = parse_imeta_tag(&tag(&[
        "imeta",
        "url https://cdn.example/a.mp4",
        "m video/mp4",
        "x abc",
    ]))
    .unwrap();

    assert_eq!(parsed.mime.as_deref(), Some("video/mp4"));
}

#[test]
fn imeta_tag_round_trips_ordered_fields() {
    let media = MediaMeta::new("https://cdn.example/a.webp")
        .sha256("abc")
        .mime("image/webp")
        .dimensions(640, 480)
        .alt("alt text")
        .fallback("https://mirror.example/a.webp");

    assert_eq!(
        media.imeta_tag(),
        tag(&[
            "imeta",
            "url https://cdn.example/a.webp",
            "x abc",
            "m image/webp",
            "dim 640x480",
            "alt alt text",
            "fallback https://mirror.example/a.webp",
        ])
    );
}
