//! `GroupModerationEventModule` — audit-only DomainModule for 9000-9009 +
//! 9021 + 9022.
//!
//! **Audit-only**: per `moderation.md` §5, this module never mutates
//! `GroupAdmins`/`GroupMembers`. Those flip only when the relay republishes
//! 39001/39002 in response to a moderation action. The audit record exists
//! so admins / members / developers can inspect the moderation log later.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use super::records::ModerationEventRecord;

pub struct GroupModerationEventModule;
impl DomainModule for GroupModerationEventModule {
    const NAMESPACE: &'static str = "nip29.group_moderation_event";
    const SCHEMA_VERSION: u32 = 1;
    fn migrations() -> Vec<DomainMigration> { Vec::new() }
    fn indexes() -> Vec<DomainIndex> { Vec::new() }
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<ModerationEventRecord>();
    }
}
