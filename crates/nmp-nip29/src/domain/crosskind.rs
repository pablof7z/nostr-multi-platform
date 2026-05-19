//! Cross-kind h-tagged variants — kinds whose non-`h` form lives in another
//! protocol crate, but whose h-tagged variant is owned here per kinds.md §4.
//!
//! These modules exist to keep protocol-crate isolation intact: `nmp-nip25`
//! never knows about groups; `nmp-nip29` never knows about public reactions.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use super::records::{GroupCommentRecord, GroupReactionRecord};

macro_rules! noop_migrations {
    () => {
        fn migrations() -> Vec<DomainMigration> { Vec::new() }
        fn indexes() -> Vec<DomainIndex> { Vec::new() }
    };
}

/// Kind 7 with h tag — h-tagged reaction.
///
/// Highlighter today does **not** emit h-tagged reactions; this module is
/// forward-looking so the protocol crate handles them cleanly when any client
/// (Highlighter post-M11.5 or other) starts emitting them. The non-`h`
/// reaction stays in `nmp-nip25`.
pub struct GroupReactionModule;
impl DomainModule for GroupReactionModule {
    const NAMESPACE: &'static str = "nip29.group_reaction";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<GroupReactionRecord>();
    }
}

/// Kind 1111 with h tag — h-tagged NIP-22 comment.
///
/// Same forward-looking rationale as `GroupReactionModule`: Highlighter's
/// `comments.rs::publish_comment` does not attach `h` today, so this module
/// is not consumed by the M11.5 Highlighter rebuild but exists for cleanliness
/// when in-room comments do attach `h`.
pub struct GroupCommentModule;
impl DomainModule for GroupCommentModule {
    const NAMESPACE: &'static str = "nip29.group_comment";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<GroupCommentRecord>();
    }
}
