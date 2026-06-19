//! `Kind0Parser` — the [`IngestParser`] that decodes kind:0 profile-metadata
//! events and upserts the parsed [`ProfileView`] into [`ProfileCache`].
//!
//! Structural sibling of `nmp_nip17::Kind10050Parser` (kind:10050) and
//! `nmp_router::Kind10002Parser` (NIP-65 kind:10002). The kernel's
//! [`nmp_core::substrate::EventIngestDispatcher`] fans every accepted
//! `Inserted | Replaced` event to every registered parser; this parser filters
//! on `evt.raw().kind == 0` so an unintended dispatch is a silent no-op rather
//! than corrupting the profile cache.
//!
//! # Parse contract — exact port of the kernel's former `parse_profile`
//!
//! The decode logic is a verbatim port of the kernel's old
//! `kernel::nostr::parse_profile` so the cache shape is byte-identical to the
//! pre-PR-2 behaviour:
//!
//! * **display** — first non-empty of `display_name`, `displayName`, `name`,
//!   trimmed. Empty string when none (aim.md §2 — no `short_npub` fallback;
//!   the projection boundary converts `""` to `None`).
//! * **picture_url** — `picture`, kept only when it starts with `http`.
//! * **nip05** / **about** — verbatim, empty string when absent.
//! * **lnurl** — `lud16` preferred over `lud06`; both empty-trimmed values
//!   filter out (the zap button stays disabled when the key carries an empty
//!   value).
//!
//! Supersession (newest kind:0 wins, lexicographic event-id tiebreak) is owned
//! by [`ProfileCache::upsert_view`].

use std::sync::Arc;

use nmp_core::store::VerifiedEvent;
use nmp_core::substrate::{IngestParser, ProfileView};
use serde_json::{Map, Value};

use crate::profile_cache::ProfileCache;

/// NIP-01 — the kind number for profile-metadata events.
const KIND_PROFILE_METADATA: u32 = 0;

/// The kind:0 ingest parser. Constructed with a shared `Arc<ProfileCache>`
/// handle — the same `Arc` the kernel holds as its `Arc<dyn ProfileLookup>`,
/// so the writer side (this parser) and the reader side (the kernel's
/// enrichment / claim-TTL / zap-LNURL paths) see one source of truth.
pub struct Kind0Parser {
    cache: Arc<ProfileCache>,
}

impl Kind0Parser {
    /// Construct a parser writing into the supplied [`ProfileCache`].
    #[must_use]
    pub fn new(cache: Arc<ProfileCache>) -> Self {
        Self { cache }
    }

    /// Static-dispatch path for tests and direct callers. Returns `false`
    /// (no-op) when `evt`'s kind is not 0; otherwise parses + upserts and
    /// returns whether the candidate superseded the cached entry (the change
    /// signal — newest kind:0 wins).
    pub fn parse_event(&self, evt: &VerifiedEvent) -> bool {
        let raw = evt.raw();
        if raw.kind != KIND_PROFILE_METADATA {
            return false;
        }
        let candidate = parse_profile_view(&raw.id, raw.created_at, &raw.content);
        self.cache.upsert_view(raw.pubkey.clone(), candidate)
    }
}

impl IngestParser for Kind0Parser {
    fn parse(&self, evt: &VerifiedEvent) {
        let _ = self.parse_event(evt);
    }
}

/// Decode a kind:0 `content` JSON object into a [`ProfileView`]. Verbatim port
/// of the kernel's former `parse_profile`.
fn parse_profile_view(event_id: &str, created_at: u64, content: &str) -> ProfileView {
    let raw_fields = serde_json::from_str::<Map<String, Value>>(content).unwrap_or_default();
    let name = string_field(&raw_fields, "name");
    let raw_display_name = string_field(&raw_fields, "display_name");
    let display_name_camel = string_field(&raw_fields, "displayName");
    let picture = string_field(&raw_fields, "picture");
    let nip05 = string_field(&raw_fields, "nip05");
    let about = string_field(&raw_fields, "about");
    let lud16 = string_field(&raw_fields, "lud16");
    let lud06 = string_field(&raw_fields, "lud06");
    let display = raw_display_name
        .clone()
        .or_else(|| display_name_camel.clone())
        .or_else(|| name.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    ProfileView {
        event_id: event_id.to_string(),
        created_at,
        display,
        name,
        raw_display_name,
        display_name_camel,
        picture_url: picture.filter(|value| value.starts_with("http")),
        banner: string_field(&raw_fields, "banner"),
        website: string_field(&raw_fields, "website"),
        nip05: nip05.unwrap_or_default(),
        about: about.unwrap_or_default(),
        lud16: lud16.clone(),
        lud06: lud06.clone(),
        lnurl: lud16
            .filter(|s| !s.trim().is_empty())
            .or_else(|| lud06.filter(|s| !s.trim().is_empty())),
        raw_fields,
    }
}

fn string_field(raw_fields: &Map<String, Value>, key: &str) -> Option<String> {
    raw_fields
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::RawEvent;
    use nmp_core::substrate::{EventIngestDispatcher, ProfileLookup};

    fn evt(pubkey: &str, id: &str, kind: u32, created_at: u64, content: &str) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: id.into(),
            pubkey: pubkey.into(),
            created_at,
            kind,
            tags: Vec::new(),
            content: content.into(),
            sig: "22".repeat(64),
        })
    }

    #[test]
    fn ignores_non_kind_0() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        assert!(!parser.parse_event(&evt("alice", "aa", 1, 100, r#"{"name":"x"}"#)));
        assert!(cache.is_empty());
    }

    #[test]
    fn parses_all_fields() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        let content = r#"{"display_name":"Alice","picture":"https://img.example/a.png","nip05":"alice@example.com","about":"hi","lud16":"alice@ln.example"}"#;
        assert!(parser.parse_event(&evt("alice", "aa", 0, 100, content)));
        let v = cache.profile("alice").expect("cached");
        assert_eq!(v.display, "Alice");
        assert_eq!(v.picture_url.as_deref(), Some("https://img.example/a.png"));
        assert_eq!(v.nip05, "alice@example.com");
        assert_eq!(v.about, "hi");
        assert_eq!(v.raw_display_name.as_deref(), Some("Alice"));
        assert_eq!(v.lnurl.as_deref(), Some("alice@ln.example"));
        assert_eq!(v.lud16.as_deref(), Some("alice@ln.example"));
        assert_eq!(v.event_id, "aa");
        assert_eq!(v.created_at, 100);
    }

    #[test]
    fn captures_app_neutral_raw_fields_and_unknowns() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        let content = r#"{"name":"alice","displayName":"Alice C","banner":"nostr:bad","website":"https://example.com","unknown_app_field":{"x":1}}"#;
        assert!(parser.parse_event(&evt("alice", "aa", 0, 100, content)));
        let v = cache.profile("alice").expect("cached");
        assert_eq!(v.name.as_deref(), Some("alice"));
        assert_eq!(v.display_name_camel.as_deref(), Some("Alice C"));
        assert_eq!(v.banner.as_deref(), Some("nostr:bad"));
        assert_eq!(v.website.as_deref(), Some("https://example.com"));
        assert_eq!(
            v.raw_fields.get("unknown_app_field"),
            Some(&serde_json::json!({"x":1}))
        );
    }

    #[test]
    fn display_name_precedence_and_trim() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        // display_name wins over name
        parser.parse_event(&evt("a", "1", 0, 1, r#"{"display_name":" D ","name":"N"}"#));
        assert_eq!(cache.profile("a").expect("cached").display, "D");
        // displayName (camel) used when display_name absent
        parser.parse_event(&evt(
            "b",
            "1",
            0,
            1,
            r#"{"displayName":"Camel","name":"N"}"#,
        ));
        assert_eq!(cache.profile("b").expect("cached").display, "Camel");
        // name used when neither present
        parser.parse_event(&evt("c", "1", 0, 1, r#"{"name":"Nm"}"#));
        assert_eq!(cache.profile("c").expect("cached").display, "Nm");
    }

    #[test]
    fn non_http_picture_filtered() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        parser.parse_event(&evt(
            "a",
            "1",
            0,
            1,
            r#"{"picture":"data:image/png;base64,x"}"#,
        ));
        assert!(cache.profile("a").expect("cached").picture_url.is_none());
    }

    #[test]
    fn lud06_used_when_lud16_absent_or_empty() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        parser.parse_event(&evt(
            "a",
            "1",
            0,
            1,
            r#"{"lud16":"  ","lud06":"lnurl1abc"}"#,
        ));
        assert_eq!(
            cache.profile("a").expect("cached").lnurl.as_deref(),
            Some("lnurl1abc")
        );
    }

    #[test]
    fn malformed_json_yields_empty_profile() {
        let cache = Arc::new(ProfileCache::new());
        let parser = Kind0Parser::new(Arc::clone(&cache));
        assert!(parser.parse_event(&evt("a", "1", 0, 1, "not json")));
        let v = cache.profile("a").expect("cached");
        assert!(v.display.is_empty());
        assert!(v.picture_url.is_none());
        assert!(v.lnurl.is_none());
    }

    #[test]
    fn newer_kind0_supersedes_via_dispatcher() {
        let cache = Arc::new(ProfileCache::new());
        let parser: Arc<dyn IngestParser> = Arc::new(Kind0Parser::new(Arc::clone(&cache)));
        let mut d = EventIngestDispatcher::new();
        d.register_kind(0, parser);

        d.dispatch(&evt("a", "old", 0, 100, r#"{"display_name":"Old"}"#));
        d.dispatch(&evt("a", "new", 0, 200, r#"{"display_name":"New"}"#));

        assert_eq!(cache.profile("a").expect("cached").display, "New");
    }
}
