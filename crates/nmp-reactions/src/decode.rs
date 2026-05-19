//! Decoder half (read side) per `docs/design/kind-wrappers.md` §3.1.
//!
//! Pure, allocation-bounded, no I/O. Returns the typed [`SocialRecord`] an app
//! views read instead of re-parsing tag arrays in hot paths (D8).
//!
//! Decoder name is the uniform `try_from_event` per PD-010 — every NMP
//! protocol crate exposes this verb so consumers can write
//! `nmp_reactions::try_from_event(&event)` without learning per-crate
//! vocabulary.
//!
//! ## One decoder, classified output
//!
//! Reactions (kind:7), reposts (kind:6) and generic reposts (kind:16) share the
//! same target-extraction shape (last `e`/`a` tag → target, `p` tag → target
//! author) and the same provenance fields (`event_id`, `author`, `created_at`,
//! verbatim `tags`). The only per-kind divergence is a small payload tail:
//! reactions carry `content` + an optional `emoji`; reposts carry the verbatim
//! original-event JSON string; generic reposts carry the original `k` kind.
//!
//! Per `kind-wrappers.md` §9 anti-pattern #3 ("one-class-many-kinds with a
//! `static kinds[]` and `kind`-discriminated branching inside getters"), the
//! per-kind tail is modelled as a closed [`SocialKind`] enum **on an immutable
//! record**, not as branching getters over a mutable tag bag. There is exactly
//! one decoder function (`decode_borrowed`); the enum is data, not dispatch.
//! Two records behind an enum was the considered alternative — rejected because
//! every consumer (indexes, views, summaries) needs the shared target/reactor
//! fields uniformly, and an outer enum would force a match at every read site
//! purely to reach fields that are identical across variants.

use nmp_core::planner::NaddrCoord;
use nmp_core::store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use serde::{Deserialize, Serialize};

use crate::kinds::{KIND_GENERIC_REPOST, KIND_REACTION, KIND_REPOST};

/// What a reaction / repost points at.
///
/// Per NIP-25, the target is referenced by the **last** `e` tag (an event id)
/// or, for addressable targets, the last `a` tag (an `naddr` coordinate). The
/// variant is tagged so the byte key encoding in [`crate::domain::keys`] can
/// keep event-id targets and address targets in disjoint key spaces.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum ReactionTarget {
    /// Reaction/repost of a concrete event, by hex event id.
    Event(String),
    /// Reaction/repost of an addressable (replaceable) event, by coordinate.
    Address(NaddrCoord),
}

/// The per-kind payload tail. Shared fields live on [`SocialRecord`]; this enum
/// is the only thing that differs by kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SocialKind {
    /// kind:7 — `content` is `"+"`, `"-"`, an emoji, or a `:shortcode:`.
    Reaction {
        /// The reaction string verbatim (NIP-25 requires it non-empty; we
        /// preserve whatever the wire sent, including the empty string, so the
        /// decoder stays a faithful read of the event — emptiness is a
        /// build-time concern, see [`crate::build`]).
        content: String,
        /// Optional `["emoji", <shortcode>, <url>]` custom-emoji binding.
        emoji: Option<EmojiRef>,
    },
    /// kind:6 — repost of a kind:1 note. `embedded` is the `.content` field
    /// verbatim: per NIP-18 it is the stringified original event JSON, or an
    /// empty string. We deliberately do NOT parse it (the task brief: "keep the
    /// string"); re-parsing would be I/O-shaped speculative work the views do
    /// not need.
    Repost { embedded: String },
    /// kind:16 — generic repost of any kind. `original_kind` is the `k` tag
    /// parsed strict-or-`None` to `Option<u32>` (anti-pattern §9 #8: normalise
    /// once at decode, never coerce on read).
    GenericRepost {
        embedded: String,
        original_kind: Option<u32>,
    },
}

/// A custom-emoji reference from a NIP-30 `["emoji", <shortcode>, <url>]` tag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmojiRef {
    /// Shortcode without surrounding colons (e.g. `soapbox`).
    pub shortcode: String,
    /// Image URL the shortcode resolves to.
    pub url: String,
}

/// Decoded NIP-25 reaction / NIP-18 repost. Immutable; produced once at ingest,
/// read everywhere per `kind-wrappers.md` §1 (no read-side mutable wrappers —
/// that is the D4 violation NDK's setter pattern commits).
///
/// `created_at` and `tags` are preserved alongside the design-doc surface
/// because views sort/filter by `created_at` and occasionally need unknown-tag
/// access without re-fetching the raw event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SocialRecord {
    /// Hex event id of *this* reaction/repost (its own immutable identity —
    /// the primary key, since kinds 7/6/16 are regular, not replaceable).
    pub event_id: String,
    /// Hex pubkey of the reactor / reposter.
    pub author: String,
    /// What is being reacted to / reposted (last `e` or `a` tag).
    pub target: ReactionTarget,
    /// Target author from the `p` tag, if present (NIP-25 SHOULD include it).
    pub target_author: Option<String>,
    /// `created_at` from the event header (unix seconds). Always set.
    pub created_at: u64,
    /// Per-kind payload tail.
    pub kind: SocialKind,
    /// Raw tags preserved verbatim for callers needing unknown-tag access.
    pub tags: Vec<Vec<String>>,
}

impl SocialRecord {
    /// `true` for kind:7 reactions.
    #[must_use]
    pub fn is_reaction(&self) -> bool {
        matches!(self.kind, SocialKind::Reaction { .. })
    }

    /// `true` for kind:6 / kind:16 reposts.
    #[must_use]
    pub fn is_repost(&self) -> bool {
        matches!(
            self.kind,
            SocialKind::Repost { .. } | SocialKind::GenericRepost { .. }
        )
    }

    /// The reaction string for kind:7, else `None`.
    #[must_use]
    pub fn reaction_content(&self) -> Option<&str> {
        match &self.kind {
            SocialKind::Reaction { content, .. } => Some(content.as_str()),
            _ => None,
        }
    }
}

/// Decode a stored event into a [`SocialRecord`].
///
/// Returns `None` when `event.kind` is not in `{7, 6, 16}`. All target/`p`/
/// `emoji`/`k` tags are optional at decode time (the wire can be sloppy); the
/// builders in [`crate::build`] enforce the NIP invariants on the write side.
pub fn try_from_event(event: &StoredEvent) -> Option<SocialRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(
        &raw.id,
        &raw.pubkey,
        raw.kind,
        raw.created_at,
        &raw.tags,
        &raw.content,
    )
}

/// Decode directly from a borrowed [`KernelEvent`] — the view-substrate event
/// shape — without re-wrapping it in a `StoredEvent`/`Arc<RawEvent>`.
///
/// The hot insert path in `view::accumulator` calls this on every delivered
/// event; per D8 (zero per-event alloc after warmup) it must not allocate an
/// intermediate `RawEvent` + `Arc`. The only allocations are the owned
/// `String`s the immutable record output unavoidably owns.
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<SocialRecord> {
    decode_borrowed(
        &event.id,
        &event.author,
        event.kind,
        event.created_at,
        &event.tags,
        &event.content,
    )
}

/// Shared decode core over borrowed event fields. Both entry points funnel
/// through here so kind gating and tag normalisation stay defined exactly once.
fn decode_borrowed(
    id: &str,
    pubkey: &str,
    kind: u32,
    created_at: u64,
    tags: &[Vec<String>],
    content: &str,
) -> Option<SocialRecord> {
    let social_kind = match kind {
        KIND_REACTION => SocialKind::Reaction {
            content: content.to_string(),
            emoji: extract_emoji(tags),
        },
        KIND_REPOST => SocialKind::Repost {
            embedded: content.to_string(),
        },
        KIND_GENERIC_REPOST => SocialKind::GenericRepost {
            embedded: content.to_string(),
            original_kind: first_tag_value(tags, "k").and_then(|s| s.parse::<u32>().ok()),
        },
        _ => return None,
    };

    Some(SocialRecord {
        event_id: id.to_string(),
        author: pubkey.to_string(),
        target: extract_target(tags),
        target_author: last_tag_value(tags, "p").map(str::to_string),
        created_at,
        kind: social_kind,
        tags: tags.to_vec(),
    })
}

/// Extract the reaction/repost target.
///
/// Per NIP-25 the relevant tag is the **last** `e` (event id) or `a`
/// (addressable coordinate) tag. We prefer an `a` tag when present (an
/// addressable target is more specific); otherwise we fall back to the last
/// `e` tag. If neither is present the target is an empty-event sentinel so the
/// record stays decodable (D1: always-renderable; emptiness is observable, not
/// a decode failure).
fn extract_target(tags: &[Vec<String>]) -> ReactionTarget {
    if let Some(coord) = last_addressable(tags) {
        return ReactionTarget::Address(coord);
    }
    let event_id = last_tag_value(tags, "e").unwrap_or("").to_string();
    ReactionTarget::Event(event_id)
}

/// Parse the last `a` tag into an [`NaddrCoord`]. NIP-01 `a` tag value is
/// `<kind>:<pubkey>:<d-tag>`; a malformed value is treated as absent.
fn last_addressable(tags: &[Vec<String>]) -> Option<NaddrCoord> {
    let raw = last_tag_value(tags, "a")?;
    let mut parts = raw.splitn(3, ':');
    let kind = parts.next()?.parse::<u32>().ok()?;
    let pubkey = parts.next()?.to_string();
    let d_tag = parts.next().unwrap_or("").to_string();
    Some(NaddrCoord {
        pubkey,
        kind,
        d_tag,
    })
}

/// Extract a NIP-30 `["emoji", <shortcode>, <url>]` custom-emoji binding (first
/// such tag wins; reactions reference at most one custom emoji).
fn extract_emoji(tags: &[Vec<String>]) -> Option<EmojiRef> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some("emoji"))
        .and_then(|t| match (t.get(1), t.get(2)) {
            (Some(shortcode), Some(url)) => Some(EmojiRef {
                shortcode: shortcode.clone(),
                url: url.clone(),
            }),
            _ => None,
        })
}

/// Second column of the *first* tag whose first column equals `key`.
fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(|s| s.as_str()) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

/// Second column of the *last* tag whose first column equals `key`. NIP-25
/// target precedence: the last `e`/`a`/`p` tag is the authoritative one.
fn last_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .rev()
        .find(|t| t.first().map(|s| s.as_str()) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::{RawEvent, StoredEvent};
    use std::sync::Arc;

    fn make_stored(kind: u32, tags: Vec<Vec<String>>, content: &str) -> StoredEvent {
        StoredEvent {
            raw: Arc::new(RawEvent {
                id: "a".repeat(64),
                pubkey: "b".repeat(64),
                created_at: 1_700_000_000,
                kind,
                tags,
                content: content.into(),
                sig: "c".repeat(128),
            }),
            received_at_ms: 0,
        }
    }

    #[test]
    fn decodes_kind_7_reaction_content() {
        let event = make_stored(
            7,
            vec![
                vec!["e".into(), "f".repeat(64)],
                vec!["p".into(), "d".repeat(64)],
            ],
            "+",
        );
        let r = try_from_event(&event).expect("kind:7 decodes");
        assert!(r.is_reaction());
        assert_eq!(r.reaction_content(), Some("+"));
        assert_eq!(r.target, ReactionTarget::Event("f".repeat(64)));
        assert_eq!(r.target_author.as_deref(), Some(&"d".repeat(64)[..]));
    }

    #[test]
    fn decodes_kind_6_repost_preserves_content_verbatim() {
        let original_json = r#"{"id":"abc","kind":1,"content":"hi"}"#;
        let event = make_stored(6, vec![vec!["e".into(), "f".repeat(64)]], original_json);
        let r = try_from_event(&event).expect("kind:6 decodes");
        assert!(r.is_repost());
        match r.kind {
            SocialKind::Repost { embedded } => assert_eq!(embedded, original_json),
            _ => panic!("expected Repost"),
        }
    }

    #[test]
    fn decodes_kind_16_generic_repost_parses_k_tag() {
        let event = make_stored(
            16,
            vec![
                vec!["e".into(), "f".repeat(64)],
                vec!["k".into(), "30023".into()],
            ],
            "",
        );
        let r = try_from_event(&event).expect("kind:16 decodes");
        match r.kind {
            SocialKind::GenericRepost { original_kind, .. } => {
                assert_eq!(original_kind, Some(30023));
            }
            _ => panic!("expected GenericRepost"),
        }
    }

    #[test]
    fn kind_16_non_numeric_k_tag_is_none() {
        let event = make_stored(16, vec![vec!["k".into(), "not-a-kind".into()]], "");
        let r = try_from_event(&event).unwrap();
        match r.kind {
            SocialKind::GenericRepost { original_kind, .. } => assert_eq!(original_kind, None),
            _ => panic!("expected GenericRepost"),
        }
    }

    #[test]
    fn wrong_kind_returns_none() {
        let event = make_stored(1, vec![vec!["e".into(), "x".into()]], "note");
        assert!(try_from_event(&event).is_none());
    }

    #[test]
    fn last_e_tag_wins_as_target() {
        let event = make_stored(
            7,
            vec![
                vec!["e".into(), "a".repeat(64)],
                vec!["e".into(), "b".repeat(64)],
            ],
            "+",
        );
        let r = try_from_event(&event).unwrap();
        assert_eq!(r.target, ReactionTarget::Event("b".repeat(64)));
    }

    #[test]
    fn a_tag_target_is_address_variant() {
        let event = make_stored(
            7,
            vec![vec![
                "a".into(),
                format!("30023:{}:my-article", "d".repeat(64)),
            ]],
            "+",
        );
        let r = try_from_event(&event).unwrap();
        match r.target {
            ReactionTarget::Address(coord) => {
                assert_eq!(coord.kind, 30023);
                assert_eq!(coord.pubkey, "d".repeat(64));
                assert_eq!(coord.d_tag, "my-article");
            }
            _ => panic!("expected Address target"),
        }
    }

    #[test]
    fn emoji_tag_extracted_for_reaction() {
        let event = make_stored(
            7,
            vec![
                vec!["e".into(), "f".repeat(64)],
                vec![
                    "emoji".into(),
                    "soapbox".into(),
                    "https://gleasonator.com/emoji/soapbox.png".into(),
                ],
            ],
            ":soapbox:",
        );
        let r = try_from_event(&event).unwrap();
        match r.kind {
            SocialKind::Reaction { content, emoji } => {
                assert_eq!(content, ":soapbox:");
                let e = emoji.expect("emoji extracted");
                assert_eq!(e.shortcode, "soapbox");
                assert_eq!(e.url, "https://gleasonator.com/emoji/soapbox.png");
            }
            _ => panic!("expected Reaction"),
        }
    }

    #[test]
    fn tags_preserved_verbatim() {
        let event = make_stored(
            7,
            vec![
                vec!["e".into(), "f".repeat(64)],
                vec!["custom".into(), "value".into()],
            ],
            "+",
        );
        let r = try_from_event(&event).unwrap();
        assert_eq!(r.tags.len(), 2);
        assert_eq!(r.tags[1], vec!["custom".to_string(), "value".to_string()]);
    }

    #[test]
    fn carries_created_at_from_header() {
        let event = make_stored(7, vec![vec!["e".into(), "x".into()]], "+");
        assert_eq!(try_from_event(&event).unwrap().created_at, 1_700_000_000);
    }

    #[test]
    fn last_p_tag_wins_as_target_author() {
        let event = make_stored(
            7,
            vec![
                vec!["p".into(), "a".repeat(64)],
                vec!["p".into(), "b".repeat(64)],
                vec!["e".into(), "f".repeat(64)],
            ],
            "+",
        );
        let r = try_from_event(&event).unwrap();
        assert_eq!(r.target_author.as_deref(), Some(&"b".repeat(64)[..]));
    }
}
