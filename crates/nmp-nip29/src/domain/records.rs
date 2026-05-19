//! `DomainRecord` shapes: persistent data the `DomainModule`s own.
//!
//! Each record carries a `GroupId` composite-key prefix
//! `(host_relay_url, local_id)` so the kernel's reverse index can compose
//! cross-protocol joins at the app layer without protocol-crate awareness
//! (`nip29-crate.md` §6).

use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;

/// Per `kinds.md` §2.4: 39000 carries `public`/`private` and `open`/`closed`
/// markers; absence defaults to public/open/visible (Highlighter's behavior).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum GroupVisibility {
    #[default]
    Public,
    Private,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum GroupAccessPolicy {
    #[default]
    Open,
    Closed,
}

/// Kind 39000 — group metadata snapshot. Replaceable by `d = local_id`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupMetadataRecord {
    pub group: GroupId,
    pub event_id: String,
    pub signer_pubkey: String,
    pub created_at: u64,
    pub name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub visibility: GroupVisibility,
    pub access: GroupAccessPolicy,
    pub restricted: bool,
    pub hidden: bool,
}

/// Kind 39001 / 39002 — relay-signed pubkey set. Same shape for both because
/// admins and members are projected identically; the kind tells us which.
///
/// `entries` preserves the per-`p`-tag tuple `(pubkey, role?, description?)`
/// so role-aware projection is opt-in for views that need it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupMembershipSnapshot {
    pub group: GroupId,
    pub event_id: String,
    pub signer_pubkey: String,
    pub created_at: u64,
    pub kind: u32, // 39001 or 39002
    pub entries: Vec<MemberEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MemberEntry {
    pub pubkey: String,
    pub role: Option<String>,
    pub description: Option<String>,
}

/// Kind 39003 — relay-declared role catalog. Optional per the NIP; many
/// relays do not emit 39003.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupRolesRecord {
    pub group: GroupId,
    pub event_id: String,
    pub signer_pubkey: String,
    pub created_at: u64,
    pub roles: Vec<RoleDecl>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoleDecl {
    pub name: String,
    pub description: Option<String>,
}

/// Kind 9 — group chat message (h-tagged).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupChatMessageRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub content: String,
    pub reply_to_event_id: Option<String>,
    pub root_event_id: Option<String>,
    pub previous_tag_prefixes: Vec<String>,
}

/// Kind 11 + `["t","discussion"]` — group discussion root.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupDiscussionRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub title: Option<String>,
    pub body: String,
    pub image_urls: Vec<String>,
}

/// Kind 11 (no `t=discussion`) — artifact share (article / podcast / book /
/// long-form ref); the `d` tag is the artifact stable id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupArtifactRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub artifact_id: String,
    pub url_reference: Option<String>, // r tag
    pub isbn_reference: Option<String>, // i tag
    pub naddr_reference: Option<String>, // a tag
    pub title: Option<String>,
    pub note: String,
}

/// Kind 16 with h tag — generic repost into a group. The reposted event id is
/// in the `e` tag.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupRepostRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub reposted_event_id: String,
    pub reposted_kind: Option<u32>,
}

/// Kind 9802 with h tag — NIP-84 highlight published directly into a room.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupHighlightRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub content: String,
    pub source_url: Option<String>,
}

/// Kind 7 with h tag — reaction inside a group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupReactionRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub target_event_id: String,
    pub content: String, // "+", emoji, etc.
}

/// Kind 1111 with h tag — NIP-22 comment inside a group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupCommentRecord {
    pub group: GroupId,
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub root_event_id: Option<String>,
    pub parent_event_id: Option<String>,
    pub content: String,
}

/// Kinds 9000–9009, 9021, 9022 — audit-only moderation record.
///
/// `moderation.md` §5: **never** mutates `GroupAdmins`/`GroupMembers`; those
/// flip only on the relay's republished 39001/39002 snapshot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModerationEventRecord {
    pub group: GroupId,
    pub event_id: String,
    pub kind: u32,
    pub actor_pubkey: String,
    pub target_pubkey: Option<String>,
    pub target_event_id: Option<String>,
    pub reason: Option<String>,
    pub created_at: u64,
    pub raw_tags: Vec<Vec<String>>,
}

/// Generic fallback record for unknown h-tagged kinds — preserves the event
/// so future-aware apps can layer their own DomainModule without losing data
/// (`kinds.md` §2.1 "Future / extensibility").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupContextEventRecord {
    pub group: GroupId,
    pub event_id: String,
    pub kind: u32,
    pub author: String,
    pub created_at: u64,
    pub content: String,
    pub raw_tags: Vec<Vec<String>>,
}
