//! Browser-runtime feed-session adapter.
//!
//! Browser owns runtime lifecycle, not a second feed model. The app-visible
//! session descriptor, handle, registry, and teardown semantics come from
//! `nmp-feed`; this module only wires the browser runtime slots into the
//! existing OP-feed machinery.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::slots;
use nmp_core::substrate::ObservedProjectionCommandHandle;
use nmp_core::substrate::{
    EmptySuppressionLookup, ObservedProjectionReconciler, ObservedProjectionRegistrar,
};
use nmp_core::{CommandSender, ObservedProjectionSink, TypedProjectionData};
use nmp_feed::{
    FeedAdmission, FeedAuthorRefs, FeedHandle, FeedParams, FeedRanking, FeedRender,
    FeedRenderSource, FeedSessionBuild, FeedSessionRegistry, ProjectionKey, PubkeySetExpr,
    TeardownAction,
};
use nmp_note_feed::op_feed::{op_feed_observer, register_op_feed};
use nmp_planner::InterestShape;

type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

pub(crate) struct FeedRuntimeAccess<'a> {
    pub(crate) reducer: &'a mut nmp_core::KernelReducer,
    pub(crate) observed_projection_registrar: ObservedProjectionCommandHandle,
    pub(crate) command_sender: CommandSender,
}

pub(crate) struct OpenedBrowserFeedSession {
    pub(crate) handle: FeedHandle,
    runtime: BrowserFeedSessionRuntime,
}

impl OpenedBrowserFeedSession {
    pub(crate) fn sync_identity_change(&self) {
        self.runtime.sync_identity_change();
    }
}

struct BrowserFeedSessionRuntime {
    follow_observer: ObservedProjectionReconciler,
    follow_set: Arc<nmp_nip02::ActiveFollowSet>,
    _engine: Arc<nmp_note_feed::op_feed::OpFeedEngine>,
}

impl BrowserFeedSessionRuntime {
    fn sync_identity_change(&self) {
        self.follow_set.notify_account_changed();
        self.follow_observer.sync();
    }
}

pub(crate) fn open_browser_feed_session(
    sessions: &FeedSessionRegistry,
    access: FeedRuntimeAccess<'_>,
    params: FeedParams,
) -> Option<OpenedBrowserFeedSession> {
    if params.render != FeedRender::OpCentric
        || params.acquisition != PubkeySetExpr::ActiveUserFollows
        || params.admission != FeedAdmission::All
        || params.ranking != FeedRanking::ChronologicalDesc
    {
        return None;
    }

    let acquisition_kinds =
        nmp_nip18::validate_primary_kinds(params.primary_kinds.iter().copied()).ok()?;
    let projection = params.projection.clone();

    let (runtime, build) = compile_feed(access, acquisition_kinds, projection.clone());
    let session_id = sessions.open(build);
    if session_id.0 == 0 {
        return None;
    }

    Some(OpenedBrowserFeedSession {
        handle: FeedHandle {
            projection_key: projection,
            session_id,
        },
        runtime,
    })
}

fn compile_feed(
    access: FeedRuntimeAccess<'_>,
    acquisition_kinds: BTreeSet<u32>,
    projection: ProjectionKey,
) -> (BrowserFeedSessionRuntime, FeedSessionBuild) {
    let active_account_slot = access.reducer.active_account_handle();
    let event_store = access.reducer.event_store_handle();
    let follow_store_slot = slots::new_event_store_slot();
    if let Ok(mut slot) = follow_store_slot.lock() {
        *slot = Some(Arc::clone(&event_store));
    }
    let registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync> =
        Arc::new(access.observed_projection_registrar.clone());

    let follow_set = nmp_nip02::ActiveFollowSet::new(
        active_account_slot.clone(),
        nmp_nip02::LatestKind3FollowSet::new(follow_store_slot.clone()),
    );
    let event_lookup: nmp_feed::EventLookup =
        Arc::new(move |id| slots::event_by_id_from_arc(&event_store, id));

    let engine = register_op_feed(
        String::new(),
        follow_set.predicate(),
        Arc::clone(&event_lookup),
    );
    let observer = op_feed_observer(
        Arc::clone(&engine),
        event_lookup,
        Arc::new(EmptySuppressionLookup),
    );

    let follow_observer = ObservedProjectionReconciler::new(
        Arc::clone(&registrar),
        follow_set.clone() as Arc<dyn ObservedProjectionSink>,
        format!("{}.follow_set", projection.0),
        1,
        64,
        active_contact_list_shape(active_account_slot.clone()),
    );
    let feed_observer = ObservedProjectionReconciler::new(
        registrar,
        observer as Arc<dyn ObservedProjectionSink>,
        format!("{}.engine", projection.0),
        1,
        512,
        active_follow_feed_shape(
            active_account_slot,
            Arc::clone(&follow_set),
            acquisition_kinds.clone(),
        ),
    );

    // Eager sync for cold-start: the account may already be set. Without an
    // active account, ActiveUserFollows fails closed until the graph source
    // effect from sign-in reconciles this observer.
    follow_observer.sync();
    feed_observer.sync();

    let engine_for_source_effect = Arc::clone(&engine);
    let feed_for_source_effect = feed_observer.clone();
    let notify_for_source_effect = access.command_sender.clone();
    follow_set.on_source_effect(Box::new(move |_| {
        feed_for_source_effect.sync();
        let had_rows = !engine_for_source_effect
            .snapshot_current_window()
            .cards
            .is_empty();
        engine_for_source_effect.reset_for_perspective_change();
        if had_rows {
            notify_for_source_effect.mark_changed_since_emit();
        }
    }));

    register_feed_render_source(access.reducer, projection.0.clone(), Arc::clone(&engine));

    let teardown: Vec<TeardownAction> = vec![
        mark_changed(access.command_sender.clone()),
        access
            .reducer
            .remove_feed_snapshot_projection_action(projection.0.clone()),
        close_reconciler(feed_observer.clone()),
        close_reconciler(follow_observer.clone()),
    ];

    (
        BrowserFeedSessionRuntime {
            follow_observer,
            follow_set,
            _engine: engine,
        },
        FeedSessionBuild {
            projection_key: projection,
            teardown,
        },
    )
}

fn register_feed_render_source(
    reducer: &nmp_core::KernelReducer,
    key: String,
    engine: Arc<nmp_note_feed::op_feed::OpFeedEngine>,
) {
    let source = FeedRenderSource::new(move || engine.snapshot_current_window());
    let Some((tick_rev, emitted_sink)) = reducer.feed_render_source_handles() else {
        return;
    };

    let source_for_typed = Arc::clone(&source);
    let tick_rev_for_typed = Arc::clone(&tick_rev);
    let consumer_for_typed = format!("feed-author:{key}");
    let typed_key = key.clone();
    reducer.register_typed_snapshot_projection(key.clone(), move || {
        let rev = tick_rev_for_typed.load(std::sync::atomic::Ordering::Acquire);
        let snapshot = source_for_typed.snapshot_for_tick(rev);
        nmp_core::record_emitted_feed_authors(
            &emitted_sink,
            rev,
            consumer_for_typed.clone(),
            snapshot.visible_author_keys(),
        );
        Some(TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_note_feed::op_feed::OP_FEED_SCHEMA_ID.to_string(),
            schema_version: nmp_note_feed::op_feed::OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                nmp_note_feed::op_feed::OP_FEED_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: nmp_note_feed::op_feed::encode_op_feed_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let source_for_provider = source;
    reducer.register_feed_author_provider(key, move || {
        let rev = tick_rev.load(std::sync::atomic::Ordering::Acquire);
        source_for_provider.author_keys_for_tick(rev)
    });
}

fn close_reconciler(reconciler: ObservedProjectionReconciler) -> TeardownAction {
    Box::new(move || {
        reconciler.close_current();
    })
}

fn mark_changed(sender: CommandSender) -> TeardownAction {
    Box::new(move || {
        sender.mark_changed_since_emit();
    })
}

fn active_contact_list_shape(active_account_slot: nmp_core::slots::ActiveAccountSlot) -> LiveShape {
    Arc::new(move || {
        let active = active_account_slot.lock().ok()?.clone()?;
        Some(InterestShape {
            authors: [active].into_iter().collect(),
            kinds: [nmp_kinds::KIND_CONTACT_LIST].into_iter().collect(),
            ..Default::default()
        })
    })
}

fn active_follow_feed_shape(
    active_account_slot: nmp_core::slots::ActiveAccountSlot,
    follow_set: Arc<nmp_nip02::ActiveFollowSet>,
    acquisition_kinds: BTreeSet<u32>,
) -> LiveShape {
    Arc::new(move || {
        let active = active_account_slot.lock().ok().and_then(|g| g.clone())?;
        let mut authors: BTreeSet<String> = follow_set.follows().into_iter().collect();
        authors.insert(active);
        Some(InterestShape::timeline_for(
            authors,
            acquisition_kinds.clone(),
        ))
    })
}
