use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::{session::Session, wire};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Profile {
    pub created_at: u64,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub nip05: Option<String>,
}

impl Profile {
    pub fn label(&self, pubkey: &str) -> String {
        self.display_name
            .as_deref()
            .or(self.name.as_deref())
            .or(self.nip05.as_deref())
            .map(clean_handle)
            .filter(|s| !s.is_empty())
            .map(|s| format!("@{s}"))
            .unwrap_or_else(|| fallback_handle(pubkey))
    }
}

pub fn remember_profiles(session: &mut Session, events: &[Value]) {
    for event in events {
        let Some((pubkey, profile)) = parse_profile_event(event) else {
            continue;
        };
        let should_replace = session
            .profiles
            .get(&pubkey)
            .map(|existing| profile.created_at >= existing.created_at)
            .unwrap_or(true);
        if should_replace {
            session.profiles.insert(pubkey, profile);
        }
    }
}

pub fn cache_note_authors(session: &mut Session, events: &[Value]) {
    let missing = missing_note_authors(session, events);
    if missing.is_empty() {
        return;
    }
    let profiles = wire::fetch(
        &session.relays,
        json!({"kinds":[0], "authors":missing, "limit":missing.len()}),
        session.wall,
    );
    remember_profiles(session, &profiles);
}

pub fn event_author_label(session: &Session, event: &Value) -> String {
    let pubkey = event.get("pubkey").and_then(Value::as_str).unwrap_or("?");
    session
        .profiles
        .get(pubkey)
        .map(|profile| profile.label(pubkey))
        .unwrap_or_else(|| fallback_handle(pubkey))
}

fn missing_note_authors(session: &Session, events: &[Value]) -> Vec<String> {
    let mut authors = BTreeSet::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64) != Some(1) {
            continue;
        }
        let Some(pubkey) = event.get("pubkey").and_then(Value::as_str) else {
            continue;
        };
        if !session.profiles.contains_key(pubkey) {
            authors.insert(pubkey.to_string());
        }
    }
    authors.into_iter().collect()
}

fn parse_profile_event(event: &Value) -> Option<(String, Profile)> {
    if event.get("kind").and_then(Value::as_u64) != Some(0) {
        return None;
    }
    let pubkey = event.get("pubkey").and_then(Value::as_str)?.to_string();
    let created_at = event.get("created_at").and_then(Value::as_u64).unwrap_or(0);
    let content = event.get("content").and_then(Value::as_str).unwrap_or("{}");
    let metadata = serde_json::from_str::<Value>(content).ok()?;
    Some((
        pubkey,
        Profile {
            created_at,
            name: string_field(&metadata, "name"),
            display_name: string_field(&metadata, "display_name"),
            nip05: string_field(&metadata, "nip05"),
        },
    ))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn clean_handle(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('@')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

fn fallback_handle(pubkey: &str) -> String {
    if pubkey.len() <= 12 {
        format!("@{pubkey}")
    } else {
        format!("@{}..{}", &pubkey[..8], &pubkey[pubkey.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBKEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn profile_label_prefers_kind0_names() {
        let profile = Profile {
            created_at: 1,
            name: Some("pablof7z".into()),
            display_name: None,
            nip05: None,
        };

        assert_eq!(profile.label(PUBKEY), "@pablof7z");
    }

    #[test]
    fn remembers_newest_profile_event() {
        let mut session = Session::default();
        remember_profiles(
            &mut session,
            &[
                json!({"kind":0,"pubkey":PUBKEY,"created_at":2,"content":"{\"name\":\"new\"}"}),
                json!({"kind":0,"pubkey":PUBKEY,"created_at":1,"content":"{\"name\":\"old\"}"}),
            ],
        );

        assert_eq!(
            session
                .profiles
                .get(PUBKEY)
                .map(|profile| profile.label(PUBKEY)),
            Some("@new".into())
        );
    }

    #[test]
    fn missing_note_authors_ignores_cached_and_non_notes() {
        let mut session = Session::default();
        session.profiles.insert(
            PUBKEY.into(),
            Profile {
                created_at: 1,
                name: Some("cached".into()),
                display_name: None,
                nip05: None,
            },
        );
        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let missing = missing_note_authors(
            &session,
            &[
                json!({"kind":1,"pubkey":PUBKEY}),
                json!({"kind":0,"pubkey":other}),
                json!({"kind":1,"pubkey":other}),
            ],
        );

        assert_eq!(missing, vec![other.to_string()]);
    }
}
