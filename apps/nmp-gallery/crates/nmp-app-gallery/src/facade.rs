use std::sync::{Arc, Mutex};

use nmp_native_runtime::NmpApp as RuntimeApp;
use zeroize::Zeroizing;

use crate::composition::gallery_nip55_permissions;

#[uniffi::export(callback_interface)]
pub trait GalleryUpdateSink: Send + Sync {
    fn on_update(&self, frame: Vec<u8>);
}

#[uniffi::export(callback_interface)]
pub trait GalleryCapabilitySink: Send + Sync {
    fn on_capability_request(&self, request_json: String) -> String;
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct GalleryDispatchOutcome {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
    pub code: Option<String>,
}

impl From<nmp_uniffi_support::DispatchOutcome> for GalleryDispatchOutcome {
    fn from(out: nmp_uniffi_support::DispatchOutcome) -> Self {
        Self {
            correlation_id: out.correlation_id,
            error: out.error,
            code: out.code,
        }
    }
}

#[uniffi::export]
pub fn gallery_showcase_references_json() -> String {
    crate::showcase::raw_json().to_string()
}

#[uniffi::export]
pub fn gallery_registry_json() -> String {
    crate::registry::raw_json().to_string()
}

#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum GalleryRefNamespace {
    Profile,
    Event,
}

#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum GalleryProfileShape {
    Ref,
    Card,
}

#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum GalleryEventShape {
    Embed,
    Raw,
}

#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum GalleryRefShape {
    Profile { shape: GalleryProfileShape },
    Event { shape: GalleryEventShape },
}

#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum GalleryRefLiveness {
    CacheOk,
    Live,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct GalleryResolveMetadata {
    pub hints: Vec<String>,
    pub event_author: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct GalleryEventRef {
    pub key: String,
    pub metadata: GalleryResolveMetadata,
}

#[derive(uniffi::Object)]
pub struct GalleryApp {
    inner: RuntimeApp,
    ref_stores: Mutex<GalleryRefStores>,
}

pub struct GalleryRefStores {
    pub(crate) profiles: nmp_core::refs::RefProfileStore,
    pub(crate) events: nmp_core::refs::RefEventStore,
}

impl GalleryRefStores {
    fn new() -> Self {
        Self {
            profiles: nmp_core::refs::RefProfileStore::new(),
            events: nmp_core::refs::RefEventStore::new(),
        }
    }
}

#[uniffi::export]
impl GalleryApp {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        let mut inner = nmp_native_runtime::new_app();
        let installed = crate::register_gallery_composition(&mut inner);
        inner.set_external_signer_permissions(gallery_nip55_permissions());
        debug_assert!(
            installed,
            "fresh GalleryApp must install its composition root"
        );
        Arc::new(Self {
            inner,
            ref_stores: Mutex::new(GalleryRefStores::new()),
        })
    }

    pub fn start(&self, visible_limit: u32, emit_hz: u32) {
        nmp_uniffi_support::start_runtime(&self.inner, visible_limit, emit_hz);
    }

    pub fn configure(&self, visible_limit: u32, emit_hz: u32) {
        nmp_uniffi_support::configure_runtime(&self.inner, visible_limit, emit_hz);
    }

    pub fn stop(&self) {
        self.inner.stop_runtime();
    }

    pub fn reset(&self) {
        self.inner.reset_runtime();
    }

    pub fn shutdown(&self) {
        self.inner.shutdown();
    }

    pub fn set_storage_path(&self, path: Option<String>) {
        self.inner.set_storage_path(path);
    }

    pub fn set_update_sink(&self, sink: Option<Box<dyn GalleryUpdateSink>>) {
        nmp_uniffi_support::set_update_sink(&self.inner, sink, |sink, frame| {
            sink.on_update(frame);
        });
    }

    pub fn set_capability_callback(&self, sink: Option<Box<dyn GalleryCapabilitySink>>) {
        nmp_uniffi_support::set_capability_callback(&self.inner, sink, |sink, request_json| {
            sink.on_capability_request(request_json)
        });
    }

    pub fn dispatch_capability_json(&self, request_json: String) -> String {
        nmp_uniffi_support::dispatch_capability_json(&self.inner, &request_json)
    }

    pub fn dispatch_action(&self, envelope: Vec<u8>) -> GalleryDispatchOutcome {
        nmp_uniffi_support::dispatch_action_vec(&self.inner, envelope).into()
    }

    pub fn resolve_profile_ref(&self, key: String, consumer_id: String) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Profile,
            key,
            consumer_id,
            nmp_core::RefShape::Profile(nmp_core::ProfileShape::Ref),
            nmp_core::RefLiveness::CacheOk,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    pub fn resolve_profile_card(&self, key: String, consumer_id: String) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Profile,
            key,
            consumer_id,
            nmp_core::RefShape::Profile(nmp_core::ProfileShape::Card),
            nmp_core::RefLiveness::CacheOk,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    pub fn resolve_profile_card_live(&self, key: String, consumer_id: String) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Profile,
            key,
            consumer_id,
            nmp_core::RefShape::Profile(nmp_core::ProfileShape::Card),
            nmp_core::RefLiveness::Live,
            nmp_core::RefResolveMetadata::default(),
        );
    }

    pub fn release_profile_ref(&self, key: String, consumer_id: String) {
        self.inner
            .release_ref(nmp_core::RefNamespace::Profile, key, consumer_id);
    }

    pub fn resolve_event_embed_with_metadata(
        &self,
        key: String,
        consumer_id: String,
        metadata: GalleryResolveMetadata,
    ) {
        self.resolve_event_embed(key, consumer_id, metadata, nmp_core::RefLiveness::CacheOk);
    }

    pub fn resolve_event_embed_live_with_metadata(
        &self,
        key: String,
        consumer_id: String,
        metadata: GalleryResolveMetadata,
    ) {
        self.resolve_event_embed(key, consumer_id, metadata, nmp_core::RefLiveness::Live);
    }

    pub fn release_event_ref(&self, key: String, consumer_id: String) {
        self.inner
            .release_ref(nmp_core::RefNamespace::Event, key, consumer_id);
    }

    pub fn add_relay(&self, url: String, role: Option<String>) {
        let role = role
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "both".to_string());
        self.inner.add_relay(url, role);
    }

    pub fn init_external_signer(&self) {
        self.inner.init_external_signer();
    }

    pub fn signin_nip55(&self, signer_package: Option<String>) {
        self.inner.signin_nip55(signer_package);
    }

    pub fn deliver_external_signer_response(&self, response_json: String) {
        self.inner.deliver_external_signer_response(&response_json);
    }

    pub fn signin_nsec(&self, secret: String, make_active: bool) {
        let secret = Zeroizing::new(secret);
        self.inner
            .add_signer(nmp_core::SignerSource::LocalNsec(secret), make_active);
    }

    pub fn gallery_showcase_references_json(&self) -> String {
        gallery_showcase_references_json()
    }

    pub fn gallery_registry_json(&self) -> String {
        gallery_registry_json()
    }

    pub fn event_ref_from_uri(&self, uri: String) -> Option<GalleryEventRef> {
        crate::event_ref::event_ref_from_uri(&uri).map(|event_ref| GalleryEventRef {
            key: event_ref.key,
            metadata: GalleryResolveMetadata {
                hints: event_ref.hints,
                event_author: event_ref.event_author,
            },
        })
    }

    pub fn decode_snapshot_json(&self, frame: Vec<u8>) -> Option<String> {
        if frame.is_empty() {
            return None;
        }
        let Ok(mut guard) = self.ref_stores.lock() else {
            return None;
        };
        let stores = &mut *guard;
        crate::snapshot_json::snapshot_json_from_update_frame(
            &frame,
            &mut stores.profiles,
            &mut stores.events,
        )
        .ok()
    }
}

impl GalleryApp {
    fn resolve_event_embed(
        &self,
        key: String,
        consumer_id: String,
        metadata: GalleryResolveMetadata,
        liveness: nmp_core::RefLiveness,
    ) {
        self.inner.resolve_ref_with_metadata(
            nmp_core::RefNamespace::Event,
            key,
            consumer_id,
            nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
            liveness,
            nmp_core::RefResolveMetadata {
                hints: metadata.hints,
                event_author: metadata.event_author,
            },
        );
    }
}
