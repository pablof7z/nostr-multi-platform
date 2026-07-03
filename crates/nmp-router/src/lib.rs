//! `nmp-router` — Layer-2 routing (`docs/architecture/crate-boundaries.md` §4).
//!
//! Seven responsibilities:
//!
//! 1. [`InMemoryMailboxCache`] — the NIP-65 / kind:10002 cache the substrate
//!    [`nmp_core::substrate::MailboxCache`] trait points at. The kind:10002
//!    ingest parser is its single writer; the router and the planner are
//!    the only readers.
//! 2. [`Kind10002Parser`] — an [`nmp_core::substrate::IngestParser`] that
//!    delegates tag decoding to `nmp-nip65-types` and upserts into the cache.
//! 3. [`GenericOutboxRouter`] — the single generic
//!    [`nmp_core::substrate::OutboxRouter`] impl. Ships the generic
//!    algorithm (NIP-65 write (outbox) sets, hints, provenance,
//!    user-configured relays, indexers, and AppRelay fallback).
//! 4. [`publish_relay_list::PublishRelayListAction`] — the
//!    `nmp.nip65.publish_relay_list` action module. Routing owns kind:10002
//!    end-to-end: ingest (parser → cache), routing (router reads cache),
//!    publish (action builds the event).
//! 5. [`Nip65OutboxResolver`] — the publish-side concrete
//!    [`nmp_core::publish::OutboxResolver`] impl that reads kind:10002 from
//!    the shared [`MailboxCache`](nmp_core::substrate::MailboxCache)
//!    (crate-boundary spec §271; moved out of
//!    `nmp-core::publish::nip65` so the substrate stays NIP-neutral per D0).
//!    Production composition installs it via
//!    `AppHost::set_publish_resolver_factory` →
//!    `Kernel::set_publish_resolver`; the kernel default is
//!    `nmp_core::publish::NoopOutboxResolver` so a kernel without the
//!    router-side resolver is still a clean no-op (fail-closed).
//! 6. [`IndexerRepublishPolicy`] — the pure policy object for forwarding
//!    accepted replaceable events to indexer relays. `nmp-core` owns the
//!    generic raw-event observer and pool send; this crate owns the
//!    replaceable-kind, provenance, source-skip, and bounded-dedup rules.
//! 7. [`RelayAdmissionPolicy`] / [`PrivateNetworkPolicy`] — structural URL
//!    guard applied to untrusted lanes (1–3) before a relay URL is ever
//!    used. Rejects loopback, RFC-1918, link-local, and unspecified
//!    addresses. Composable via [`GenericOutboxRouter::with_admission_policy`].
//!
//! Step 3 cuts the kernel over to `Arc<dyn OutboxRouter>` injection,
//! deletes `nmp_core::kernel::outbox`, and replaces the old kernel-owned
//! relay-list map with reads through the substrate
//! [`InMemoryMailboxCache`] held as `Arc<dyn MailboxCache>`.

mod cache;
mod discovery;
mod indexer_republish;
mod ingest;
mod nip65_resolver;
mod relay_admission;
mod router;

pub mod publish_relay_list;
pub mod wire;

pub use cache::InMemoryMailboxCache;
pub use indexer_republish::{IndexerRepublishPolicy, IndexerRepublishPolicyHandle};
pub use ingest::Kind10002Parser;
pub use nip65_resolver::{
    is_discovery_kind, Nip65OutboxResolver, RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD,
};
pub use publish_relay_list::{
    build_relay_list_event, register_actions, Nip65RelayListPublishSupport, PublishRelayListAction,
    PublishRelayListInput, RelayListEntry, RelayMarker,
};
pub use relay_admission::{PrivateNetworkPolicy, RelayAdmissionPolicy};
pub use router::GenericOutboxRouter;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
