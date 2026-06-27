use crate::{MediaMeta, PicturePost, PicturePostBuildError, KIND_PICTURE_EVENT};

fn image(url: &str, hash: &str, mime: &str) -> MediaMeta {
    MediaMeta::new(url)
        .sha256(hash)
        .mime(mime)
        .dimensions(640, 480)
}

#[test]
fn builds_picture_post_draft_with_imeta_and_query_tags() {
    let draft = PicturePost::new(image("https://cdn.example/a.jpg", "abc", "image/jpeg"))
        .title("Sunset")
        .content("caption")
        .hashtag("#travel")
        .tagged_pubkey("alice")
        .geohash("9q5c")
        .language("en", "ISO-639-1")
        .build()
        .unwrap();

    assert_eq!(draft.kind, KIND_PICTURE_EVENT);
    assert_eq!(draft.content, "caption");
    assert_eq!(
        draft.tags,
        vec![
            vec!["title", "Sunset"],
            vec![
                "imeta",
                "url https://cdn.example/a.jpg",
                "x abc",
                "m image/jpeg",
                "dim 640x480",
            ],
            vec!["p", "alice"],
            vec!["m", "image/jpeg"],
            vec!["x", "abc"],
            vec!["t", "travel"],
            vec!["g", "9q5c"],
            vec!["L", "ISO-639-1"],
            vec!["l", "en", "ISO-639-1"],
        ]
        .into_iter()
        .map(|tag| tag.into_iter().map(str::to_string).collect::<Vec<String>>())
        .collect::<Vec<_>>()
    );
}

#[test]
fn rejects_missing_image_and_incomplete_imeta() {
    assert_eq!(
        PicturePost::with_images(Vec::new()).build().unwrap_err(),
        PicturePostBuildError::MissingImage
    );
    assert!(matches!(
        PicturePost::new(MediaMeta::new("https://cdn.example/a.jpg")).build(),
        Err(PicturePostBuildError::IncompleteMediaMetadata { .. })
    ));
}

#[test]
fn emits_empty_title_tag_when_title_is_omitted() {
    let draft = PicturePost::new(image("https://cdn.example/a.jpg", "abc", "image/jpeg"))
        .build()
        .unwrap();

    assert_eq!(
        draft.tags.first(),
        Some(&vec!["title".into(), String::new()])
    );
}

#[test]
fn rejects_unsupported_mime() {
    let err = PicturePost::new(
        MediaMeta::new("https://cdn.example/a.mp4")
            .sha256("abc")
            .mime("video/mp4"),
    )
    .build()
    .unwrap_err();
    assert_eq!(
        err,
        PicturePostBuildError::UnsupportedMime {
            mime: "video/mp4".to_string()
        }
    );
}

#[test]
fn materializes_unsigned_event_when_author_and_time_are_supplied() {
    let unsigned = PicturePost::new(image("https://cdn.example/a.png", "abc", "image/png"))
        .content("caption")
        .build_unsigned("author", 123)
        .unwrap();

    assert_eq!(unsigned.pubkey, "author");
    assert_eq!(unsigned.kind, KIND_PICTURE_EVENT);
    assert_eq!(unsigned.created_at, 123);
    assert_eq!(unsigned.content, "caption");
}
