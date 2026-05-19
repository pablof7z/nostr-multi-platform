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
//! D0: no podcast nouns in `nmp-core`; this file lives under
//! `apps/podcast/podcast-core`.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule};

// ─── PodcastsModule ───────────────────────────────────────────────────────────

/// Domain module for `PodcastRecord` — one row per subscribed podcast.
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
}

// ─── EpisodesModule ───────────────────────────────────────────────────────────

/// Domain module for `EpisodeRecord` — one row per episode.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podcasts_module_namespace_is_stable() {
        assert_eq!(PodcastsModule::NAMESPACE, "podcast.podcasts");
    }

    #[test]
    fn episodes_module_namespace_is_stable() {
        assert_eq!(EpisodesModule::NAMESPACE, "podcast.episodes");
    }

    #[test]
    fn podcasts_module_schema_version_is_one() {
        assert_eq!(PodcastsModule::SCHEMA_VERSION, 1);
    }

    #[test]
    fn episodes_module_schema_version_is_one() {
        assert_eq!(EpisodesModule::SCHEMA_VERSION, 1);
    }
}
