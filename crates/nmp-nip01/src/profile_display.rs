//! Profile display helpers: `AuthorDisplay` (wire type for UI) and
//! `ProfileDisplay` (kernel-side cache entry).  `profile_from_event` decodes
//! kind:0 metadata events; `should_replace` enforces last-writer-wins ordering.

use nmp_core::nip19::encode_npub;
use nmp_core::substrate::{picture_placeholder, KernelEvent};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorDisplaySource {
    Kind0,
    Npub,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorDisplay {
    pub name: String,
    pub npub: String,
    pub picture_url: String,
    pub source: AuthorDisplaySource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileDisplay {
    pub display: String,
    pub picture_url: Option<String>,
    pub created_at: u64,
    pub event_id: String,
}

impl AuthorDisplay {
    #[must_use] 
    pub fn fallback(pubkey: &str) -> Self {
        let npub = encode_npub(pubkey).unwrap_or_else(|_| short_key(pubkey));
        Self {
            name: npub.clone(),
            npub,
            picture_url: picture_placeholder(pubkey),
            source: AuthorDisplaySource::Npub,
        }
    }

    #[must_use] 
    pub fn from_profile(pubkey: &str, profile: Option<&ProfileDisplay>) -> Self {
        let Some(profile) = profile else {
            return Self::fallback(pubkey);
        };
        let fallback = Self::fallback(pubkey);
        let picture_url = profile
            .picture_url
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback.picture_url.as_str())
            .to_string();
        Self {
            name: profile.display.clone(),
            npub: fallback.npub,
            picture_url,
            source: AuthorDisplaySource::Kind0,
        }
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
        .or_else(|| string_field(&parsed, "name"))
        .unwrap_or_else(|| AuthorDisplay::fallback(&event.author).name);
    let picture_url = string_field(&parsed, "picture").filter(|value| value.starts_with("http"));
    Some(ProfileDisplay {
        display,
        picture_url,
        created_at: event.created_at,
        event_id: event.id.clone(),
    })
}

#[must_use]
pub fn should_replace(current: Option<&ProfileDisplay>, candidate: &ProfileDisplay) -> bool {
    current
        .is_none_or(|profile| {
            candidate.created_at > profile.created_at
                || (candidate.created_at == profile.created_at
                    && candidate.event_id < profile.event_id)
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

fn short_key(value: &str) -> String {
    if value.len() <= 12 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 4..])
    }
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

        assert_eq!(profile.display, "Alice A.");
        assert_eq!(
            profile.picture_url.as_deref(),
            Some("https://example.com/a.png")
        );
    }

    #[test]
    fn replacement_uses_created_at_then_event_id() {
        let old = ProfileDisplay {
            display: "old".to_string(),
            picture_url: None,
            created_at: 10,
            event_id: "b".to_string(),
        };
        let tie_winner = ProfileDisplay {
            display: "new".to_string(),
            picture_url: None,
            created_at: 10,
            event_id: "a".to_string(),
        };
        assert!(should_replace(Some(&old), &tie_winner));
    }
}
