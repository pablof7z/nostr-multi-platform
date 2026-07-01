//! Observed feed-source sessions for Rust app projections.
//!
//! This is the reusable source-graph half of a feed session without the generic
//! note-feed row sidecar. It lets a Rust app crate attach an app-owned
//! [`ObservedProjectionSink`] to the same `FeedScope` resolver, dependent
//! acquisition replacement, source-effect reset, and handle-based teardown that
//! ordinary `open_feed` uses. Native shells still only render typed projections
//! registered by the app crate.

use std::sync::Arc;

use crate::source::{acquisition_children, ExtraAcquisition, ReducedSource};
use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{FeedAdmission, FeedParams, FeedSessionBuild, ProjectionKey};

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
/// The params' primary kinds, acquisition source, admission policy, and
/// projection key are honored. Render/ranking/window remain meaningful for the
/// generic feed sidecar path; this source-only path uses `replay_limit` for
/// replay bounds and leaves row/schema meaning to the app-owned projection.
pub fn compile_observed_feed_source<H: FeedSessionHost>(
    app: &H,
    params: &FeedParams,
    acquisition_kinds: &std::collections::BTreeSet<u32>,
    options: ObservedFeedSourceOptions,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let mut resolved =
        crate::custom::resolve_acquisition(app, &params.acquisition, acquisition_kinds)?;

    if let FeedAdmission::Custom(id) = &params.admission {
        resolved = crate::custom::apply_custom_admission(app, resolved, id, acquisition_kinds)?;
    }

    build_observed_source_session(app, params.projection.as_str(), resolved, options)
}

fn build_observed_source_session(
    app: &impl FeedSessionHost,
    key: &str,
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
        reset_hooks,
        source_effect_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set: _,
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
    let owner = crate::session_engine::session_acquisition_owner(key);
    let fixed_acquisition = Arc::new(interests);
    let sync_acquisition = {
        let sender = sender.clone();
        let fixed_acquisition = Arc::clone(&fixed_acquisition);
        move |extra: &ExtraAcquisition| {
            let children = acquisition_children(&fixed_acquisition, extra);
            let _ = sender.send(ActorCommand::Interests(
                InterestsCommand::ReplaceDependentInterestSet {
                    owner,
                    children,
                    reason: "feed-observed-source-acquisition".to_string(),
                },
            ));
        }
    };
    sync_acquisition(&extra_acquisition);

    for hook in reset_hooks {
        let extra = Arc::clone(&extra_acquisition);
        let sync_acquisition = sync_acquisition.clone();
        let sync_observer = source_observer.clone();
        let notify = sender.clone();
        let reset = options.reset_on_source_change.clone();
        let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            reset_app_projection(reset.as_ref());
            sync_observer.sync();
            sync_acquisition(&extra);
            notify.mark_changed_since_emit();
        });
        hook(trigger);
    }

    for hook in source_effect_hooks {
        let extra = Arc::clone(&extra_acquisition);
        let sync_acquisition = sync_acquisition.clone();
        let sync_observer = source_observer.clone();
        let notify = sender.clone();
        let reset = options.reset_on_source_change.clone();
        let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            reset_app_projection(reset.as_ref());
            sync_observer.sync();
            sync_acquisition(&extra);
            notify.mark_changed_since_emit();
        });
        hook(trigger);
    }

    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(app.mark_changed_action());
    teardown.push(crate::session_engine::clear_acquisition_set(
        sender.clone(),
        owner,
    ));
    teardown.push(app.remove_projection_action(key.to_string()));
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
