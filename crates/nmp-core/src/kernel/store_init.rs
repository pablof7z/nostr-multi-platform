//! Kernel construction helpers — event-store and publish-store initialisation.
//!
//! Split from `kernel/mod.rs` to keep it under the file-size gate.

use std::sync::Arc;

use crate::store::{EventStore, MemEventStore};
use crate::substrate::RelayAuthorScoreStore;

pub(super) struct EventStoreBundle {
    pub(super) store: Arc<dyn EventStore>,
    pub(super) relay_score_store: Option<Box<dyn RelayAuthorScoreStore>>,
}

/// Construct the kernel's `EventStore`.
///
/// Default: `MemEventStore` — used by all tests and the pre-M15 web target.
///
/// When compiled with `--features lmdb-backend`, an `LmdbEventStore` is
/// opened when a persistent path is available. The path is resolved in
/// priority order:
///
/// 1. `storage_path` — the FFI-supplied path threaded through from
///    `nmp_app_set_storage_path` (production iOS / Android). When the host
///    sets it before `nmp_app_start`, this is the path used.
/// 2. `NMP_LMDB_PATH` environment variable — the pre-existing opt-in
///    mechanism, kept for tests and tools that drive the kernel without
///    the FFI surface.
///
/// When neither is present (the common case for the in-process test
/// suites) the in-memory store is used. When a path is present but the
/// store cannot be opened, the function still falls back to the in-memory
/// store (so the app runs) but returns a non-`None` failure reason — the
/// caller stores this on `Kernel::store_open_failure` and surfaces it
/// through the normal snapshot channel (D6: no stderr writes).
///
/// Return value: `(bundle, open_failure)`.
/// - `open_failure` is `Some(reason)` ONLY when a path was resolved but
///   the open failed — it is `None` for the legitimate no-path/no-feature
///   in-memory default so the host can distinguish "degraded" from "normal".
pub(super) fn build_event_store(storage_path: Option<&str>) -> (EventStoreBundle, Option<String>) {
    #[cfg(feature = "lmdb-backend")]
    {
        // Priority 1: FFI-supplied path. Priority 2: env-var fallback.
        let resolved: Option<String> = storage_path
            .map(str::to_owned)
            .or_else(|| std::env::var("NMP_LMDB_PATH").ok());
        if let Some(path) = resolved {
            match crate::store::LmdbEventStore::open(std::path::Path::new(&path)) {
                Ok(s) => {
                    let relay_score_store =
                        crate::substrate::LmdbRelayAuthorScoreStore::from_event_store(s.clone());
                    return (
                        EventStoreBundle {
                            store: Arc::new(s),
                            relay_score_store: Some(Box::new(relay_score_store)),
                        },
                        None,
                    );
                }
                Err(e) => {
                    // V-67: path was supplied but open failed — fall back to
                    // in-memory and surface the reason as a diagnostic. D6: no
                    // stderr write; the caller stores this on
                    // `Kernel::store_open_failure` and emits it through the
                    // snapshot channel.
                    return (
                        EventStoreBundle {
                            store: Arc::new(MemEventStore::new()),
                            relay_score_store: None,
                        },
                        Some(e.to_string()),
                    );
                }
            }
        }
    }
    // `storage_path` is unused when the `lmdb-backend` feature is off.
    #[cfg(not(feature = "lmdb-backend"))]
    let _ = storage_path;
    // No path or feature — in-memory is the legitimate default; no failure.
    (
        EventStoreBundle {
            store: Arc::new(MemEventStore::new()),
            relay_score_store: None,
        },
        None,
    )
}

/// Choose the [`PublishStore`](crate::publish::PublishStore) backing the
/// publish engine.
///
/// Publish intents composed offline only survive an app kill if the store is
/// durable - `PublishEngine::resume_from_store` replays exactly what
/// `load_pending` returns at startup. There are three backends:
///
/// 1. [`FsPublishStore`](crate::publish::FsPublishStore) - JSON files under
///    `{storage_path}/publish_intents/`. Durable **without** any feature flag,
///    so it is the chosen backend whenever the host supplied a storage path.
/// 2. [`DomainPublishStore`](crate::publish::DomainPublishStore) - LMDB-backed
///    via the shared `EventStore`. Durable *only* with `--features
///    lmdb-backend`; without it the underlying store is `MemEventStore` and
///    intents are lost on restart. Kept as the fallback when no storage path
///    is set but the event store still opened cleanly.
/// 3. [`InMemoryPublishStore`](crate::publish::InMemoryPublishStore) - last
///    resort (and the steady state for CI / in-process tests, which pass no
///    storage path).
///
/// Resolution mirrors [`build_event_store`]: the FFI-supplied `storage_path`
/// wins, then the `NMP_LMDB_PATH` env-var fallback. When a path resolves, the
/// `FsPublishStore` is rooted at the *same* directory as the LMDB event store
/// so one `storage_path` covers all durable kernel state.
pub(super) fn resolve_publish_store(
    storage_path: Option<&str>,
    event_store: &Arc<dyn EventStore>,
) -> Arc<dyn crate::publish::PublishStore> {
    let resolved = resolve_storage_path(storage_path);
    if let Some(path) = resolved {
        // Durable, feature-flag-independent: offline intents survive restart.
        return Arc::new(crate::publish::FsPublishStore::new(path));
    }
    // No storage path: fall back to the LMDB-domain store (durable only under
    // `lmdb-backend`), then the in-memory store. This keeps CI/test behaviour
    // (no storage path -> no on-disk artefacts) unchanged.
    crate::publish::DomainPublishStore::open(Arc::clone(event_store)).map_or_else(
        |_| {
            Arc::new(crate::publish::InMemoryPublishStore::new())
                as Arc<dyn crate::publish::PublishStore>
        },
        |store| Arc::new(store) as Arc<dyn crate::publish::PublishStore>,
    )
}

pub(super) fn resolve_storage_path(storage_path: Option<&str>) -> Option<String> {
    storage_path
        .map(str::to_owned)
        .or_else(|| std::env::var("NMP_LMDB_PATH").ok())
}
