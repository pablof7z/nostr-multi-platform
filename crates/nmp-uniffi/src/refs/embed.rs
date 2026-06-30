//! Typed event-embed UniFFI adapters — M14-C3.
//!
//! Mirrors the C-ABI typed event adapters:
//! `nmp_app_resolve_event_embed`, `nmp_app_resolve_event_embed_live`,
//! `nmp_app_resolve_event_embed_with_metadata`,
//! `nmp_app_resolve_event_embed_live_with_metadata`,
//! `nmp_app_release_event_ref`.

use super::{to_core_metadata, ResolveMetadata};
use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Resolve an event embed with CacheOk liveness and no URI metadata.
    ///
    /// Mirrors `nmp_app_resolve_event_embed`.
    /// D6: invalid key is a no-op at the kernel. D8: fire-and-forget.
    pub fn resolve_event_embed(&self, key: String, consumer_id: String) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            key,
            consumer_id,
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            nmp_core::RefLiveness::CacheOk,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    /// Resolve a live event embed (tailing subscription).
    ///
    /// Mirrors `nmp_app_resolve_event_embed_live`.
    pub fn resolve_event_embed_live(&self, key: String, consumer_id: String) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            key,
            consumer_id,
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            nmp_core::RefLiveness::Live,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    /// Resolve an event embed with caller-decoded relay/author metadata,
    /// CacheOk liveness.
    ///
    /// Mirrors `nmp_app_resolve_event_embed_with_metadata`.
    pub fn resolve_event_embed_with_metadata(
        &self,
        key: String,
        consumer_id: String,
        metadata: ResolveMetadata,
    ) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            key,
            consumer_id,
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            nmp_core::RefLiveness::CacheOk,
            to_core_metadata(metadata),
        );
    }

    /// Resolve a live event embed with caller-decoded relay/author metadata.
    ///
    /// Mirrors `nmp_app_resolve_event_embed_live_with_metadata`.
    pub fn resolve_event_embed_live_with_metadata(
        &self,
        key: String,
        consumer_id: String,
        metadata: ResolveMetadata,
    ) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            key,
            consumer_id,
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            nmp_core::RefLiveness::Live,
            to_core_metadata(metadata),
        );
    }

    /// Release an event ref acquired through a typed event adapter.
    ///
    /// Mirrors `nmp_app_release_event_ref`. Idempotent (D6).
    pub fn release_event_ref(&self, key: String, consumer_id: String) {
        self.inner
            .release_ref(nmp_core::RefNamespace::Event, key, consumer_id);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_EVENT_ID: &str = "b3e392b11f5d4f28321cedd09303a748d8dcf77cc7b8840dce05daf95c68b600";
    const VALID_PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    /// Parity with `nmp_app_resolve_event_embed`: must not panic.
    #[test]
    fn parity_resolve_event_embed_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_event_embed(VALID_EVENT_ID.to_string(), "view-e1".to_string());
    }

    /// Parity with `nmp_app_resolve_event_embed_live`: must not panic.
    #[test]
    fn parity_resolve_event_embed_live_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_event_embed_live(VALID_EVENT_ID.to_string(), "view-e2".to_string());
    }

    /// Parity with `nmp_app_resolve_event_embed_with_metadata`: must not panic.
    #[test]
    fn parity_resolve_event_embed_with_metadata_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_event_embed_with_metadata(
            VALID_EVENT_ID.to_string(),
            "view-e3".to_string(),
            ResolveMetadata {
                hints: vec!["wss://relay.example.com".to_string()],
                event_author: Some(VALID_PUBKEY.to_string()),
            },
        );
    }

    /// Parity with `nmp_app_resolve_event_embed_live_with_metadata`: must not panic.
    #[test]
    fn parity_resolve_event_embed_live_with_metadata_no_panic() {
        let app = crate::NmpApp::new();
        app.resolve_event_embed_live_with_metadata(
            VALID_EVENT_ID.to_string(),
            "view-e4".to_string(),
            ResolveMetadata {
                hints: vec![],
                event_author: None,
            },
        );
    }

    /// Teardown lifecycle: `resolve_event_embed_live` → `release_event_ref` → idempotent.
    #[test]
    fn teardown_event_embed_live_release_idempotent() {
        let app = crate::NmpApp::new();
        let consumer = "event-embed-teardown".to_string();
        app.resolve_event_embed_live(VALID_EVENT_ID.to_string(), consumer.clone());
        app.release_event_ref(VALID_EVENT_ID.to_string(), consumer.clone());
        // Second release — idempotent.
        app.release_event_ref(VALID_EVENT_ID.to_string(), consumer);
    }

    /// D6: release of a never-registered event ref must be a silent no-op.
    #[test]
    fn parity_release_event_ref_idempotent_no_panic() {
        let app = crate::NmpApp::new();
        app.release_event_ref(VALID_EVENT_ID.to_string(), "view-e5".to_string());
        app.release_event_ref(VALID_EVENT_ID.to_string(), "view-e5".to_string());
    }

    /// `resolve_event_embed_live_with_metadata` then release via `release_event_ref`.
    #[test]
    fn teardown_event_embed_live_with_metadata_release_idempotent() {
        let app = crate::NmpApp::new();
        let consumer = "event-meta-teardown".to_string();
        app.resolve_event_embed_live_with_metadata(
            VALID_EVENT_ID.to_string(),
            consumer.clone(),
            ResolveMetadata {
                hints: vec!["wss://relay.example.com".to_string()],
                event_author: None,
            },
        );
        app.release_event_ref(VALID_EVENT_ID.to_string(), consumer.clone());
        app.release_event_ref(VALID_EVENT_ID.to_string(), consumer);
    }
}
