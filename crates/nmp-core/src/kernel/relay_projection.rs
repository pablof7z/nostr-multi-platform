//! Typed projection slots for relay-shaped actor-owned state.
//!
//! Three relay-shaped fact caches sit behind typed slot wrappers:
//! `nmp_router::Nip65OutboxResolver::indexer_relays`,
//! `nmp_router::Nip65OutboxResolver::local_write_relays` (spec §271,
//! 2026-05-25 — resolver moved out of `nmp-core::publish::nip65` into
//! `nmp-router`; production composition wires these slots into the
//! resolver via `NmpApp::set_publish_resolver_factory`), and
//! `NmpApp::configured_relays`. All three are actor-owned (the actor thread
//! is the sole writer via `IdentityState::set_configured_relays`). The typed
//! wrappers make the slot's purpose visible at the declaration site.
//!
//! D14: every actor-owned relay-shaped cache crosses thread boundaries through
//! a **named** typed slot. The lint
//! (`crates/nmp-testing/bin/doctrine-lint/rules/d14.rs`) flags any new
//! `Arc<Mutex<Vec<...>>>` field on `NmpApp` / `Kernel` / `Actor*` structs in
//! `crates/nmp-core/src/`. The escape hatch is to introduce a typed slot
//! here (or wherever the slot's owner already lives) so the field's purpose
//! is visible at the declaration site.
//!
//! ## What lives here
//!
//! - [`RelayUrls`] — newtype around `Vec<String>` for relay URL lists
//!   (indexer set / local write set).
//! - [`AppRelayList`] — newtype around `Vec<AppRelay>` for the
//!   user-editable relay-row projection.
//! - [`IndexerRelaysSlot`] / [`LocalWriteRelaysSlot`] /
//!   [`AppRelaySlot`] — `Arc<Mutex<…>>` type aliases the resolver / FFI
//!   layer use as fields.
//! - [`new_indexer_relays_slot`] / [`new_local_write_relays_slot`] /
//!   [`new_app_relay_slot`] — constructors so call-sites never need to
//!   spell `Arc::new(Mutex::new(Default::default()))` inline.
//!
//! ## Threading
//!
//! The actor thread is the **sole writer** for every slot (D4): the
//! `IdentityState::set_configured_relays` reducer takes the kernel-owned
//! authoritative `configured_relays: Vec<AppRelay>` source-of-truth and
//! pushes derived URL lists into the shared slots. Readers (the publish
//! `Nip65OutboxResolver` on the actor thread, per-app crates via
//! `NmpApp::write_relay_urls()` on the FFI thread) take a short-lived lock
//! and clone out. No reader ever holds the lock across a `.await` or a
//! `send`.
//!
//! ## Why not a single struct?
//!
//! A unified `Arc<Mutex<RelayProjections>>` slot was considered. Rejected
//! because the three readers have non-overlapping needs (the resolver wants
//! `indexer_set` + `write_set`, FFI wants `edit_rows`) and a unified lock
//! would force any reader to contend on every relay-state write. Three
//! independent slots keep the lock-radius minimal — D8 (≤ 60 Hz emission /
//! no shared-state coupling on the hot path).

use std::sync::{Arc, Mutex};

use serde::Serialize;

use super::identity_state::AppRelay;

/// Typed wrapper around a list of relay URLs. Used for the indexer-relay set
/// and the local-write-relay set that the publish resolver reads on every
/// publish.
///
/// `Serialize` so the relay slot contents can be forwarded to the outbox
/// resolver without an intermediate `Vec<String>` copy. `#[serde(transparent)]`
/// so the wrapper is invisible on the wire — any future consumer decodes the
/// value as a plain JSON array of strings.
///
/// The tuple field is **private** (not `pub(crate)`) so even modules
/// inside `nmp-core` cannot mutate the inner `Vec` directly through `.0`.
/// All access must go through the typed accessors (`replace()` / `as_slice()`)
/// — that keeps the sole-writer (D4) and "never re-hand the inner `Vec`
/// across an `await`" invariants enforceable at the type level instead of by
/// code review.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct RelayUrls(Vec<String>);

impl RelayUrls {
    /// Construct a fresh, empty slot value.
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    /// Replace the slot contents in-place. Sole-writer helper — the actor
    /// thread is the only caller (D4).
    ///
    /// `pub` (widened from `pub(crate)` 2026-05-25, spec §271): the
    /// `nmp-router::Nip65OutboxResolver` test suite seeds the indexer /
    /// local-write slots through this accessor. Production callers in
    /// `nmp-core` remain on the actor thread (D4: still sole-writer by
    /// convention).
    pub fn replace(&mut self, urls: Vec<String>) {
        self.0 = urls;
    }

    /// Borrow the underlying list. Readers iterate this; never re-hand the
    /// inner `Vec` across an `await` boundary.
    ///
    /// `pub` (widened from `pub(crate)` 2026-05-25, spec §271): external
    /// readers in `nmp-router::Nip65OutboxResolver` (the publish-side
    /// resolver, moved out of `nmp-core::publish::nip65`) read the slot
    /// through this accessor. The mutator (`replace`) stays `pub(crate)`
    /// so the actor remains the sole writer per D4.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// Typed wrapper around the user-editable relay-row projection. Mirrors the
/// kernel's authoritative `configured_relays: Vec<AppRelay>` field; the
/// actor pushes a clone into the shared slot every time the kernel reducer
/// settles a new value, so external readers (FFI, per-app crates) observe a
/// consistent snapshot without crossing the kernel boundary.
///
/// Same private-field discipline as [`RelayUrls`] — readers must go
/// through `as_slice()` (`pub` so out-of-crate callers like
/// `apps/chirp/nmp-app-chirp/src/dm_runtime.rs` can use it), writers through
/// `replace()`. The inner `Vec<AppRelay>` is never reachable via `.0`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AppRelayList(Vec<AppRelay>);

impl AppRelayList {
    /// Construct a fresh, empty slot value.
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    /// Replace the slot contents in-place. Sole-writer helper — the actor
    /// thread is the only caller (D4).
    pub(crate) fn replace(&mut self, rows: Vec<AppRelay>) {
        self.0 = rows;
    }

    /// Borrow the underlying rows. Readers iterate; never re-hand the inner
    /// `Vec` across an `await` boundary.
    ///
    /// Marked `pub` so per-app crates that hold a `AppRelaySlot` clone
    /// (via `NmpApp::configured_relays_handle()`) can read the slot through
    /// the named slice affordance — without it the consumer would have to
    /// touch the inner `Vec` directly, which is exactly what the typed
    /// wrapper is meant to hide.
    #[must_use]
    pub fn as_slice(&self) -> &[AppRelay] {
        &self.0
    }
}

/// Shared slot for the indexer relay URL set. Cloned by the publish
/// `Nip65OutboxResolver` so it can fan discovery-kind publishes out to the
/// configured indexer relays.
///
/// `pub` (not `pub(crate)`) because `Nip65OutboxResolver::new` is part of
/// the publish module's `pub` surface and this alias appears in its
/// parameter list.
pub type IndexerRelaysSlot = Arc<Mutex<RelayUrls>>;

/// Shared slot for the local write-relay URL set. The publish resolver
/// falls back to these for the active account between the time the user
/// edits relay rows and the time the kernel:10002 round-trips from a relay.
pub type LocalWriteRelaysSlot = Arc<Mutex<RelayUrls>>;

/// Shared slot for the user-editable relay-row projection. Cloned by the
/// FFI `NmpApp` so per-app crates (e.g. `nmp-marmot`) can read the live
/// relay list without crossing FFI.
///
/// `pub` so `crate::ffi::NmpApp` can name the slot type in its field
/// declaration (the field itself is private; only the *type alias* needs
/// to be importable).
pub type AppRelaySlot = Arc<Mutex<AppRelayList>>;

/// Construct a fresh, empty [`IndexerRelaysSlot`].
#[must_use]
pub fn new_indexer_relays_slot() -> IndexerRelaysSlot {
    Arc::new(Mutex::new(RelayUrls::new()))
}

/// Construct a fresh, empty [`LocalWriteRelaysSlot`].
#[must_use]
pub fn new_local_write_relays_slot() -> LocalWriteRelaysSlot {
    Arc::new(Mutex::new(RelayUrls::new()))
}

/// Construct a fresh, empty [`AppRelaySlot`].
#[must_use]
pub fn new_app_relay_slot() -> AppRelaySlot {
    Arc::new(Mutex::new(AppRelayList::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_urls_default_is_empty() {
        // A fresh slot starts empty so resolver / FFI readers observe the
        // safe "nothing configured yet" shape before the actor pushes
        // anything.
        let urls = RelayUrls::new();
        assert!(urls.as_slice().is_empty());
    }

    #[test]
    fn relay_urls_replace_overwrites_inner() {
        // Replace is the sole-writer affordance — confirm it actually
        // overwrites instead of merging.
        let mut urls = RelayUrls::new();
        urls.replace(vec!["wss://one.example".to_string()]);
        assert_eq!(urls.as_slice(), &["wss://one.example".to_string()]);
        urls.replace(vec!["wss://two.example".to_string()]);
        assert_eq!(urls.as_slice(), &["wss://two.example".to_string()]);
    }

    #[test]
    fn relay_edit_row_list_replace_overwrites_inner() {
        // Same sole-writer semantics as `RelayUrls::replace`, but holding
        // typed `AppRelay` records instead of bare URL strings.
        // `AppRelay` is built via `::new(url, role)` — the constructor
        // canonicalizes the role string only; no display fields are derived
        // (ADR-0041: presentation strings were removed from kernel state).
        let mut rows = AppRelayList::new();
        rows.replace(vec![AppRelay::new(
            "wss://r.example".to_string(),
            "read".to_string(),
        )]);
        assert_eq!(rows.as_slice().len(), 1);
        rows.replace(Vec::new());
        assert!(rows.as_slice().is_empty());
    }

    #[test]
    fn slot_constructors_return_independent_handles() {
        // Each `new_*_slot` must return a fresh `Arc<Mutex<…>>` so two
        // constructors never alias each other's contents.
        let a = new_indexer_relays_slot();
        let b = new_indexer_relays_slot();
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn relay_urls_serialize_round_trips_through_projection_value() {
        // The slot is serialized into `KernelSnapshot::projections` as a JSON
        // array of strings. The newtype around `Vec<String>` must serialize
        // identically to the underlying `Vec<String>` so consumers can decode
        // the projection key as a plain string list.
        let urls = RelayUrls(vec![
            "wss://a.example".to_string(),
            "wss://b.example".to_string(),
        ]);
        let value = serde_json::to_value(&urls).expect("serializes");
        let plain = serde_json::to_value(vec![
            "wss://a.example".to_string(),
            "wss://b.example".to_string(),
        ])
        .expect("serializes");
        assert_eq!(value, plain);
    }
}
