//! General (origin-blind) reference resolution — M14-C3.
//!
//! Mirrors `nmp_app_resolve_ref`, `nmp_app_resolve_ref_with_metadata`, and
//! `nmp_app_release_ref` from `nmp-ffi/src/resolve_ref.rs`.
//!
//! See `refs/mod.rs` for the reactive-lifecycle design note.

use nmp_core::__ffi_internal::is_hex_pubkey;

use super::{
    to_core_liveness, to_core_metadata, to_core_namespace, to_core_shape, RefLiveness,
    RefNamespace, RefShape, ResolveMetadata,
};
use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Register (or upgrade) a consumer's interest in `(namespace, key)`.
    ///
    /// Mirrors `nmp_app_resolve_ref`. The kernel refcounts per `consumer_id`;
    /// a key already held by another consumer is deduped to one resolver slot
    /// with the widest requested shape and the highest liveness (`Live` wins).
    ///
    /// `namespace` — `Profile` or `Event`.
    /// `key` — 64-hex pubkey (Profile); hex event-id, `"kind:pubkey:d"`, or
    ///   `"i:<external-id>"` (Event). Not a `nostr:` URI.
    /// `consumer_id` — caller-chosen refcount owner key (e.g. SwiftUI view id).
    ///   The same string MUST be passed to `release_ref` to tear down.
    /// `shape` — determines which projection field is populated.
    /// `liveness` — `CacheOk` (background) or `Live` (open screen).
    ///
    /// D6: invalid keys, null arguments, and namespace/shape mismatches are
    /// silent no-ops. D8: fire-and-forget; the actor processes asynchronously.
    pub fn resolve_ref(
        &self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
    ) {
        if namespace == RefNamespace::Profile && !is_hex_pubkey(&key) {
            return;
        }
        self.inner.resolve_ref_with_metadata(
            to_core_namespace(&namespace),
            key,
            consumer_id,
            to_core_shape(&shape),
            to_core_liveness(&liveness),
            nmp_core::RefResolveMetadata::default(),
        );
    }

    /// Register (or upgrade) a consumer's interest with caller-decoded relay
    /// and author metadata.
    ///
    /// Mirrors `nmp_app_resolve_ref_with_metadata`. `metadata.hints` are relay
    /// URL hints decoded from NIP-19/NIP-21 TLVs by the caller. The `key` is
    /// always raw — never a `nostr:` URI.
    ///
    /// D6: all validation rules from `resolve_ref` apply; invalid metadata is
    /// a silent no-op (caller is expected to pass valid decoded values).
    pub fn resolve_ref_with_metadata(
        &self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        metadata: ResolveMetadata,
    ) {
        if namespace == RefNamespace::Profile && !is_hex_pubkey(&key) {
            return;
        }
        self.inner.resolve_ref_with_metadata(
            to_core_namespace(&namespace),
            key,
            consumer_id,
            to_core_shape(&shape),
            to_core_liveness(&liveness),
            to_core_metadata(metadata),
        );
    }

    /// Release a reference previously registered via `resolve_ref`.
    ///
    /// Mirrors `nmp_app_release_ref`. Decrements the refcount for
    /// `consumer_id`'s stake in `(namespace, key)`. The resolver slot is torn
    /// down when the last consumer releases.
    ///
    /// This is the teardown call for **both** `CacheOk` and `Live` resolves.
    /// For `Live` resolves the tailing subscription is quiesced before the
    /// slot is dropped (no further callbacks or UAF).
    ///
    /// D6: null/invalid arguments and unknown triples are silent no-ops
    /// (idempotent). D8: fire-and-forget.
    pub fn release_ref(&self, namespace: RefNamespace, key: String, consumer_id: String) {
        if namespace == RefNamespace::Profile && !is_hex_pubkey(&key) {
            return;
        }
        self.inner
            .release_ref(to_core_namespace(&namespace), key, consumer_id);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::{EventShape, ProfileShape};
    use super::*;

    const VALID_PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const VALID_EVENT_ID: &str = "b3e392b11f5d4f28321cedd09303a748d8dcf77cc7b8840dce05daf95c68b600";

    // ── Parity: resolve_ref (profile) ─────────────────────────────────────

    /// Parity with C-ABI `nmp_app_resolve_ref(namespace=0, …)`:
    /// `resolve_ref(Profile, valid_pubkey, …)` must not panic (D6 + D8).
    #[test]
    fn parity_resolve_profile_ref_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_ref(
            RefNamespace::Profile,
            VALID_PUBKEY.to_string(),
            "view-1".to_string(),
            RefShape::Profile {
                shape: ProfileShape::Ref,
            },
            RefLiveness::CacheOk,
        );
    }

    /// D6: invalid pubkey for Profile namespace must be a silent no-op.
    #[test]
    fn parity_resolve_profile_invalid_key_silent_noop() {
        let app = crate::NmpApp::new();
        // Should not panic — just silently ignored.
        app.resolve_ref(
            RefNamespace::Profile,
            "not-a-valid-pubkey".to_string(),
            "view-1".to_string(),
            RefShape::Profile {
                shape: ProfileShape::Card,
            },
            RefLiveness::Live,
        );
    }

    /// Parity with C-ABI `nmp_app_resolve_ref(namespace=1, …)`:
    /// `resolve_ref(Event, valid_event_id, …)` must not panic.
    #[test]
    fn parity_resolve_event_ref_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_ref(
            RefNamespace::Event,
            VALID_EVENT_ID.to_string(),
            "view-2".to_string(),
            RefShape::Event {
                shape: EventShape::Embed,
            },
            RefLiveness::CacheOk,
        );
    }

    // ── Parity: resolve_ref_with_metadata ────────────────────────────────

    /// Parity with C-ABI `nmp_app_resolve_ref_with_metadata`:
    /// metadata with hints and event_author passes through without panic.
    #[test]
    fn parity_resolve_ref_with_metadata_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_ref_with_metadata(
            RefNamespace::Event,
            VALID_EVENT_ID.to_string(),
            "view-3".to_string(),
            RefShape::Event {
                shape: EventShape::Embed,
            },
            RefLiveness::Live,
            ResolveMetadata {
                hints: vec!["wss://relay.example.com".to_string()],
                event_author: Some(VALID_PUBKEY.to_string()),
            },
        );
    }

    /// Empty metadata (no hints, no author) must not panic.
    #[test]
    fn parity_resolve_ref_empty_metadata_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_ref_with_metadata(
            RefNamespace::Profile,
            VALID_PUBKEY.to_string(),
            "view-4".to_string(),
            RefShape::Profile {
                shape: ProfileShape::Card,
            },
            RefLiveness::CacheOk,
            ResolveMetadata {
                hints: vec![],
                event_author: None,
            },
        );
    }

    // ── Parity: release_ref ───────────────────────────────────────────────

    /// Parity with C-ABI `nmp_app_release_ref(namespace=0, …)`:
    /// release of a never-registered key must be a silent no-op (D6).
    #[test]
    fn parity_release_profile_ref_idempotent_no_panic() {
        let app = crate::NmpApp::new();
        // First call — key was never registered.
        app.release_ref(
            RefNamespace::Profile,
            VALID_PUBKEY.to_string(),
            "view-1".to_string(),
        );
        // Second call — idempotent.
        app.release_ref(
            RefNamespace::Profile,
            VALID_PUBKEY.to_string(),
            "view-1".to_string(),
        );
    }

    /// Teardown lifecycle test: resolve-live → release → release (idempotent).
    ///
    /// Mirrors the C-ABI lifecycle contract:
    /// * `resolve_ref(…, Live)` registers a live subscription.
    /// * `release_ref(…)` tears it down (no UAF, no leak).
    /// * A second `release_ref` is a silent no-op (idempotent).
    ///
    /// This is the canonical teardown test for the reactive lifecycle.
    #[test]
    fn teardown_live_resolve_then_release_idempotent() {
        let app = crate::NmpApp::new();
        let consumer = "live-consumer-teardown-test".to_string();

        // Register a live subscription.
        app.resolve_ref(
            RefNamespace::Profile,
            VALID_PUBKEY.to_string(),
            consumer.clone(),
            RefShape::Profile {
                shape: ProfileShape::Card,
            },
            RefLiveness::Live,
        );

        // Tear it down.
        app.release_ref(
            RefNamespace::Profile,
            VALID_PUBKEY.to_string(),
            consumer.clone(),
        );

        // Second release must be idempotent (D6 silent no-op, no panic, no UAF).
        app.release_ref(RefNamespace::Profile, VALID_PUBKEY.to_string(), consumer);
    }

    /// D6: invalid key for release must be a silent no-op.
    #[test]
    fn parity_release_invalid_key_silent_noop() {
        let app = crate::NmpApp::new();
        app.release_ref(
            RefNamespace::Profile,
            "bad-key".to_string(),
            "view-1".to_string(),
        );
    }
}
