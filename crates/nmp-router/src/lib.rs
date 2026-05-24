//! `nmp-router` — Layer-2 routing (`docs/architecture/crate-boundaries.md` §3).
//!
//! Three responsibilities:
//!
//! 1. [`InMemoryMailboxCache`] — the NIP-65 / kind:10002 cache the substrate
//!    [`nmp_core::substrate::MailboxCache`] trait points at. The kind:10002
//!    ingest parser is its single writer; the router and the planner are
//!    the only readers.
//! 2. [`Kind10002Parser`] — an [`nmp_core::substrate::IngestParser`] that
//!    decodes kind:10002 tags and upserts into the cache.
//! 3. [`GenericOutboxRouter`] — the single generic
//!    [`nmp_core::substrate::OutboxRouter`] impl. Step 2 ships the
//!    `explicit_targets` override path (fully correct) and a minimal generic
//!    algorithm (NIP-65 write set on publish, NIP-65 read set on subscribe,
//!    AppRelay fallback). Lanes 2/3/4/5/6 (hints, provenance,
//!    user-configured, class-routed, indexer) ship as `// TODO §3.1 lane X`
//!    insertion points consumed by follow-on PRs in step 3.
//!
//! Step 2 does NOT cut the kernel over to this router. Step 3 (the next PR
//! in the migration ladder) does, by swapping the hardwired
//! `nmp_core::kernel::outbox` path for `Arc<dyn OutboxRouter>` injection.
//!
//! `nmp-nip65`'s `PublishRelayListAction` is absorbed here at step 3 too —
//! deleting `nmp-nip65` while step 2's parallel infrastructure is still
//! unused would leave a window where neither path is wired up.

mod cache;
mod ingest;
mod router;

pub use cache::InMemoryMailboxCache;
pub use ingest::Kind10002Parser;
pub use router::GenericOutboxRouter;
