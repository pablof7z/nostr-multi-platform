//! NIP-29 event kinds + `h`-tag-based dispatch.
//!
//! Per `docs/design/nip29/kinds.md` §4: **any event carrying an `["h",
//! group_id]` tag is a NIP-29 group event and lives in `nmp-nip29`, regardless
//! of its kind.** This module classifies the kind, and `group_id_from_tags`
//! pulls the `h` tag value if present.

use crate::group_id::{GroupId, RelayUrl};

/// Group chat message (kind 9) — only when `h` tag present.
pub const KIND_CHAT_MESSAGE: u32 = 9;
/// Group discussion / artifact share (kind 11) — only when `h` tag present.
pub const KIND_DISCUSSION_OR_ARTIFACT: u32 = 11;
/// Generic repost (kind 16) — group variant when `h` tag present.
pub const KIND_REPOST: u32 = 16;
/// Reaction (kind 7) — group variant when `h` tag present.
pub const KIND_REACTION: u32 = 7;
/// NIP-22 comment (kind 1111) — group variant when `h` tag present.
pub const KIND_COMMENT: u32 = 1111;
/// NIP-84 highlight (kind 9802) — group variant when `h` tag present.
pub const KIND_HIGHLIGHT: u32 = 9802;

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
    /// Known h-tagged user-sent group event (9, 11, 16, 7, 1111, 9802 if h-tagged).
    KnownGroupEvent,
    /// Unknown h-tagged kind — routed to `GroupContextEvent` fallback per
    /// `kinds.md` §2.1 "Future / extensibility".
    UnknownGroupEvent,
    /// Not a NIP-29 event at all.
    NotGroup,
}

/// Classify a (kind, has_h_tag) pair. The `h` tag is the routing key and the
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
        _ if has_h_tag => match kind {
            KIND_CHAT_MESSAGE
            | KIND_DISCUSSION_OR_ARTIFACT
            | KIND_REPOST
            | KIND_REACTION
            | KIND_COMMENT
            | KIND_HIGHLIGHT => KindClass::KnownGroupEvent,
            _ => KindClass::UnknownGroupEvent,
        },
        _ => KindClass::NotGroup,
    }
}

/// Sub-class of `KnownGroupEvent` — finer-grained dispatch for known
/// h-tagged kinds, used by the ingest router to pick the owning per-kind
/// handler.
///
/// `Kind11Discussion` vs `Kind11Artifact` requires inspecting tags for the
/// presence of `["t", "discussion"]` per `kinds.md` §2.1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupEventClass {
    Chat,
    Discussion,
    Artifact,
    Repost,
    Reaction,
    Comment,
    Highlight,
}

/// Refine a `KIND_DISCUSSION_OR_ARTIFACT` (kind 11) event by inspecting whether
/// it carries `["t","discussion"]`.
#[must_use] 
pub fn classify_kind11(tags: &[Vec<String>]) -> GroupEventClass {
    let has_discussion_marker = tags
        .iter()
        .any(|t| t.len() >= 2 && t[0] == "t" && t[1] == "discussion");
    if has_discussion_marker {
        GroupEventClass::Discussion
    } else {
        GroupEventClass::Artifact
    }
}

/// Pick the owning `GroupEventClass` for a `KnownGroupEvent`.
#[must_use] 
pub fn group_event_class(kind: u32, tags: &[Vec<String>]) -> Option<GroupEventClass> {
    match kind {
        KIND_CHAT_MESSAGE => Some(GroupEventClass::Chat),
        KIND_DISCUSSION_OR_ARTIFACT => Some(classify_kind11(tags)),
        KIND_REPOST => Some(GroupEventClass::Repost),
        KIND_REACTION => Some(GroupEventClass::Reaction),
        KIND_COMMENT => Some(GroupEventClass::Comment),
        KIND_HIGHLIGHT => Some(GroupEventClass::Highlight),
        _ => None,
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
/// `(host, local_id)` (group_id.rs); the relay is the trust anchor.
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
    fn classify_chat_with_h_is_known_group_event() {
        assert_eq!(classify(KIND_CHAT_MESSAGE, true), KindClass::KnownGroupEvent);
        assert_eq!(classify(KIND_CHAT_MESSAGE, false), KindClass::NotGroup);
    }

    #[test]
    fn classify_unknown_h_tagged_is_fallback() {
        // A future poll kind with an h tag — survives in GroupContextEvent.
        assert_eq!(classify(40000, true), KindClass::UnknownGroupEvent);
    }

    #[test]
    fn kind11_discussion_vs_artifact() {
        let with_marker = vec![vec!["t".into(), "discussion".into()]];
        assert_eq!(classify_kind11(&with_marker), GroupEventClass::Discussion);
        let without = vec![vec!["r".into(), "https://example.com".into()]];
        assert_eq!(classify_kind11(&without), GroupEventClass::Artifact);
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
