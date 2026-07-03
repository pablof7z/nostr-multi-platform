//! Observed feed-source sessions for Rust app projections.
//!
//! This is the reusable source-graph half of a feed session without the generic
//! note-feed row sidecar. It lets a Rust app crate attach an app-owned
//! [`ObservedProjectionSink`] to the same `FeedScope` resolver, dependent
//! acquisition delta, source-effect reset, and handle-based teardown that
//! ordinary `open_feed` uses. Native shells still only render typed projections
//! registered by the app crate.

use std::sync::Arc;

use crate::source::ReducedSource;
use crate::trellis_adapter::FeedSessionTrellisAdapter;
use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    FeedAdmission, FeedItemProjection, FeedParams, FeedSessionBuild, FeedShape, ProjectionKey,
};

/// Options for wiring a Rust app projection to a compiled feed source.
pub struct ObservedFeedSourceOptions {
    /// App-owned row projection that receives admitted events.
    pub observer: Arc<dyn ObservedProjectionSink>,
    /// Maximum cached events replayed into each live observed-projection shape.
    pub replay_limit: usize,
    /// Optional app reset hook fired when the source set changes.
    pub reset_on_source_change: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Compile a [`FeedParams`] source into observed delivery for an app projection.
///
/// The params' primary kinds, acquisition source, admission policy, and output
/// key are honored. Render/order/window remain meaningful for the generic feed
/// sidecar path; this source-only path uses `replay_limit` for replay bounds
/// and leaves row/schema meaning to the app-owned projection.
pub fn compile_observed_feed_source<H: FeedSessionHost>(
    app: &H,
    params: &FeedParams,
    acquisition_kinds: &std::collections::BTreeSet<u32>,
    options: ObservedFeedSourceOptions,
) -> Result<FeedSessionBuild, FeedOpenError> {
    match &params.item_projection {
        FeedItemProjection::FeedRows => {}
    }

    let mut resolved = crate::custom::resolve_acquisition(app, &params.source, acquisition_kinds)?;

    if let FeedAdmission::Custom(id) = &params.admission {
        resolved = crate::custom::apply_custom_admission(app, resolved, id, acquisition_kinds)?;
    }

    build_observed_source_session(
        app,
        params.key.as_str(),
        params.shape.clone(),
        resolved,
        options,
    )
}

fn build_observed_source_session(
    app: &impl FeedSessionHost,
    key: &str,
    shape: FeedShape,
    resolved: ReducedSource,
    options: ObservedFeedSourceOptions,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let ReducedSource {
        op_session_identity: _,
        admission,
        attribution: _,
        interests,
        live_shape: _,
        live_shapes,
        observer_scope,
        extra_acquisition,
        reactivity_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set: _,
        row_context: _,
    } = resolved;

    let admitted_observer: Arc<dyn ObservedProjectionSink> = Arc::new(AdmittedFeedObserver {
        inner: options.observer,
        admission,
    });
    let source_observer = crate::dynamic_observer::DynamicObservedProjectionSet::new(
        app.observed_projection_handle(),
        admitted_observer,
        format!("{key}.observer"),
        crate::session_engine::interest_scope_code(observer_scope),
        live_shapes,
        options.replay_limit,
    );
    source_observer.sync();

    let sender = app.command_sender();
    let acquisition_adapter = FeedSessionTrellisAdapter::new_with_diagnostics(
        key,
        shape,
        interests,
        sender,
        app.feed_session_diagnostics(),
    )?;
    acquisition_adapter.sync(&extra_acquisition, "feed-observed-source-acquisition");

    for hook in reactivity_hooks {
        let extra = Arc::clone(&extra_acquisition);
        let acquisition_adapter = acquisition_adapter.clone();
        let sync_observer = source_observer.clone();
        let reset = options.reset_on_source_change.clone();
        let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            reset_app_projection(reset.as_ref());
            sync_observer.sync();
            acquisition_adapter.schedule_source_effect(
                Arc::clone(&extra),
                "feed-observed-source-acquisition",
                true,
            );
        });
        hook(trigger);
    }

    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(app.mark_changed_action());
    teardown.push(acquisition_adapter.close_action(app.remove_projection_action(key.to_string())));
    for id in resolver_observer_ids {
        let handle = app.observed_projection_handle();
        teardown.push(Box::new(move || handle.close(id)));
    }
    for id in identity_observer_ids {
        teardown.push(app.unregister_identity_change_observer_action(id));
    }
    teardown.extend(resolver_teardown);
    teardown.push(source_observer.teardown_action());

    Ok(FeedSessionBuild {
        projection_key: ProjectionKey::app_owned(key).unwrap(),
        teardown,
    })
}

fn reset_app_projection(reset: Option<&Arc<dyn Fn() + Send + Sync>>) {
    if let Some(reset) = reset {
        reset();
    }
}

struct AdmittedFeedObserver {
    inner: Arc<dyn ObservedProjectionSink>,
    admission: nmp_feed::RootAdmission,
}

impl ObservedProjectionSink for AdmittedFeedObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if (self.admission)(event) {
            self.inner.on_kernel_event(event);
        }
    }
}
