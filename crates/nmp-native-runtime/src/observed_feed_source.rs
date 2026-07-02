//! `NmpApp::open_observed_feed_source` — feed-source lifecycle for app rows.
//!
//! This is the Rust app-crate hook for app-owned projections that need NMP's
//! reusable feed source graph without adopting the generic note-feed sidecar.

use std::sync::Arc;

use crate::{FeedHandle, FeedOpenError, FeedParams, NmpApp};
use nmp_core::ObservedProjectionSink;

impl NmpApp {
    /// Open a feed-source session that delivers admitted events to `observer`.
    ///
    /// This uses the same `FeedScope` compiler, dependent-interest owner,
    /// reactive source-effect hooks, and `FeedHandle` teardown registry as
    /// [`Self::open_feed`]. The app crate owns row/schema projection meaning;
    /// native shells only consume the typed snapshot the app crate registers
    /// under `params.projection`.
    pub fn open_observed_feed_source(
        &self,
        params: &FeedParams,
        observer: Arc<dyn ObservedProjectionSink>,
        replay_limit: usize,
        reset_on_source_change: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<FeedHandle, FeedOpenError> {
        self.open_feed_with_output(params, move |app, params, acquisition_kinds| {
            let options = nmp_feed_session::ObservedFeedSourceOptions {
                observer,
                replay_limit,
                reset_on_source_change,
            };
            nmp_feed_session::compile_observed_feed_source(app, params, acquisition_kinds, options)
                .map(|build| (build, ()))
        })
        .map(|(handle, ())| handle)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use nmp_core::substrate::KernelEvent;
    use nmp_core::TypedProjectionData;
    use nmp_feed::{
        FeedAdmission, FeedOrder, FeedScope, FeedShape, FeedWindowPolicy, ProjectionKey, TagTerm,
    };

    #[derive(Default)]
    struct CountingObserver;

    impl ObservedProjectionSink for CountingObserver {
        fn on_kernel_event(&self, _event: &KernelEvent) {}
    }

    fn tag_params() -> FeedParams {
        FeedParams {
            primary_kinds: vec![1],
            shape: FeedShape::Flat,
            source: FeedScope::Tag {
                term: TagTerm("rust".to_string()),
            },
            admission: FeedAdmission::All,
            order: FeedOrder::NewestByFeedPosition,
            window: FeedWindowPolicy { initial_limit: 20 },
            projection: ProjectionKey::app_owned("test.observed.rust").unwrap(),
        }
    }

    #[test]
    fn observed_feed_source_closes_by_handle_and_removes_projection() {
        let app = crate::new_app();
        let params = tag_params();
        let observer: Arc<dyn ObservedProjectionSink> = Arc::new(CountingObserver);

        let handle = app
            .open_observed_feed_source(&params, observer, 20, None)
            .expect("observed source opens");

        assert!(app.feed_session_is_open(&handle));
        assert_eq!(app.live_feed_session_count(), 1);
        assert!(
            app.test_observed_projection_sink_count() > 0,
            "observed source registered at least one sink"
        );

        app.register_typed_snapshot_projection(params.projection.dynamic_token(), || {
            Some(TypedProjectionData {
                key: "test.observed.rust".to_string(),
                schema_id: "test.observed.rust".to_string(),
                schema_version: 1,
                file_identifier: "TEST".to_string(),
                payload: vec![1, 2, 3],
                ..Default::default()
            })
        });
        assert!(app
            .run_typed_snapshot_projections_for_test()
            .iter()
            .any(|projection| projection.key == "test.observed.rust"));

        assert!(app.close_feed(&handle));

        assert!(!app.feed_session_is_open(&handle));
        assert_eq!(app.live_feed_session_count(), 0);
        assert_eq!(app.test_observed_projection_sink_count(), 0);
        assert!(!app
            .registered_typed_projection_keys()
            .iter()
            .any(|key| key == "test.observed.rust"));
    }

    #[test]
    fn observed_feed_source_rejects_invalid_primary_kinds_before_registering() {
        let app = crate::new_app();
        let mut params = tag_params();
        params.primary_kinds = vec![5];
        let observer: Arc<dyn ObservedProjectionSink> = Arc::new(CountingObserver);

        assert!(app
            .open_observed_feed_source(&params, observer, 20, None)
            .is_err());
        assert_eq!(app.live_feed_session_count(), 0);
        assert_eq!(app.test_observed_projection_sink_count(), 0);
    }
}
