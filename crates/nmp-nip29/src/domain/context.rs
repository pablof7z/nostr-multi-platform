//! `GroupContextEventModule` — generic fallback DomainModule for unknown
//! h-tagged kinds.
//!
//! Per `kinds.md` §2.1 "Future / extensibility": NIP-29 allows any kind with
//! an `h` tag to be a group event. Unknown kinds (livestreams, polls, files,
//! future NIPs) survive in this generic record so apps that ship custom group
//! kinds can layer their own DomainModules without modifying `nmp-nip29`.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use super::records::GroupContextEventRecord;

pub struct GroupContextEventModule;
impl DomainModule for GroupContextEventModule {
    const NAMESPACE: &'static str = "nip29.group_context_event";
    const SCHEMA_VERSION: u32 = 1;
    fn migrations() -> Vec<DomainMigration> { Vec::new() }
    fn indexes() -> Vec<DomainIndex> { Vec::new() }
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<GroupContextEventRecord>();
    }
}
