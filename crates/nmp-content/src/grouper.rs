//! Post-tokenization groupers — consecutive media URLs collapse into one
//! `Segment::Media`. Text segments containing only whitespace/punctuation
//! bridge runs (matching NDKSwift's `ImageGroupingUtils` behavior — see
//! `docs/research/content-rendering/ndkswift.md` §2).
//!
//! Grouping is a separate pass from tokenization because classification
//! (whether a URL is media or generic) is a **rendering** concern, not a
//! protocol one. Keeping the cut means apps that want raw URLs can skip
//! the grouper.

use crate::segment::{MediaKind, Segment};
use url::Url;

/// Apply media grouping: any run of `Segment::Url` whose URL classifies as
/// media (per [`media_kind_for_url`]), optionally bridged by whitespace-only
/// or empty `Text` segments, collapses into one `Segment::Media`.
///
/// Returns a new vector — the input is consumed.
pub(crate) fn group_consecutive_media(input: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::with_capacity(input.len());
    let mut pending_urls: Vec<Url> = Vec::new();
    let mut pending_kind: Option<MediaKind> = None;
    let mut pending_bridge: Vec<Segment> = Vec::new();

    for seg in input {
        match seg {
            Segment::Url(url) => {
                if let Some(kind) = media_kind_for_url(&url) {
                    if let Some(existing) = pending_kind {
                        if existing == kind {
                            // Same media kind — bridge stays implicit (we drop
                            // whitespace bridge segments).
                            pending_urls.push(url);
                            pending_bridge.clear();
                            continue;
                        }
                    }
                    flush_media(&mut out, &mut pending_urls, &mut pending_kind, &mut pending_bridge);
                    pending_urls.push(url);
                    pending_kind = Some(kind);
                } else {
                    flush_media(&mut out, &mut pending_urls, &mut pending_kind, &mut pending_bridge);
                    out.push(Segment::Url(url));
                }
            }
            Segment::Text(t) if pending_kind.is_some() && is_bridging_text(&t) => {
                pending_bridge.push(Segment::Text(t));
            }
            other => {
                flush_media(&mut out, &mut pending_urls, &mut pending_kind, &mut pending_bridge);
                out.push(other);
            }
        }
    }

    flush_media(&mut out, &mut pending_urls, &mut pending_kind, &mut pending_bridge);
    out
}

fn flush_media(
    out: &mut Vec<Segment>,
    urls: &mut Vec<Url>,
    kind: &mut Option<MediaKind>,
    bridge: &mut Vec<Segment>,
) {
    if let Some(k) = kind.take() {
        if urls.len() == 1 {
            let only = urls.remove(0);
            out.push(Segment::Media { urls: vec![only], kind: k });
        } else {
            out.push(Segment::Media { urls: std::mem::take(urls), kind: k });
        }
    }
    out.extend(std::mem::take(bridge));
    urls.clear();
}

/// Whitespace + bridging punctuation (commas, newlines). Anything heavier
/// (letters, digits) breaks the media run.
fn is_bridging_text(s: &str) -> bool {
    s.chars().all(|c| c.is_whitespace() || matches!(c, ',' | '·' | '|'))
}

/// Classify a URL as media by extension. Returns `None` for non-media URLs.
/// Pure URL-extension inference per the §10 rule: no MIME sniff, no HTTP.
pub(crate) fn media_kind_for_url(url: &Url) -> Option<MediaKind> {
    let path = url.path();
    let lower = path.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, ext)| ext)?;
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "avif" | "heic" => Some(MediaKind::Image),
        "mp4" | "mov" | "webm" | "m4v" | "mkv" => Some(MediaKind::Video),
        "mp3" | "m4a" | "wav" | "ogg" | "flac" | "opus" => Some(MediaKind::Audio),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn classifies_image_extensions() {
        assert_eq!(media_kind_for_url(&u("https://x/a.jpg")), Some(MediaKind::Image));
        assert_eq!(media_kind_for_url(&u("https://x/a.PNG")), Some(MediaKind::Image));
        assert_eq!(media_kind_for_url(&u("https://x/a.webp?q=1")), Some(MediaKind::Image));
    }

    #[test]
    fn classifies_video_extensions() {
        assert_eq!(media_kind_for_url(&u("https://x/a.mp4")), Some(MediaKind::Video));
        assert_eq!(media_kind_for_url(&u("https://x/a.MOV")), Some(MediaKind::Video));
    }

    #[test]
    fn classifies_audio_extensions() {
        assert_eq!(media_kind_for_url(&u("https://x/a.mp3")), Some(MediaKind::Audio));
        assert_eq!(media_kind_for_url(&u("https://x/a.opus")), Some(MediaKind::Audio));
    }

    #[test]
    fn rejects_non_media_extensions() {
        assert_eq!(media_kind_for_url(&u("https://x/a.html")), None);
        assert_eq!(media_kind_for_url(&u("https://x/a")), None);
    }

    #[test]
    fn groups_two_consecutive_images() {
        let input = vec![
            Segment::Url(u("https://x/a.jpg")),
            Segment::Text(" ".to_string()),
            Segment::Url(u("https://x/b.jpg")),
        ];
        let out = group_consecutive_media(input);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Segment::Media { kind: MediaKind::Image, ref urls } if urls.len() == 2));
    }

    #[test]
    fn does_not_group_image_then_video() {
        let input = vec![
            Segment::Url(u("https://x/a.jpg")),
            Segment::Url(u("https://x/b.mp4")),
        ];
        let out = group_consecutive_media(input);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn single_image_becomes_media_segment() {
        let input = vec![Segment::Url(u("https://x/a.jpg"))];
        let out = group_consecutive_media(input);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Segment::Media { kind: MediaKind::Image, .. }));
    }

    #[test]
    fn text_between_images_with_letters_breaks_group() {
        let input = vec![
            Segment::Url(u("https://x/a.jpg")),
            Segment::Text(" caption ".to_string()),
            Segment::Url(u("https://x/b.jpg")),
        ];
        let out = group_consecutive_media(input);
        // Letters break the run; the text and second image are separate.
        assert_eq!(out.len(), 3);
    }
}
