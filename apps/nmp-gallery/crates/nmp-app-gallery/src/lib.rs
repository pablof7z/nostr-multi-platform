//! `nmp-app-gallery` — the NmpGallery app-owned UniFFI facade.
//!
//! The gallery composes the shared NMP substrate and selected protocol
//! installers, then exposes one app-owned UniFFI object for native shells. The
//! shell renders and executes capabilities; Rust owns composition, policy,
//! lifecycle, reference resolution, and snapshot decoding.

uniffi::setup_scaffolding!();

mod composition;
mod concept_reads_replies;
mod event_ref;
mod facade;
mod snapshot_json;

pub mod ownership;
pub mod registry;
pub mod showcase;

const GALLERY_COMPOSITION_ROOT: &str = "nmp-app-gallery";
const GALLERY_COMPOSITION_PROVIDER: &str = "nmp_app_gallery::install_gallery_composition";

pub use composition::install_gallery_composition;
pub use concept_reads_replies::{GalleryOpenedReplies, GalleryReadError, GalleryReplySummary};
pub use facade::{
    GalleryApp, GalleryCapabilitySink, GalleryDispatchOutcome, GalleryEventRef, GalleryEventShape,
    GalleryProfileShape, GalleryRefLiveness, GalleryRefNamespace, GalleryRefShape,
    GalleryResolveMetadata, GalleryUpdateSink,
};

#[must_use]
pub fn register_gallery_composition(app: &mut nmp_native_runtime::NmpApp) -> bool {
    if !app.claim_composition_root(GALLERY_COMPOSITION_ROOT, GALLERY_COMPOSITION_PROVIDER) {
        return false;
    }
    install_gallery_composition(app);
    app.consume_all_builtin_projections();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_constructor_installs_composition_and_starts() {
        let app = GalleryApp::new();
        app.start(256, 4);
        app.shutdown();
    }

    #[test]
    fn decode_snapshot_json_empty_frame_returns_none() {
        let app = GalleryApp::new();
        assert!(app.decode_snapshot_json(Vec::new()).is_none());
    }

    #[test]
    fn decode_snapshot_json_malformed_frame_returns_none() {
        let app = GalleryApp::new();
        assert!(app.decode_snapshot_json(vec![0u8; 8]).is_none());
    }

    #[test]
    fn register_gallery_composition_is_one_shot() {
        let mut app = nmp_native_runtime::new_app();

        assert!(register_gallery_composition(&mut app));
        let first_report = app.debug_info_json(nmp_native_runtime::DOMAIN_COMPOSITION);
        let first_count = first_report["count"]
            .as_u64()
            .expect("composition count must be numeric");

        assert!(!register_gallery_composition(&mut app));
        let second_report = app.debug_info_json(nmp_native_runtime::DOMAIN_COMPOSITION);
        let second_count = second_report["count"]
            .as_u64()
            .expect("composition count must be numeric");
        assert_eq!(second_count, first_count + 1);

        let records = second_report["records"]
            .as_array()
            .expect("composition records must be an array");
        let root_records: Vec<_> = records
            .iter()
            .filter(|record| {
                record["seam"] == "composition_root" && record["key"] == GALLERY_COMPOSITION_ROOT
            })
            .collect();
        assert_eq!(root_records.len(), 2);
        assert_eq!(root_records[0]["disposition"], "Installed");
        assert_eq!(root_records[1]["disposition"], "YieldedToExisting");
        assert_eq!(root_records[1]["replaced"], GALLERY_COMPOSITION_PROVIDER);
    }
}
