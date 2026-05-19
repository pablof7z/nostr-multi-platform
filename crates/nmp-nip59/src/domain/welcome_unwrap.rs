//! `WelcomeUnwrapModule` — DomainModule that ingests kind:1059 (GiftWrap)
//! events from the relay stream and routes the unwrapped MLS Welcome messages
//! to MDK for processing.
//!
//! ## Ingest contract
//!
//! `ingest_kinds() = &[1059]` declares ownership of all NIP-59 gift-wrap
//! events. The kernel dispatch table routes every kind:1059 event here first;
//! this module is the sole consumer of that kind in the NMP kernel.
//!
//! ## Seam documentation
//!
//! The actual NIP-44 decryption (calling `unwrap_gift_wrap`) requires the
//! receiver's `Keys`. The `DomainModule` interface exposes only
//! `migrations()` and `indexes()` — it cannot perform per-event
//! side-effects inline. The kernel's future event-ingest pipeline
//! will provide a `process_event(event: &Event, ctx: &IngestContext)` hook;
//! for this milestone `WelcomeUnwrapModule` declares its ingest kind and
//! the record shape that MDK processing will materialise, but the actual
//! decryption + MDK dispatch is performed by the actor layer.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule};
use serde::{Deserialize, Serialize};

/// A record materialised when an incoming kind:1059 gift-wrap is unwrapped
/// and the inner rumor is confirmed to be an MLS Welcome payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WelcomeRecord {
    /// The event ID of the outer kind:1059 gift-wrap event.
    pub gift_wrap_id: String,
    /// The sender's public key (from the verified seal).
    pub sender_pubkey: String,
    /// The inner rumor serialised as JSON, for MDK processing.
    pub rumor_json: String,
    /// Unix timestamp of when this record was created (local wall clock).
    pub ingested_at: u64,
}

/// DomainModule that ingests kind:1059 gift-wrap events.
///
/// Declares `ingest_kinds = &[1059]` so the kernel dispatch table routes
/// gift-wrap events here. The actor layer calls [`crate::unwrap_gift_wrap`]
/// on each event and materialises a [`WelcomeRecord`] when the rumor is an
/// MLS Welcome.
pub struct WelcomeUnwrapModule;

impl DomainModule for WelcomeUnwrapModule {
    const NAMESPACE: &'static str = "nip59.welcome_unwrap";
    const SCHEMA_VERSION: u32 = 1;

    /// Owns kind:1059 — NIP-59 GiftWrap.
    fn ingest_kinds() -> &'static [u32] {
        &[1059]
    }

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        Vec::new()
    }
}
