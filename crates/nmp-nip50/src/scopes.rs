//! NIP-50 public search scopes (issue #1811).
//!
//! This crate owns the *meaning* of each searchable field: which kind-0 JSON
//! keys make a profile searchable, that a note's body is its `content`, and
//! that a long-form article is found by title + summary + a bounded prefix of
//! its body. The store (`nmp-store`) sees only opaque `(SearchField, text)`
//! pairs + the shared tokenizer; `nmp-core` compiles these providers into a
//! noun-free `CompiledIndexSpec` and installs them. No protocol noun leaks into
//! the kernel (D0).

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{
    CacheSearchMode, SearchIndexSpec, SearchPrivacyPolicy, SearchScopeProvider, SearchScopeRegistrar,
};
use nmp_store::{SearchField, SearchScopeId, StoredEvent};

use crate::request::KIND_LONG_FORM;

/// Stable scope labels — also the bridge the result projection uses to map a
/// [`crate::SearchScope`] to the store's [`SearchScopeId`].
pub const SCOPE_LABEL_PROFILES: &str = "nip50.profiles";
pub const SCOPE_LABEL_NOTES: &str = "nip50.notes";
pub const SCOPE_LABEL_LONGFORM: &str = "nip50.longform";

const KIND_PROFILE: u32 = 0;
const KIND_NOTE: u32 = 1;

/// Long-form body bytes indexed (the rest is dropped at extract time so a
/// pathological article body can't blow the per-doc token budget — the store
/// also caps at `MAX_TOKENS_PER_DOC`, this is the field-meaning-aware bound).
const LONGFORM_BODY_PREFIX_BYTES: usize = 4096;

// ─── profiles (kind:0) ───────────────────────────────────────────────────────

/// Profiles scope: extracts `name` / `display_name` / `about` / `nip05` from
/// the kind-0 metadata JSON content. Display fields are weighted above `about`.
pub struct ProfileSearchScope;

impl ProfileSearchScope {
    const F_NAME: SearchField = SearchField::with_weight(0, 3);
    const F_DISPLAY_NAME: SearchField = SearchField::with_weight(1, 3);
    const F_NIP05: SearchField = SearchField::with_weight(2, 2);
    const F_ABOUT: SearchField = SearchField::with_weight(3, 1);

    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProfileSearchScope {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchScopeProvider for ProfileSearchScope {
    fn spec(&self) -> SearchIndexSpec {
        SearchIndexSpec {
            scope: SearchScopeId::from_label(SCOPE_LABEL_PROFILES),
            source: "nip50.profiles (kind:0 metadata)",
            kinds: BTreeSet::from([KIND_PROFILE]),
            fields: vec![
                Self::F_NAME,
                Self::F_DISPLAY_NAME,
                Self::F_NIP05,
                Self::F_ABOUT,
            ],
            privacy: SearchPrivacyPolicy::PublicIndexable,
            cache_mode: CacheSearchMode::Both,
        }
    }

    fn extract(&self, event: &StoredEvent) -> Vec<(SearchField, String)> {
        let Ok(serde_json::Value::Object(map)) =
            serde_json::from_str::<serde_json::Value>(&event.raw.content)
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // NIP-24 allows both `display_name` and the legacy `displayName`.
        let str_field = |key: &str| map.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        if let Some(v) = str_field("name") {
            out.push((Self::F_NAME, v));
        }
        if let Some(v) = str_field("display_name").or_else(|| str_field("displayName")) {
            out.push((Self::F_DISPLAY_NAME, v));
        }
        if let Some(v) = str_field("nip05") {
            out.push((Self::F_NIP05, v));
        }
        if let Some(v) = str_field("about") {
            out.push((Self::F_ABOUT, v));
        }
        out
    }
}

// ─── notes (kind:1) ──────────────────────────────────────────────────────────

/// Notes scope: a kind-1 short text note is searchable by its `content`.
pub struct NoteSearchScope;

impl NoteSearchScope {
    const F_CONTENT: SearchField = SearchField::new(0);

    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoteSearchScope {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchScopeProvider for NoteSearchScope {
    fn spec(&self) -> SearchIndexSpec {
        SearchIndexSpec {
            scope: SearchScopeId::from_label(SCOPE_LABEL_NOTES),
            source: "nip50.notes (kind:1 content)",
            kinds: BTreeSet::from([KIND_NOTE]),
            fields: vec![Self::F_CONTENT],
            privacy: SearchPrivacyPolicy::PublicIndexable,
            cache_mode: CacheSearchMode::Both,
        }
    }

    fn extract(&self, event: &StoredEvent) -> Vec<(SearchField, String)> {
        if event.raw.content.is_empty() {
            return Vec::new();
        }
        vec![(Self::F_CONTENT, event.raw.content.clone())]
    }
}

// ─── long-form (kind:30023) ──────────────────────────────────────────────────

/// Long-form scope: a NIP-23 article is searchable by its `title` and
/// `summary` tags plus a bounded prefix of its body content.
pub struct LongFormSearchScope;

impl LongFormSearchScope {
    const F_TITLE: SearchField = SearchField::with_weight(0, 3);
    const F_SUMMARY: SearchField = SearchField::with_weight(1, 2);
    const F_BODY: SearchField = SearchField::new(2);

    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for LongFormSearchScope {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchScopeProvider for LongFormSearchScope {
    fn spec(&self) -> SearchIndexSpec {
        SearchIndexSpec {
            scope: SearchScopeId::from_label(SCOPE_LABEL_LONGFORM),
            source: "nip50.longform (kind:30023 title/summary/body)",
            kinds: BTreeSet::from([KIND_LONG_FORM]),
            fields: vec![Self::F_TITLE, Self::F_SUMMARY, Self::F_BODY],
            privacy: SearchPrivacyPolicy::PublicIndexable,
            cache_mode: CacheSearchMode::Both,
        }
    }

    fn extract(&self, event: &StoredEvent) -> Vec<(SearchField, String)> {
        let mut out = Vec::new();
        if let Some(title) = first_tag_value(&event.raw.tags, "title") {
            out.push((Self::F_TITLE, title));
        }
        if let Some(summary) = first_tag_value(&event.raw.tags, "summary") {
            out.push((Self::F_SUMMARY, summary));
        }
        let body = &event.raw.content;
        if !body.is_empty() {
            out.push((Self::F_BODY, bounded_prefix(body, LONGFORM_BODY_PREFIX_BYTES)));
        }
        out
    }
}

/// First value of the first tag whose tag-name equals `name` (`["name", value, ..]`).
fn first_tag_value(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter()
        .find(|t| t.first().is_some_and(|n| n == name))
        .and_then(|t| t.get(1))
        .filter(|v| !v.is_empty())
        .cloned()
}

/// Return at most `max_bytes` of `s`, truncated on a char boundary.
fn bounded_prefix(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

// ─── registration ────────────────────────────────────────────────────────────

/// Register the three NIP-50 public search scopes (profiles, notes, long-form)
/// against the host. Narrow registration surface (D6): a protocol crate takes
/// `&impl SearchScopeRegistrar`, never the whole `AppHost`. Called from the
/// composition root.
pub fn register_search_scopes(host: &impl SearchScopeRegistrar) {
    host.register_search_scope(Arc::new(ProfileSearchScope::new()));
    host.register_search_scope(Arc::new(NoteSearchScope::new()));
    host.register_search_scope(Arc::new(LongFormSearchScope::new()));
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
