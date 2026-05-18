//! `DomainModule` trait implementations for the podcast domain.
//!
//! Two modules cover the two primary record types:
//!
//! * [`PodcastsModule`] — namespace `"podcast.podcasts"`, schema v1.
//! * [`EpisodesModule`] — namespace `"podcast.episodes"`, schema v1.
//!
//! Both modules are **pure-local** (no Nostr ingest — `ingest_kinds()` returns
//! `&[]`). Records are written by the action layer directly via `DomainHandle`.
//!
//! Registering with `ModuleRegistry` records the namespace + type-name strings
//! in the kernel's descriptor table. No runtime dispatch is wired yet — that
//! is the substrate's Phase 2 story.
//!
//! D0: no podcast nouns in `nmp-core`; this file lives under
//! `apps/podcast/podcast-core`.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use super::records::{EpisodeRecord, PodcastRecord};

// ─── PodcastsModule ───────────────────────────────────────────────────────────

/// Domain module for [`PodcastRecord`] — one row per subscribed podcast.
///
/// Namespace: `"podcast.podcasts"`.
/// Storage key: ULID bytes of `PodcastRecord::id` (16 bytes, big-endian).
/// Schema version 1 — no migrations yet.
pub struct PodcastsModule;

impl DomainModule for PodcastsModule {
    const NAMESPACE: &'static str = "podcast.podcasts";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> {
        vec![] // v0 → v1 is a clean initial install; no data to migrate.
    }

    fn indexes() -> Vec<DomainIndex> {
        vec![] // Secondary indexes added in a later schema bump.
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<PodcastRecord>();
    }
}

// ─── EpisodesModule ───────────────────────────────────────────────────────────

/// Domain module for [`EpisodeRecord`] — one row per episode.
///
/// Namespace: `"podcast.episodes"`.
/// Storage key: ULID bytes of `EpisodeRecord::id` (16 bytes, big-endian).
/// Schema version 1.
pub struct EpisodesModule;

impl DomainModule for EpisodesModule {
    const NAMESPACE: &'static str = "podcast.episodes";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> {
        vec![]
    }

    fn indexes() -> Vec<DomainIndex> {
        vec![]
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<EpisodeRecord>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{ModuleFamily, ModuleRegistry};

    #[test]
    fn podcasts_module_registers_correct_namespace() {
        let mut reg = ModuleRegistry::default();
        reg.register_domain::<PodcastsModule>();
        let descriptors = reg.descriptors();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].namespace, "podcast.podcasts");
        assert_eq!(descriptors[0].family, ModuleFamily::Domain);
    }

    #[test]
    fn episodes_module_registers_correct_namespace() {
        let mut reg = ModuleRegistry::default();
        reg.register_domain::<EpisodesModule>();
        let descriptors = reg.descriptors();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].namespace, "podcast.episodes");
        assert_eq!(descriptors[0].family, ModuleFamily::Domain);
    }

    #[test]
    fn both_modules_register_without_collision() {
        let mut reg = ModuleRegistry::default();
        reg.register_domain::<PodcastsModule>();
        reg.register_domain::<EpisodesModule>();
        assert_eq!(reg.descriptors().len(), 2);
    }

    #[test]
    fn podcasts_module_schema_version_is_one() {
        assert_eq!(PodcastsModule::SCHEMA_VERSION, 1);
    }

    #[test]
    fn episodes_module_schema_version_is_one() {
        assert_eq!(EpisodesModule::SCHEMA_VERSION, 1);
    }

    #[test]
    fn podcasts_domain_registry_records_record_type() {
        let mut dr = DomainRegistry::default();
        PodcastsModule::register(&mut dr);
        assert_eq!(dr.records().len(), 1);
        assert!(
            dr.records()[0].contains("PodcastRecord"),
            "type name must contain PodcastRecord: {}",
            dr.records()[0]
        );
    }

    #[test]
    fn episodes_domain_registry_records_record_type() {
        let mut dr = DomainRegistry::default();
        EpisodesModule::register(&mut dr);
        assert_eq!(dr.records().len(), 1);
        assert!(
            dr.records()[0].contains("EpisodeRecord"),
            "type name must contain EpisodeRecord: {}",
            dr.records()[0]
        );
    }
}
