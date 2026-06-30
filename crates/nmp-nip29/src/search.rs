//! Crate-owned full-text search scope over NIP-29 group metadata (#1811).
//!
//! This proves the FTS scope registry is **protocol-crate-owned**, not a
//! NIP-50 privilege: NIP-29's reference relay (`nip29.f7z.io`) speaks no
//! NIP-50, so this scope is [`CacheSearchMode::CacheOnly`] — it indexes the
//! locally cached kind:39000 group-metadata events and answers searches from
//! the local FTS index, never fanning out to a relay.
//!
//! The scope indexes the public group-metadata fields carried as tags on
//! kind:39000 (`docs/design/nip29/kinds.md`, mirrored by
//! [`crate::projection::discovered`]):
//! - `["name", text]`   — the group's display name (highest weight),
//! - `["about", text]`  — the group's description,
//! - `["d", local_id]`  — the parameterized-replaceable group id (low weight,
//!   so a user can find a group by its slug as well as its prose).
//!
//! Group metadata is public, so the scope is
//! [`SearchPrivacyPolicy::PublicIndexable`]. The scope lives **entirely** in
//! `nmp-nip29`: `nmp-core` holds zero group nouns. A host wires it with the
//! one-liner [`register_search_scopes`].

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{
    CacheSearchMode, SearchIndexSpec, SearchPrivacyPolicy, SearchScopeProvider,
    SearchScopeRegistrar,
};
use nmp_store::{SearchField, SearchScopeId, StoredEvent};

use crate::kinds::KIND_GROUP_METADATA;

/// Stable label for the NIP-29 group-metadata search scope. Construct the
/// `SearchScopeId` from this so no two crates collide on a hand-picked integer.
pub const GROUP_SEARCH_SCOPE_LABEL: &str = "nip29.groups";

/// Field id for the group display name (`["name", _]`). Highest weight — a
/// query token that hits a group's name should outrank one that only hits its
/// description.
pub const FIELD_NAME: u16 = 0;
/// Field id for the group description (`["about", _]`).
pub const FIELD_ABOUT: u16 = 1;
/// Field id for the group local id / slug (`["d", _]`). Low weight: the slug is
/// a usable search target but ranks below human-authored prose.
pub const FIELD_GROUP_ID: u16 = 2;

const WEIGHT_NAME: u16 = 4;
const WEIGHT_ABOUT: u16 = 2;
const WEIGHT_GROUP_ID: u16 = 1;

/// The NIP-29 group-metadata search scope provider.
///
/// Owns the spec (`nip29.groups`, kind:39000, public, cache-only) and the
/// extractor that pulls the searchable `(field, text)` pairs out of a stored
/// kind:39000 event's tags.
#[derive(Clone, Copy, Debug, Default)]
pub struct GroupMetadataSearchScope;

impl GroupMetadataSearchScope {
    /// Construct the scope provider.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Pull the value of the first single-valued `["<name>", value]` tag.
fn first_tag_value<'a>(tags: &'a [Vec<String>], name: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.len() >= 2 && t[0] == name)
        .map(|t| t[1].as_str())
}

impl SearchScopeProvider for GroupMetadataSearchScope {
    fn spec(&self) -> SearchIndexSpec {
        SearchIndexSpec {
            scope: SearchScopeId::from_label(GROUP_SEARCH_SCOPE_LABEL),
            source: "nmp-nip29 group metadata (kind:39000 name/about/d)",
            kinds: BTreeSet::from([KIND_GROUP_METADATA]),
            fields: vec![
                SearchField::with_weight(FIELD_NAME, WEIGHT_NAME),
                SearchField::with_weight(FIELD_ABOUT, WEIGHT_ABOUT),
                SearchField::with_weight(FIELD_GROUP_ID, WEIGHT_GROUP_ID),
            ],
            // Group metadata is public relay-signed data.
            privacy: SearchPrivacyPolicy::PublicIndexable,
            // nip29.f7z.io has no NIP-50; searches answer from the local cache.
            cache_mode: CacheSearchMode::CacheOnly,
        }
    }

    fn extract(&self, event: &StoredEvent) -> Vec<(SearchField, String)> {
        let tags = &event.raw.tags;
        let mut out = Vec::with_capacity(3);
        if let Some(name) = first_tag_value(tags, "name") {
            if !name.is_empty() {
                out.push((
                    SearchField::with_weight(FIELD_NAME, WEIGHT_NAME),
                    name.to_string(),
                ));
            }
        }
        if let Some(about) = first_tag_value(tags, "about") {
            if !about.is_empty() {
                out.push((
                    SearchField::with_weight(FIELD_ABOUT, WEIGHT_ABOUT),
                    about.to_string(),
                ));
            }
        }
        if let Some(group_id) = first_tag_value(tags, "d") {
            if !group_id.is_empty() {
                out.push((
                    SearchField::with_weight(FIELD_GROUP_ID, WEIGHT_GROUP_ID),
                    group_id.to_string(),
                ));
            }
        }
        out
    }
}

/// Register the NIP-29 search scope(s) against `host`.
///
/// Composition-root house style (ADR-0046 / ADR-0049 — no linkme/inventory): a
/// host calls this one-liner during composition to add the `nip29.groups`
/// cache-only group-metadata FTS scope. A duplicate scope id yields to the
/// existing registration (first wins).
pub fn register_search_scopes(host: &impl SearchScopeRegistrar) {
    host.register_search_scope(Arc::new(GroupMetadataSearchScope::new()));
}

#[cfg(test)]
#[path = "search/tests.rs"]
mod tests;
