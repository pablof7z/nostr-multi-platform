//! NIP-29 event kinds + `h`-tag-based dispatch.
//!
//! Per `docs/design/nip29/kinds.md` §4: **any event carrying an `["h",
//! group_id]` tag is a NIP-29 group event and lives in `nmp-nip29`, regardless
//! of its kind.** This module classifies the kind, and `group_id_from_tags`
//! pulls the `h` tag value if present.

use crate::group_id::{GroupId, RelayUrl};

// Moderation actions (9000-9009 + 9021 + 9022) — all admin-signed (9007/9021/9022 user).
pub const KIND_PUT_USER: u32 = 9000;
pub const KIND_REMOVE_USER: u32 = 9001;
pub const KIND_EDIT_METADATA: u32 = 9002;
pub const KIND_DELETE_EVENT: u32 = 9005;
pub const KIND_CREATE_GROUP: u32 = 9007;
pub const KIND_DELETE_GROUP: u32 = 9008;
pub const KIND_CREATE_INVITE: u32 = 9009;
pub const KIND_JOIN_REQUEST: u32 = 9021;
pub const KIND_LEAVE_REQUEST: u32 = 9022;

// Relay-signed metadata (parameterized-replaceable by `d` tag).
pub const KIND_GROUP_METADATA: u32 = 39000;
pub const KIND_GROUP_ADMINS: u32 = 39001;
pub const KIND_GROUP_MEMBERS: u32 = 39002;
pub const KIND_GROUP_ROLES: u32 = 39003;

/// Coarse-grained classification of a kind for ingest dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KindClass {
    /// Relay-signed metadata (39000–39003) — parameterized-replaceable on `d`.
    Metadata,
    /// Admin-signed moderation action (9000–9009).
    Moderation,
    /// User-signed user-management request (9021 / 9022).
    UserManagement,
    /// An `h`-tagged event whose kind is NOT in the 9xxx/3900x namespace —
    /// NIP-29 routes it but does not classify or own its kind. Every non-NIP-29
    /// kind that carries an `h` tag (chat kind:9, NIP-25 kind:7 reactions,
    /// NIP-18 kind:16 reposts, NIP-84 kind:9802 highlights, future kinds, …)
    /// lands here; the owning NIP — never NIP-29 — defines what the kind means.
    GroupEvent,
    /// Not a NIP-29 event at all.
    NotGroup,
}

/// Classify a (kind, `has_h_tag`) pair. The `h` tag is the routing key and the
/// ownership discriminator (kinds.md §4); the kind is the dispatch.
#[must_use] 
pub fn classify(kind: u32, has_h_tag: bool) -> KindClass {
    match kind {
        KIND_GROUP_METADATA | KIND_GROUP_ADMINS | KIND_GROUP_MEMBERS | KIND_GROUP_ROLES => {
            // Metadata uses `d` for replacement keying, but the routing is still
            // "host relay only"; classified regardless of `h` tag presence.
            KindClass::Metadata
        }
        KIND_PUT_USER | KIND_REMOVE_USER | KIND_EDIT_METADATA | KIND_DELETE_EVENT
        | KIND_CREATE_GROUP | KIND_DELETE_GROUP | KIND_CREATE_INVITE
            if has_h_tag =>
        {
            KindClass::Moderation
        }
        KIND_JOIN_REQUEST | KIND_LEAVE_REQUEST if has_h_tag => KindClass::UserManagement,
        // Any other kind with an `h` tag is just a routed group event; NIP-29
        // does not classify or own its kind (kinds.md §4).
        _ if has_h_tag => KindClass::GroupEvent,
        _ => KindClass::NotGroup,
    }
}

/// Convenience: is this an h-tagged group event of any class?
#[must_use] 
pub fn event_is_group_event(kind: u32, tags: &[Vec<String>]) -> bool {
    let has_h = tags.iter().any(|t| t.len() >= 2 && t[0] == "h");
    !matches!(classify(kind, has_h), KindClass::NotGroup)
}

/// Pull the `h` tag value (the `local_id`) from an event's tags. Returns
/// `None` if no `h` tag exists.
#[must_use] 
pub fn h_tag_value(tags: &[Vec<String>]) -> Option<&str> {
    tags.iter()
        .find(|t| t.len() >= 2 && t[0] == "h")
        .map(|t| t[1].as_str())
}

/// NIP-29 subgroups tag helpers (nips PR #2319). The `parent` and `child`
/// tag names live on `kind:39000` (group metadata); these accessors are
/// shared by every read-side projection that folds 39000 so the
/// parent/children extraction has one canonical implementation.
pub mod tags {
    /// Pull the `["parent", <id>]` tag value from `tags`. Per the spec a
    /// 39000 carries at most one `parent` tag; the first wins. An empty
    /// value (`["parent"]` or `["parent", ""]`) normalises to `None`
    /// (absent == root group).
    #[must_use]
    pub fn parent_tag_value(tags: &[Vec<String>]) -> Option<&str> {
        tags.iter()
            .find(|t| t.len() >= 2 && t[0] == "parent")
            .map(|t| t[1].as_str())
            .filter(|s| !s.is_empty())
    }

    /// The ordered `["child", <id>]` tag values from `tags`, preserving tag
    /// order (the spec models the parent's child list as ordered). Returns
    /// `None` when no `child` tag is present so callers can distinguish
    /// "no children declared" from "an empty list"; both fold to an empty
    /// `Vec` for the projection row.
    #[must_use]
    pub fn child_tag_values(tags: &[Vec<String>]) -> Option<Vec<&str>> {
        let children: Vec<&str> = tags
            .iter()
            .filter(|t| t.len() >= 2 && t[0] == "child")
            .map(|t| t[1].as_str())
            .collect();
        if children.is_empty() {
            None
        } else {
            Some(children)
        }
    }
}

/// Pull the `d` tag value (parameterized-replaceable key for 39000–39003).
#[must_use] 
pub fn d_tag_value(tags: &[Vec<String>]) -> Option<&str> {
    tags.iter()
        .find(|t| t.len() >= 2 && t[0] == "d")
        .map(|t| t[1].as_str())
}

/// Combine an event's `h` tag (or `d` tag for metadata kinds) with a known
/// host relay URL into a typed `GroupId`. Returns `None` if neither tag is
/// present.
///
/// `host_relay_url` MUST be the provenance relay — the relay that produced the
/// event in our subscription stream. NIP-29 group identity is the pair
/// `(host, local_id)` (`group_id.rs`); the relay is the trust anchor.
#[must_use] 
pub fn group_id_from_tags(host_relay_url: &RelayUrl, tags: &[Vec<String>]) -> Option<GroupId> {
    let local = h_tag_value(tags).or_else(|| d_tag_value(tags))?;
    Some(GroupId::new(host_relay_url.clone(), local.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_metadata_independent_of_h() {
        assert_eq!(classify(KIND_GROUP_METADATA, false), KindClass::Metadata);
        assert_eq!(classify(KIND_GROUP_ADMINS, true), KindClass::Metadata);
    }

    #[test]
    fn classify_moderation_only_with_h() {
        assert_eq!(classify(KIND_PUT_USER, true), KindClass::Moderation);
        // No h tag means not a group event for moderation kinds either.
        assert_eq!(classify(KIND_PUT_USER, false), KindClass::NotGroup);
    }

    #[test]
    fn classify_chat_with_h_is_group_event() {
        // kind:9 is chat — owned by the caller, not NIP-29. With an `h` tag it
        // is just a routed group event; without one it is not a group event.
        // Use the literal kind (NIP-29 owns no constant for foreign kinds).
        assert_eq!(classify(9, true), KindClass::GroupEvent);
        assert_eq!(classify(9, false), KindClass::NotGroup);
    }

    #[test]
    fn classify_unknown_h_tagged_is_fallback() {
        // A future poll kind with an h tag — routed as a generic group event.
        assert_eq!(classify(40000, true), KindClass::GroupEvent);
    }

    #[test]
    fn group_id_from_tags_uses_h_then_d() {
        let host = "wss://groups.example.com".to_string();
        let tags_h = vec![vec!["h".into(), "room-1".into()]];
        let g = group_id_from_tags(&host, &tags_h).unwrap();
        assert_eq!(g.local_id, "room-1");
        // Metadata events carry d, not h.
        let tags_d = vec![vec!["d".into(), "room-2".into()]];
        let g = group_id_from_tags(&host, &tags_d).unwrap();
        assert_eq!(g.local_id, "room-2");
    }
}
