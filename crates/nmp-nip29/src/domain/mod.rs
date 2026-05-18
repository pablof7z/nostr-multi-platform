//! 13 `DomainModule` impls per `docs/design/nip29-crate.md` §3.1.
//!
//! Each module owns a kind range (or h-tagged variant of a kind) and is the
//! truth source for its `DomainRecord` shape. Views project off these records.
//!
//! ## The unifying ownership rule (kinds.md §4)
//!
//! Any event with an `["h", group_id]` tag is owned by `nmp-nip29`, regardless
//! of kind. The kind is the dispatch; the `h` tag is the ownership.
//!
//! ## Module split (one file per logical cluster)
//!
//! - `metadata` — Group, GroupAdmins, GroupMembers, GroupRoles (relay-signed,
//!   39000-39003; parameterized-replaceable on `d` tag).
//! - `content` — GroupChatMessage, GroupDiscussion, GroupArtifact,
//!   GroupRepost, GroupHighlight (user-sent content events).
//! - `crosskind` — GroupReaction, GroupComment (h-tagged variants whose
//!   non-`h` form lives in `nmp-nip25` / `nmp-nip22`).
//! - `moderation` — GroupModerationEvent (audit trail, 9000-9009 / 9021 / 9022).
//! - `context` — GroupContextEvent (generic fallback for unknown h-tagged
//!   kinds; future extensibility per kinds.md §2.1).
//!
//! ## Records (data shapes)
//!
//! Domain records live alongside their modules. Each one carries a `GroupId`
//! composite-key prefix so the kernel's reverse index can join across them at
//! the application layer without any protocol-crate awareness.

mod content;
mod context;
mod crosskind;
mod metadata;
mod moderation;
pub(crate) mod records;

pub use content::{
    GroupArtifactModule, GroupChatMessageModule, GroupDiscussionModule, GroupHighlightModule,
    GroupRepostModule,
};
pub use context::GroupContextEventModule;
pub use crosskind::{GroupCommentModule, GroupReactionModule};
pub use metadata::{GroupAdminsModule, GroupMembersModule, GroupModule, GroupRolesModule};
pub use moderation::GroupModerationEventModule;
pub use records::{
    GroupArtifactRecord, GroupChatMessageRecord, GroupCommentRecord, GroupContextEventRecord,
    GroupDiscussionRecord, GroupHighlightRecord, GroupMembershipSnapshot, GroupMetadataRecord,
    GroupReactionRecord, GroupRepostRecord, GroupRolesRecord, ModerationEventRecord,
    GroupVisibility, GroupAccessPolicy,
};

use nmp_core::substrate::ModuleRegistry;

/// Register all 13 `DomainModule` impls into a kernel `ModuleRegistry`.
pub fn register_all(registry: &mut ModuleRegistry) {
    registry.register_domain::<GroupModule>();
    registry.register_domain::<GroupAdminsModule>();
    registry.register_domain::<GroupMembersModule>();
    registry.register_domain::<GroupRolesModule>();
    registry.register_domain::<GroupChatMessageModule>();
    registry.register_domain::<GroupDiscussionModule>();
    registry.register_domain::<GroupArtifactModule>();
    registry.register_domain::<GroupRepostModule>();
    registry.register_domain::<GroupHighlightModule>();
    registry.register_domain::<GroupReactionModule>();
    registry.register_domain::<GroupCommentModule>();
    registry.register_domain::<GroupModerationEventModule>();
    registry.register_domain::<GroupContextEventModule>();
}
