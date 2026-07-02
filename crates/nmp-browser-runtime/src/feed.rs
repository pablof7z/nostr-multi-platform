//! Browser-runtime feed-session adapter.
//!
//! Browser owns runtime lifecycle and projection registry access, not a second
//! feed-source compiler. Feed-scope reduction, source effects, acquisition
//! replacement, OP/flat session wiring, and teardown recipes come from
//! `nmp-feed-session`.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use nmp_core::substrate::ObservedProjectionCommandHandle;
use nmp_core::{CommandSender, KernelReducer, TypedProjectionData};
use nmp_feed::{
    CustomPerspectiveDef, CustomPerspectiveId, FeedAuthorRefs, FeedController, FeedHandle,
    FeedParams, FeedSessionRegistry, FeedWindowSource, PullFn, TeardownAction,
};
use nmp_feed_session::{FeedSessionHost, IdentityChangeObserverId};
use nmp_store::{PullPage, ScanLogResult};

use crate::runtime::{
    unregister_identity_observer, BrowserIdentityObserverFn, BrowserIdentityObserverRegistration,
    BrowserIdentityObserverSlot,
};

pub(crate) struct FeedRuntimeAccess<'a> {
    pub(crate) reducer: &'a KernelReducer,
    pub(crate) observed_projection_registrar: ObservedProjectionCommandHandle,
    pub(crate) command_sender: CommandSender,
    pub(crate) feed_registry: nmp_feed::FeedRegistrySlot,
    pub(crate) identity_observers: BrowserIdentityObserverSlot,
    pub(crate) identity_observer_next_id: Arc<AtomicU64>,
    event_store_slot: nmp_core::slots::EventStoreSlot,
}

impl<'a> FeedRuntimeAccess<'a> {
    pub(crate) fn new(
        reducer: &'a KernelReducer,
        observed_projection_registrar: ObservedProjectionCommandHandle,
        command_sender: CommandSender,
        feed_registry: nmp_feed::FeedRegistrySlot,
        identity_observers: BrowserIdentityObserverSlot,
        identity_observer_next_id: Arc<AtomicU64>,
    ) -> Self {
        let event_store_slot = nmp_core::slots::new_event_store_slot();
        if let Ok(mut slot) = event_store_slot.lock() {
            *slot = Some(reducer.event_store_handle());
        }
        Self {
            reducer,
            observed_projection_registrar,
            command_sender,
            feed_registry,
            identity_observers,
            identity_observer_next_id,
            event_store_slot,
        }
    }
}

pub(crate) struct OpenedBrowserFeedSession {
    pub(crate) handle: FeedHandle,
}

pub(crate) fn open_browser_feed_session(
    sessions: &FeedSessionRegistry,
    access: FeedRuntimeAccess<'_>,
    params: FeedParams,
) -> Option<OpenedBrowserFeedSession> {
    let acquisition_kinds =
        nmp_nip18::validate_primary_kinds(params.primary_kinds.iter().copied()).ok()?;
    let projection = params.projection.clone();
    let build = nmp_feed_session::compile_feed_params(&access, &params, &acquisition_kinds).ok()?;
    let session_id = sessions.open(build);
    if session_id.0 == 0 {
        return None;
    }

    Some(OpenedBrowserFeedSession {
        handle: FeedHandle {
            projection_key: projection,
            session_id,
        },
    })
}

impl FeedSessionHost for FeedRuntimeAccess<'_> {
    fn active_account_handle(&self) -> nmp_core::slots::ActiveAccountSlot {
        self.reducer.active_account_handle()
    }

    fn event_store_handle(&self) -> nmp_core::slots::EventStoreSlot {
        Arc::clone(&self.event_store_slot)
    }

    fn observed_projection_handle(&self) -> ObservedProjectionCommandHandle {
        self.observed_projection_registrar.clone()
    }

    fn register_identity_change_observer<F>(&self, callback: F) -> IdentityChangeObserverId
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        let id = self
            .identity_observer_next_id
            .fetch_add(1, Ordering::Relaxed);
        let callback: BrowserIdentityObserverFn = Arc::new(callback);
        if let Ok(mut observers) = self.identity_observers.lock() {
            observers.push(BrowserIdentityObserverRegistration { id, callback });
        }
        id
    }

    fn unregister_identity_change_observer_action(
        &self,
        id: IdentityChangeObserverId,
    ) -> TeardownAction {
        let observers = Arc::clone(&self.identity_observers);
        Box::new(move || unregister_identity_observer(&observers, id))
    }

    fn feed_pull_fn(&self) -> PullFn {
        let slot = Arc::clone(&self.event_store_slot);
        let max_entries =
            NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let max_scan = NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE.saturating_mul(8))
            .unwrap_or(NonZeroUsize::MIN);
        let limits = nmp_core::PullLimits {
            max_entries,
            max_scan_entries: max_scan,
        };

        Arc::new(move |scope: nmp_core::PullScope, after_seq: u64| {
            let exhausted = || {
                ScanLogResult::Page(PullPage {
                    entries: Vec::new(),
                    next_after_seq: after_seq,
                    latest_seq: after_seq,
                    has_more: false,
                })
            };
            let store = {
                let Ok(guard) = slot.lock() else {
                    return exhausted();
                };
                match guard.as_ref() {
                    Some(store) => Arc::clone(store),
                    None => return exhausted(),
                }
            };
            nmp_core::pull_page_over(store.as_ref(), scope, after_seq, limits)
                .unwrap_or_else(|_| exhausted())
        })
    }

    fn command_sender(&self) -> CommandSender {
        self.command_sender.clone()
    }

    fn register_feed(&self, key: String, controller: Arc<dyn FeedController>) {
        self.feed_registry.register(key, controller);
    }

    fn load_older_feed(&self, key: &str) -> bool {
        let changed = self.feed_registry.load_older(key);
        if changed {
            self.command_sender.mark_changed_since_emit();
        }
        changed
    }

    fn register_feed_window_source<S, F>(
        &self,
        feed_key: String,
        source: Arc<FeedWindowSource<S>>,
        encode: F,
    ) where
        S: FeedAuthorRefs + Send + Sync + 'static,
        F: Fn(&S) -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
        register_feed_window_source(self.reducer, feed_key, source, encode);
    }

    fn custom_perspective(&self, _id: &CustomPerspectiveId) -> Option<CustomPerspectiveDef> {
        None
    }

    fn unregister_feed_action(&self, key: String) -> TeardownAction {
        let registry = Arc::clone(&self.feed_registry);
        Box::new(move || {
            let _ = registry.unregister(&key);
        })
    }

    fn remove_projection_action(&self, key: String) -> TeardownAction {
        self.reducer.remove_feed_snapshot_projection_action(key)
    }

    fn mark_changed_action(&self) -> TeardownAction {
        let sender = self.command_sender.clone();
        Box::new(move || sender.mark_changed_since_emit())
    }
}

fn register_feed_window_source<S, F>(
    reducer: &KernelReducer,
    key: String,
    source: Arc<FeedWindowSource<S>>,
    encode: F,
) where
    S: FeedAuthorRefs + Send + Sync + 'static,
    F: Fn(&S) -> Option<TypedProjectionData> + Send + Sync + 'static,
{
    let Some((tick_rev, emitted_sink)) = reducer.feed_window_source_handles() else {
        return;
    };

    let source_for_typed = Arc::clone(&source);
    let tick_rev_for_typed = Arc::clone(&tick_rev);
    let consumer_for_typed = format!("feed-author:{key}");
    let typed_key = key.clone();
    let Ok(registration_key) = nmp_ownership::DynamicProjectionKey::app_owned(key.clone()) else {
        return;
    };
    reducer.register_typed_snapshot_projection(registration_key, move || {
        let rev = tick_rev_for_typed.load(Ordering::Acquire);
        let snapshot = source_for_typed.snapshot_for_tick(rev);
        nmp_core::record_emitted_feed_authors(
            &emitted_sink,
            rev,
            consumer_for_typed.clone(),
            snapshot.visible_author_keys(),
        );
        encode(&snapshot).map(|mut data| {
            if data.key.is_empty() {
                data.key = typed_key.clone();
            }
            data
        })
    });

    let source_for_provider = source;
    reducer.register_feed_author_provider(key, move || {
        let rev = tick_rev.load(Ordering::Acquire);
        source_for_provider.author_keys_for_tick(rev)
    });
}
