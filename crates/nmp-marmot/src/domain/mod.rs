//! `DomainModule` impls per `docs/plan/marmot-mls.md` §Step 1 and the
//! MDK→NMP mapping table in `docs/research/mdk-api.md` §6.
//!
//! Four modules: `MarmotGroup`, `MarmotMessage`, `MarmotKeyPackage`,
//! `MarmotWelcome`. Each owns an NMP-native record shape (see [`records`]) —
//! the actual MLS ratchet state lives in MDK/SQLite (owned by
//! [`crate::service`]), NOT in these records. This keeps `nmp-core` free of
//! MLS types (kernel-boundary exit gate).
//!
//! Marmot event kinds (mdk-api.md §4):
//! - 30443 / 443 — KeyPackage (`MarmotKeyPackage`).
//! - 444 — Welcome rumor, wrapped in NIP-59 kind:1059 (`MarmotWelcome`).
//! - 445 — group message / commit / proposal (`MarmotGroup` + `MarmotMessage`).
//!
//! `ingest_kinds()` is intentionally left default-empty: inbound Marmot
//! events require MLS decryption via the service before they become records,
//! so the kernel does not raw-dispatch these kinds into a domain decoder
//! (unlike NIP-29's cleartext events). The records are materialised by the
//! service after `process_message` / `process_welcome`.

pub(crate) mod records;

pub use records::{
    MarmotGroupRecord, MarmotKeyPackageRecord, MarmotMessageRecord, MarmotWelcomeRecord,
};

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

macro_rules! noop_migrations {
    () => {
        fn migrations() -> Vec<DomainMigration> {
            Vec::new()
        }
        fn indexes() -> Vec<DomainIndex> {
            // Composite-key reverse indexes for cross-protocol joins live at
            // the kernel substrate level (ADR-0001); the per-module primary
            // key (group_id_hex prefix) is implicit.
            Vec::new()
        }
    };
}

/// Tracks MLS group display metadata (members, epoch, state). The
/// cryptographic state lives in MDK/SQLite — this record is projection only.
pub struct MarmotGroupModule;
impl DomainModule for MarmotGroupModule {
    const NAMESPACE: &'static str = "marmot.group";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<MarmotGroupRecord>();
    }
}

/// Decrypted message records, keyed by group + epoch + sender.
pub struct MarmotMessageModule;
impl DomainModule for MarmotMessageModule {
    const NAMESPACE: &'static str = "marmot.message";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<MarmotMessageRecord>();
    }
}

/// Tracks own and peers' published KeyPackages (as Nostr events) +
/// rotation lifecycle.
pub struct MarmotKeyPackageModule;
impl DomainModule for MarmotKeyPackageModule {
    const NAMESPACE: &'static str = "marmot.key_package";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<MarmotKeyPackageRecord>();
    }
}

/// Tracks pending inbound Welcome messages.
pub struct MarmotWelcomeModule;
impl DomainModule for MarmotWelcomeModule {
    const NAMESPACE: &'static str = "marmot.welcome";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();
    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<MarmotWelcomeRecord>();
    }
}

/// Register all 4 `DomainModule` impls into a kernel `ModuleRegistry`.
pub fn register_all(registry: &mut nmp_core::substrate::ModuleRegistry) {
    registry.register_domain::<MarmotGroupModule>();
    registry.register_domain::<MarmotMessageModule>();
    registry.register_domain::<MarmotKeyPackageModule>();
    registry.register_domain::<MarmotWelcomeModule>();
}
