//! User-sent group content `DomainModule` impls.
//!
//! Per `kinds.md` §2.1 + §4: all events here carry `["h", local_id]` and are
//! dispatched by kind (9 chat, 11 with `t=discussion` discussion, 11 without
//! `t=discussion` artifact, 16 with h repost, 9802 with h highlight).
//!
//! Routing per `routing.md` §3: host-pinned via `relay_pin` on the
//! `LogicalInterest` (lattice Rule 9 + partition Case E).

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule};

macro_rules! noop_migrations {
    () => {
        fn migrations() -> Vec<DomainMigration> { Vec::new() }
        fn indexes() -> Vec<DomainIndex> { Vec::new() }
    };
}

/// Kind 9 (h-tagged) — chat message.
pub struct GroupChatMessageModule;
impl DomainModule for GroupChatMessageModule {
    const NAMESPACE: &'static str = "nip29.group_chat_message";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 11 with `["t","discussion"]` — discussion root.
pub struct GroupDiscussionModule;
impl DomainModule for GroupDiscussionModule {
    const NAMESPACE: &'static str = "nip29.group_discussion";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 11 without `t=discussion` — artifact share (URL / ISBN / NIP-23 ref).
pub struct GroupArtifactModule;
impl DomainModule for GroupArtifactModule {
    const NAMESPACE: &'static str = "nip29.group_artifact";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 16 with h tag — generic repost into the group.
pub struct GroupRepostModule;
impl DomainModule for GroupRepostModule {
    const NAMESPACE: &'static str = "nip29.group_repost";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 9802 with h tag — NIP-84 highlight published directly into a group.
pub struct GroupHighlightModule;
impl DomainModule for GroupHighlightModule {
    const NAMESPACE: &'static str = "nip29.group_highlight";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}
