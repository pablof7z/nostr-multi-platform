//! Typed profile-ref UniFFI adapters — M14-C3.
//!
//! Mirrors the C-ABI typed profile adapters:
//! `nmp_app_resolve_profile_ref`, `nmp_app_resolve_profile_card_live`,
//! `nmp_app_release_profile_ref`.

use nmp_core::__ffi_internal::is_hex_pubkey;

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Resolve a profile ref (feed-avatar shape, CacheOk liveness).
    ///
    /// Typed adapter: fixes namespace=Profile, shape=Ref, liveness=CacheOk.
    /// Use for feed-row avatars. Mirrors `nmp_app_resolve_profile_ref`.
    ///
    /// D6: invalid `key` is a silent no-op. D8: fire-and-forget.
    pub fn resolve_profile_ref(&self, key: String, consumer_id: String) {
        if !is_hex_pubkey(&key) {
            return;
        }
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Profile,
            key,
            consumer_id,
            nmp_core::RefShape::Profile(nmp_core::ProfileShape::Ref),
            nmp_core::RefLiveness::CacheOk,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    /// Resolve a live profile card (full-card shape, Live liveness).
    ///
    /// Typed adapter: fixes namespace=Profile, shape=Card, liveness=Live.
    /// Use for open profile screens. Mirrors `nmp_app_resolve_profile_card_live`.
    ///
    /// D6: invalid `key` is a silent no-op. D8: fire-and-forget.
    pub fn resolve_profile_card_live(&self, key: String, consumer_id: String) {
        if !is_hex_pubkey(&key) {
            return;
        }
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Profile,
            key,
            consumer_id,
            nmp_core::RefShape::Profile(nmp_core::ProfileShape::Card),
            nmp_core::RefLiveness::Live,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    /// Release a profile ref acquired through a typed profile adapter.
    ///
    /// Mirrors `nmp_app_release_profile_ref`. Idempotent (D6).
    pub fn release_profile_ref(&self, key: String, consumer_id: String) {
        if !is_hex_pubkey(&key) {
            return;
        }
        self.inner
            .release_ref(nmp_core::RefNamespace::Profile, key, consumer_id);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_PUBKEY: &str =
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    /// Parity with `nmp_app_resolve_profile_ref`: must not panic.
    #[test]
    fn parity_resolve_profile_ref_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_profile_ref(VALID_PUBKEY.to_string(), "view-p1".to_string());
    }

    /// D6: invalid pubkey must be a silent no-op.
    #[test]
    fn parity_resolve_profile_ref_invalid_key_noop() {
        let app = crate::NmpApp::new();
        app.resolve_profile_ref("not-a-pubkey".to_string(), "view-p1".to_string());
    }

    /// Parity with `nmp_app_resolve_profile_card_live`: must not panic.
    #[test]
    fn parity_resolve_profile_card_live_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_profile_card_live(VALID_PUBKEY.to_string(), "view-p2".to_string());
    }

    /// Teardown lifecycle: `resolve_profile_card_live` → `release_profile_ref` → idempotent.
    #[test]
    fn teardown_profile_card_live_release_idempotent() {
        let app = crate::NmpApp::new();
        let consumer = "profile-card-teardown".to_string();
        app.resolve_profile_card_live(VALID_PUBKEY.to_string(), consumer.clone());
        app.release_profile_ref(VALID_PUBKEY.to_string(), consumer.clone());
        // Second release — idempotent.
        app.release_profile_ref(VALID_PUBKEY.to_string(), consumer);
    }

    /// Parity with `nmp_app_release_profile_ref`: idempotent on unknown key.
    #[test]
    fn parity_release_profile_ref_idempotent() {
        let app = crate::NmpApp::new();
        app.release_profile_ref(VALID_PUBKEY.to_string(), "view-p3".to_string());
        app.release_profile_ref(VALID_PUBKEY.to_string(), "view-p3".to_string());
    }
}
