//! kind:38000 — NIP-87 mint recommendation / review codec.
//!
//! A user vouches for a mint by publishing a kind:38000 event that names the
//! recommended announcement kind in a `k` tag and points at the mint via an
//! `a` tag (the `38172:<pubkey>:<d>` coordinate) and/or a `u` tag (mint URL).
//! The event `content` MAY carry free-text review.
//!
//! **Cashu only.** [`decode_mint_recommendation`] rejects any recommendation
//! whose `k` tag is not `38172` (the Cashu announcement kind) — including
//! kind:38173 Fedimint recommendations, which are out of scope for NMP. This is
//! deliberate fail-closed decoding: a recommendation with no `k`, or a non-Cashu
//! `k`, decodes to `None` rather than being silently treated as a Cashu vouch.

use nostr::{EventBuilder, Kind, Tag, TagKind};

use crate::announcement::raw_tags;
use crate::kinds::{KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND};

/// Decoded kind:38000 mint recommendation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintRecommendation {
    /// Event id (hex).
    pub event_id: String,
    /// Recommending author's pubkey (hex) — the identity the reading account
    /// web-of-trust-scores at aggregation time.
    pub author: String,
    /// Announcement coordinates recommended (`a` tags: `38172:<pubkey>:<d>`).
    pub mint_coordinates: Vec<String>,
    /// Mint URLs recommended (`u` tags).
    pub mint_urls: Vec<String>,
    /// Free-text review, if any.
    pub content: String,
}

/// Fields for [`build_mint_recommendation`].
#[derive(Clone, Debug, Default)]
pub struct MintRecommendationDraft<'a> {
    /// Announcement coordinates to recommend (`a` tags).
    pub mint_coordinates: &'a [String],
    /// Mint URLs to recommend (`u` tags).
    pub mint_urls: &'a [String],
    /// Free-text review (`content`).
    pub content: &'a str,
}

/// Build a kind:38000 Cashu mint recommendation. Always emits `["k", "38172"]`
/// so the recommendation is unambiguously a Cashu vouch.
#[must_use]
pub fn build_mint_recommendation(draft: &MintRecommendationDraft<'_>) -> EventBuilder {
    let mut tags: Vec<Tag> = Vec::new();
    tags.push(Tag::custom(
        TagKind::custom("k"),
        [KIND_MINT_ANNOUNCE.to_string()],
    ));
    for coordinate in draft.mint_coordinates {
        tags.push(Tag::custom(TagKind::custom("a"), [coordinate.as_str()]));
    }
    for url in draft.mint_urls {
        tags.push(Tag::custom(TagKind::custom("u"), [url.as_str()]));
    }
    EventBuilder::new(Kind::from(KIND_MINT_RECOMMEND as u16), draft.content).tags(tags)
}

/// Decode a kind:38000 recommendation from raw parts.
///
/// Returns `None` unless a `k` tag naming the Cashu announcement kind (`38172`)
/// is present, and unless the recommendation references at least one mint (via
/// an `a` coordinate or a `u` URL) — a vouch that names no mint is unusable.
#[must_use]
pub fn decode_mint_recommendation(
    event_id: &str,
    author: &str,
    tags: &[Vec<String>],
    content: &str,
) -> Option<MintRecommendation> {
    let mut recommends_cashu = false;
    let mut mint_coordinates: Vec<String> = Vec::new();
    let mut mint_urls: Vec<String> = Vec::new();

    for tag in tags {
        let Some(key) = tag.first() else { continue };
        let value = tag.get(1).map(String::as_str);
        match key.as_str() {
            "k" => {
                if value.and_then(|v| v.trim().parse::<u32>().ok()) == Some(KIND_MINT_ANNOUNCE) {
                    recommends_cashu = true;
                }
            }
            "a" => {
                // Only accept coordinates for the Cashu announcement kind.
                if let Some(v) = value {
                    if v.starts_with(&format!("{KIND_MINT_ANNOUNCE}:")) {
                        mint_coordinates.push(v.to_string());
                    }
                }
            }
            "u" => {
                if let Some(v) = value {
                    mint_urls.push(v.to_string());
                }
            }
            _ => {}
        }
    }

    if !recommends_cashu {
        return None;
    }
    if mint_coordinates.is_empty() && mint_urls.is_empty() {
        return None;
    }

    Some(MintRecommendation {
        event_id: event_id.to_string(),
        author: author.to_string(),
        mint_coordinates,
        mint_urls,
        content: content.to_string(),
    })
}

/// Convenience decoder over a signed `nostr::Event`.
#[must_use]
pub fn decode_mint_recommendation_event(event: &nostr::Event) -> Option<MintRecommendation> {
    if event.kind != Kind::from(KIND_MINT_RECOMMEND as u16) {
        return None;
    }
    decode_mint_recommendation(
        &event.id.to_hex(),
        &event.pubkey.to_hex(),
        &raw_tags(event),
        &event.content,
    )
}

/// Filter matching every kind:38000 recommendation. The reading account scopes
/// which recommenders it trusts in Rust (web of trust), not at the relay.
#[must_use]
pub fn mint_recommendation_filter() -> nostr::Filter {
    nostr::Filter::new().kind(Kind::from(KIND_MINT_RECOMMEND as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    fn sign(builder: EventBuilder) -> nostr::Event {
        builder.sign_with_keys(&Keys::generate()).expect("sign")
    }

    #[test]
    fn round_trips_through_a_signed_event() {
        let coords = vec!["38172:abc:mintpubkey".to_string()];
        let urls = vec!["https://mint.example".to_string()];
        let draft = MintRecommendationDraft {
            mint_coordinates: &coords,
            mint_urls: &urls,
            content: "great mint",
        };
        let event = sign(build_mint_recommendation(&draft));
        let decoded = decode_mint_recommendation_event(&event).expect("decodes");
        assert_eq!(decoded.mint_coordinates, coords);
        assert_eq!(decoded.mint_urls, urls);
        assert_eq!(decoded.content, "great mint");
        assert_eq!(decoded.author, event.pubkey.to_hex());
    }

    #[test]
    fn recommendation_without_k_tag_is_rejected() {
        let tags = vec![vec!["u".to_string(), "https://mint.example".to_string()]];
        assert!(decode_mint_recommendation("id", "author", &tags, "").is_none());
    }

    #[test]
    fn fedimint_recommendation_is_rejected() {
        // kind:38173 is Fedimint — out of scope.
        let tags = vec![
            vec!["k".to_string(), "38173".to_string()],
            vec!["u".to_string(), "https://fed.example".to_string()],
        ];
        assert!(decode_mint_recommendation("id", "author", &tags, "").is_none());
    }

    #[test]
    fn recommendation_naming_no_mint_is_rejected() {
        let tags = vec![vec!["k".to_string(), "38172".to_string()]];
        assert!(decode_mint_recommendation("id", "author", &tags, "").is_none());
    }

    #[test]
    fn wrong_kind_event_is_rejected() {
        let event = sign(EventBuilder::new(Kind::from(1u16), "hi"));
        assert!(decode_mint_recommendation_event(&event).is_none());
    }
}
