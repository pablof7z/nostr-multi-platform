use nmp_core::substrate::KernelEvent;
use serde::{Deserialize, Serialize};

/// Author display metadata derived from a kind:0 profile event.
///
/// Per aim.md §2 (NMP is a data framework; backend ships raw protocol
/// data, presentation layers own formatting), every field that can be
/// absent in kind:0 is modelled as `Option<String>` — the host shell
/// chooses how to render the missing case (typically by formatting
/// `author_pubkey` itself).
///
/// ADR-0032 / V-115: `npub` (bech32) has been removed — Rust must not
/// encode presentation-layer bech32 in projection types. Shells that
/// need bech32 must call `nmp_app_encode_profile` / their own encoder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorDisplay {
    /// Display name from kind:0 (`display_name` / `displayName` / `name`).
    /// `None` when no kind:0 has arrived yet for this author — presentation
    /// layer falls back to formatting the raw pubkey itself.
    pub name: Option<String>,
    /// `picture` URL from kind:0. `None` when no kind:0 has arrived yet,
    /// or when the kind:0 omits the `picture` field — presentation layer
    /// chooses a placeholder/identicon strategy.
    pub picture_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileDisplay {
    /// `display_name` / `displayName` / `name` from kind:0, or `None`
    /// when none of those fields are present in the parsed metadata.
    pub display: Option<String>,
    pub picture_url: Option<String>,
    pub created_at: u64,
    pub event_id: String,
}

impl AuthorDisplay {
    /// The "no kind:0 yet" shape — name and picture_url are absent.
    /// Shells that need bech32 must encode the raw pubkey themselves.
    #[must_use]
    pub fn fallback(_pubkey: &str) -> Self {
        Self {
            name: None,
            picture_url: None,
        }
    }

    /// Build from an optional `ProfileDisplay` (the kind:0 cache entry).
    /// When `profile` is `None` or carries only absent fields, the
    /// corresponding `AuthorDisplay` field is `None` and the host shell
    /// renders its own fallback.
    #[must_use]
    pub fn from_profile(pubkey: &str, profile: Option<&ProfileDisplay>) -> Self {
        let mut card = Self::fallback(pubkey);
        if let Some(profile) = profile {
            card.name = profile.display.clone();
            card.picture_url = profile.picture_url.clone();
        }
        card
    }
}

#[must_use]
pub fn profile_from_event(event: &KernelEvent) -> Option<ProfileDisplay> {
    if event.kind != 0 {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(&event.content).ok()?;
    let display = string_field(&parsed, "display_name")
        .or_else(|| string_field(&parsed, "displayName"))
        .or_else(|| string_field(&parsed, "name"));
    let picture_url = string_field(&parsed, "picture").filter(|value| value.starts_with("http"));
    Some(ProfileDisplay {
        display,
        picture_url,
        created_at: event.created_at,
        event_id: event.id.clone(),
    })
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: u32, content: &str, created_at: u64, id: &str) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d".to_string(),
            kind,
            created_at,
            tags: vec![],
            content: content.to_string(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn kind0_profile_prefers_display_name() {
        let profile = profile_from_event(&event(
            0,
            r#"{"name":"alice","display_name":"Alice A.","picture":"https://example.com/a.png"}"#,
            7,
            "b",
        ))
        .expect("profile");

        assert_eq!(profile.display.as_deref(), Some("Alice A."));
        assert_eq!(
            profile.picture_url.as_deref(),
            Some("https://example.com/a.png")
        );
    }

    #[test]
    fn kind0_profile_with_no_name_yields_none() {
        let profile =
            profile_from_event(&event(0, r#"{"about":"hello"}"#, 7, "b")).expect("profile");
        assert_eq!(profile.display, None);
        assert_eq!(profile.picture_url, None);
    }
}
