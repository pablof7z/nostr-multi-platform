use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, LifecycleCommand};
use nmp_core::substrate::{
    KernelEvent, ObservedProjection, ObservedProjectionCommandHandle, ObservedProjectionReconciler,
};
use nmp_core::ObservedProjectionSink;
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip25::{
    encode_reaction_aggregate_snapshot, ReactionAggregateProjection, KIND_REACTION,
    KIND_REACTION_DELETE, REACTION_AGGREGATE_FILE_IDENTIFIER, REACTION_AGGREGATE_SCHEMA_ID,
    REACTION_AGGREGATE_SCHEMA_VERSION,
};
use nmp_planner::InterestShape;

use crate::app_struct::{read_active_account, NmpApp};

mod types;
pub use types::Nip25ReactionsHandle;
pub(crate) use types::ReactionReadSession;

const SCOPE_GLOBAL: u32 = 1;
static NEXT_REACTION_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

impl NmpApp {
    #[must_use]
    pub fn open_reactions(
        &self,
        target_event_id: impl Into<String>,
    ) -> Option<Nip25ReactionsHandle> {
        self.open_reactions_with_reader(target_event_id)
            .map(|(handle, _)| handle)
    }

    #[must_use]
    pub fn open_reactions_with_reader(
        &self,
        target_event_id: impl Into<String>,
    ) -> Option<(Nip25ReactionsHandle, Arc<ReactionAggregateProjection>)> {
        let target_event_id = target_event_id.into();
        if !is_hex_64(&target_event_id) {
            return None;
        }
        let projection_key = reactions_projection_key(&target_event_id);
        self.close_reactions_key(&projection_key);

        let handle_id = NEXT_REACTION_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
        let active_pubkey = read_active_account(&self.read_handles.active_account_handle);
        let projection = Arc::new(ReactionAggregateProjection::new(active_pubkey));
        let observer = Arc::new(ReactionReadObserver::new(
            target_event_id.clone(),
            Arc::clone(&projection),
        ));

        let registrar = self.observed_projection_handle();
        let delete_reconciler = build_delete_reconciler(
            &registrar,
            &projection_key,
            Arc::clone(&observer),
            &projection,
        );
        observer.set_delete_reconciler(delete_reconciler.clone());

        let observer_sink: Arc<dyn ObservedProjectionSink> = observer;
        let base_observer_id = registrar.open(ObservedProjection::from_shape(
            observer_sink,
            format!("{projection_key}.reactions"),
            SCOPE_GLOBAL,
            reactions_shape(&target_event_id),
            DEFAULT_FEED_WINDOW_LIMIT,
        ));
        if base_observer_id.0 == 0 {
            return None;
        }

        register_sidecar(self, &projection_key, Arc::clone(&projection));

        let projection_for_identity = Arc::clone(&projection);
        let sender = self.command_sender();
        let identity_observer_id = self.register_identity_change_observer(move |active_pubkey| {
            projection_for_identity.set_viewer_pubkey(active_pubkey);
            let _ = sender.send(ActorCommand::Lifecycle(
                LifecycleCommand::MarkChangedSinceEmit,
            ));
        });

        if let Ok(mut sessions) = self.reaction_read_sessions.lock() {
            sessions.insert(
                projection_key.clone(),
                ReactionReadSession {
                    projection_key: projection_key.clone(),
                    base_observer_id,
                    delete_reconciler,
                    identity_observer_id,
                    handle_id,
                },
            );
        } else {
            registrar.close(base_observer_id);
            self.unregister_identity_change_observer(identity_observer_id);
            self.remove_snapshot_projection(&projection_key);
            return None;
        }

        Some((
            Nip25ReactionsHandle {
                key: projection_key,
                target_event_id,
                handle_id,
            },
            projection,
        ))
    }

    pub fn close_reactions(&self, handle: Nip25ReactionsHandle) {
        let should_close = self
            .reaction_read_sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(&handle.key).map(|s| s.handle_id))
            .is_some_and(|open_handle_id| open_handle_id == handle.handle_id);
        if should_close {
            self.close_reactions_key(&handle.key);
        }
    }

    fn close_reactions_key(&self, key: &str) {
        let session = self
            .reaction_read_sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(key));
        let Some(session) = session else {
            return;
        };
        session.delete_reconciler.close_current();
        self.observed_projection_handle()
            .close(session.base_observer_id);
        self.unregister_identity_change_observer(session.identity_observer_id);
        self.remove_snapshot_projection(&session.projection_key);
    }
}

fn register_sidecar(app: &NmpApp, key: &str, projection: Arc<ReactionAggregateProjection>) {
    let key_for_encode = key.to_string();
    app.register_typed_snapshot_projection(
        nmp_ownership::FrameworkProjectionKey::declared(
            key.to_string(),
            "projection.nmp.nip25.reactions",
        )
        .expect("plain reaction projection keys use the nmp.nip25.reactions family"),
        move || {
            let snapshot = projection.snapshot();
            Some(nmp_core::TypedProjectionData {
                key: key_for_encode.clone(),
                schema_id: REACTION_AGGREGATE_SCHEMA_ID.to_string(),
                schema_version: REACTION_AGGREGATE_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(REACTION_AGGREGATE_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_reaction_aggregate_snapshot(&snapshot),
                ..Default::default()
            })
        },
    );
}

fn build_delete_reconciler(
    registrar: &ObservedProjectionCommandHandle,
    projection_key: &str,
    observer: Arc<ReactionReadObserver>,
    projection: &Arc<ReactionAggregateProjection>,
) -> ObservedProjectionReconciler {
    let target = observer.target_event_id.clone();
    let projection = Arc::clone(projection);
    let observer_sink: Arc<dyn ObservedProjectionSink> = observer;
    ObservedProjectionReconciler::new(
        Arc::new(registrar.clone()),
        observer_sink,
        format!("{projection_key}.deletes"),
        SCOPE_GLOBAL,
        DEFAULT_FEED_WINDOW_LIMIT,
        Arc::new(move || delete_shape(&projection, &target)),
    )
}

struct ReactionReadObserver {
    target_event_id: String,
    projection: Arc<ReactionAggregateProjection>,
    delete_reconciler: Mutex<Option<ObservedProjectionReconciler>>,
}

impl ReactionReadObserver {
    fn new(target_event_id: String, projection: Arc<ReactionAggregateProjection>) -> Self {
        Self {
            target_event_id,
            projection,
            delete_reconciler: Mutex::new(None),
        }
    }

    fn set_delete_reconciler(&self, reconciler: ObservedProjectionReconciler) {
        if let Ok(mut slot) = self.delete_reconciler.lock() {
            *slot = Some(reconciler);
        }
    }

    fn sync_deletes(&self) {
        let reconciler = self
            .delete_reconciler
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        if let Some(reconciler) = reconciler {
            reconciler.sync();
        }
    }
}

impl ObservedProjectionSink for ReactionReadObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        match event.kind {
            KIND_REACTION
                if last_tag_value(&event.tags, "e") == Some(self.target_event_id.as_str()) =>
            {
                self.projection.on_kernel_event(event);
                self.sync_deletes();
            }
            KIND_REACTION_DELETE => {
                self.projection.on_kernel_event(event);
                self.sync_deletes();
            }
            _ => {}
        }
    }
}

fn reactions_projection_key(target_event_id: &str) -> String {
    format!("nmp.nip25.reactions.{target_event_id}")
}

fn reactions_shape(target_event_id: &str) -> InterestShape {
    let mut tags = BTreeMap::new();
    tags.insert(
        "e".to_string(),
        BTreeSet::from([target_event_id.to_string()]),
    );
    InterestShape {
        kinds: BTreeSet::from([KIND_REACTION]),
        tags,
        ..Default::default()
    }
}

fn delete_shape(
    projection: &ReactionAggregateProjection,
    target_event_id: &str,
) -> Option<InterestShape> {
    let targets = projection.delete_targets_for(target_event_id);
    if targets.is_empty() {
        return None;
    }
    let mut tags = BTreeMap::new();
    tags.insert(
        "e".to_string(),
        targets
            .iter()
            .map(|target| target.reaction_event_id.clone())
            .collect(),
    );
    Some(InterestShape {
        authors: targets
            .iter()
            .map(|target| target.author_pubkey.clone())
            .collect(),
        kinds: BTreeSet::from([KIND_REACTION_DELETE]),
        tags,
        ..Default::default()
    })
}

fn is_hex_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn last_tag_value<'a>(tags: &'a [Vec<String>], name: &str) -> Option<&'a str> {
    tags.iter().rev().find_map(|tag| {
        if tag.first().is_some_and(|candidate| candidate == name) {
            tag.get(1)
                .and_then(|value| (!value.is_empty()).then_some(value.as_str()))
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "plain_reactions_tests.rs"]
mod tests;
