//! Unit tests for the NIP-51 mute-list observed-projection reconciler.

use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{
    KernelEvent, ObservedProjection, ObservedProjectionReconciler, ObservedProjectionRegistrar,
};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_nip51::active_mute_list_interest;
use nmp_planner::InterestShape;
use nostr::Keys;

#[derive(Clone)]
struct OpenRecord {
    id: ObservedProjectionId,
    consumer_id: String,
    scope: u32,
    shape: InterestShape,
}

#[derive(Default)]
struct RecordingRegistrar {
    next_id: Mutex<u64>,
    opened: Mutex<Vec<OpenRecord>>,
    closed: Mutex<Vec<ObservedProjectionId>>,
}

impl RecordingRegistrar {
    fn opened(&self) -> Vec<OpenRecord> {
        self.opened.lock().unwrap().clone()
    }

    fn closed(&self) -> Vec<ObservedProjectionId> {
        self.closed.lock().unwrap().clone()
    }
}

impl ObservedProjectionRegistrar for RecordingRegistrar {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        let id = ObservedProjectionId(*next);
        self.opened.lock().unwrap().push(OpenRecord {
            id,
            consumer_id: decl.consumer_id,
            scope: decl.scope,
            shape: decl.replay_shapes.into_iter().next().expect("shape"),
        });
        id
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.closed.lock().unwrap().push(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        panic!("test does not request nested registrar handles")
    }
}

struct NoopSink;

impl ObservedProjectionSink for NoopSink {
    fn on_kernel_event(&self, _event: &KernelEvent) {}
}

fn controller() -> (
    ObservedProjectionReconciler,
    ActiveAccountSlot,
    Arc<RecordingRegistrar>,
) {
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let registrar = Arc::new(RecordingRegistrar::default());
    let slot = Arc::clone(&active_pubkey);
    let controller = ObservedProjectionReconciler::new(
        registrar.clone(),
        Arc::new(NoopSink),
        "nmp.nip51.mute_list",
        1,
        128,
        Arc::new(move || {
            let pubkey = slot.lock().ok()?.clone()?;
            Some(active_mute_list_interest(&pubkey).shape)
        }),
    );
    (controller, active_pubkey, registrar)
}

fn sign_in(slot: &ActiveAccountSlot, keys: &Keys) -> String {
    let pubkey = keys.public_key().to_hex();
    *slot.lock().unwrap() = Some(pubkey.clone());
    pubkey
}

#[test]
fn sign_in_opens_author_scoped_observed_projection_once() {
    let (controller, slot, registrar) = controller();
    controller.sync();
    assert!(
        registrar.opened().is_empty(),
        "no active pubkey means no open"
    );

    let pubkey = sign_in(&slot, &Keys::generate());
    controller.sync();
    let opened = registrar.opened();
    assert_eq!(opened.len(), 1);
    assert_open_for(&opened[0], &pubkey);

    controller.sync();
    assert_eq!(registrar.opened().len(), 1, "steady pubkey must idle");
}

#[test]
fn account_switch_closes_old_observer_then_opens_new_author() {
    let (controller, slot, registrar) = controller();
    sign_in(&slot, &Keys::generate());
    controller.sync();
    let first_id = registrar.opened()[0].id;

    let second = sign_in(&slot, &Keys::generate());
    controller.sync();

    assert_eq!(registrar.closed(), vec![first_id]);
    let opened = registrar.opened();
    assert_eq!(opened.len(), 2);
    assert_open_for(&opened[1], &second);
}

#[test]
fn sign_out_closes_observer_once() {
    let (controller, slot, registrar) = controller();
    sign_in(&slot, &Keys::generate());
    controller.sync();
    let first_id = registrar.opened()[0].id;

    *slot.lock().unwrap() = None;
    controller.sync();
    assert_eq!(registrar.closed(), vec![first_id]);

    controller.sync();
    assert_eq!(registrar.closed(), vec![first_id], "signed-out idle tick");
}

/// Verify the identity-change callback drives the open without any tick firing.
///
/// Wire a callback that calls `sync()` directly (mimicking
/// `register_identity_change_observer`), sign in, fire the callback once, and
/// assert `current_id()` is non-zero — no tick needed.
#[test]
fn identity_change_callback_opens_without_tick() {
    let (controller, slot, registrar) = controller();

    // Wire the identity-change callback.
    let controller_for_cb = controller.clone();
    let identity_cb = move || controller_for_cb.sync();

    // Sign in, fire the callback directly.
    sign_in(&slot, &Keys::generate());
    identity_cb();

    // The reconciler must have opened a projection — no tick fired.
    assert_ne!(
        controller.current_id().0,
        0,
        "identity-change callback must open the projection without a tick"
    );
    assert_eq!(registrar.opened().len(), 1, "exactly one open");
}

fn assert_open_for(record: &OpenRecord, pubkey: &str) {
    assert_eq!(record.consumer_id, "nmp.nip51.mute_list");
    assert_eq!(record.scope, 1, "explicit author shape is global-scoped");
    assert_eq!(
        record.shape.authors,
        [pubkey.to_string()].into_iter().collect()
    );
    assert_eq!(record.shape.kinds, [10000u32].into_iter().collect());
}
