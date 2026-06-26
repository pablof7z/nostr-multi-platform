use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static SERIAL: Mutex<()> = Mutex::new(());

struct CountingObserver(AtomicU32);

impl ObservedProjectionSink for CountingObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn event() -> KernelEvent {
    event_from("auth", 1)
}

fn event_from(author: &str, kind: u32) -> KernelEvent {
    KernelEvent {
        id: format!("id-{author}-{kind}"),
        author: author.into(),
        kind,
        created_at: 1,
        tags: vec![],
        content: "hi".into(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn test_only_rust_observer_fires_per_event() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    register_rust_observer(&slot, obs.clone());

    notify_observers(&slot, &event());
    notify_observers(&slot, &event());

    assert_eq!(obs.0.load(Ordering::SeqCst), 2);
}

#[test]
fn unregister_stops_scoped_callbacks() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    let id = register_rust_observer(&slot, obs.clone());

    notify_observers(&slot, &event());
    unregister_observer(&slot, id);
    notify_observers(&slot, &event());
    notify_observers(&slot, &event());

    assert_eq!(obs.0.load(Ordering::SeqCst), 1);
}

#[test]
fn empty_slot_is_silent() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    notify_observers(&slot, &event());
}

#[test]
fn panicking_rust_observer_isolated_from_siblings() {
    struct Boom;
    impl ObservedProjectionSink for Boom {
        fn on_kernel_event(&self, _event: &KernelEvent) {
            panic!("buggy rust observer");
        }
    }

    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    register_rust_observer(&slot, Arc::new(Boom));
    let sibling = Arc::new(CountingObserver(AtomicU32::new(0)));
    register_rust_observer(&slot, sibling.clone());

    notify_observers(&slot, &event());
    notify_observers(&slot, &event());

    assert_eq!(sibling.0.load(Ordering::SeqCst), 2);
}

#[test]
fn muted_observer_skipped_by_global_notify() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));

    register_rust_observer_muted(&slot, obs.clone());
    notify_observers(&slot, &event());
    notify_observers(&slot, &event());

    assert_eq!(obs.0.load(Ordering::SeqCst), 0);
}

#[test]
fn notify_observer_by_id_reaches_muted_observer() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    let id = register_rust_observer_muted(&slot, obs.clone());

    notify_observers(&slot, &event());
    assert_eq!(obs.0.load(Ordering::SeqCst), 0);

    assert!(notify_observer_by_id(&slot, id, &event()));
    assert_eq!(obs.0.load(Ordering::SeqCst), 1);
}

#[test]
fn activate_observer_scoped_filters_global_fanout() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    let id = register_rust_observer_muted(&slot, obs.clone());

    let shape = crate::planner::InterestShape::from_filter_json(
        r#"{"kinds":[1],"authors":["match-author"]}"#,
    )
    .expect("valid observed-projection shape");
    assert!(activate_observer_scoped(&slot, id, shape));

    notify_observers(&slot, &event_from("other-author", 1));
    notify_observers(&slot, &event_from("match-author", 2));
    assert_eq!(obs.0.load(Ordering::SeqCst), 0);

    notify_observers(&slot, &event_from("match-author", 1));
    assert_eq!(obs.0.load(Ordering::SeqCst), 1);
}

#[test]
fn activate_observer_scoped_appends_shapes_for_same_observer() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    let id = register_rust_observer_muted(&slot, obs.clone());

    let shape_a =
        crate::planner::InterestShape::from_filter_json(r#"{"kinds":[1],"authors":["author-a"]}"#)
            .expect("valid shape a");
    let shape_b =
        crate::planner::InterestShape::from_filter_json(r#"{"kinds":[1],"authors":["author-b"]}"#)
            .expect("valid shape b");

    assert!(activate_observer_scoped(&slot, id, shape_a));
    assert!(activate_observer_scoped(&slot, id, shape_b));

    notify_observers(&slot, &event_from("author-a", 1));
    notify_observers(&slot, &event_from("author-b", 1));
    notify_observers(&slot, &event_from("author-c", 1));

    assert_eq!(obs.0.load(Ordering::SeqCst), 2);
}

#[test]
fn unregister_removes_muted_observer() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    let id = register_rust_observer_muted(&slot, obs.clone());

    notify_observer_by_id(&slot, id, &event());
    assert_eq!(obs.0.load(Ordering::SeqCst), 1);

    unregister_observer(&slot, id);
    assert!(!notify_observer_by_id(&slot, id, &event()));
    assert_eq!(obs.0.load(Ordering::SeqCst), 1);
}

#[test]
fn panicking_targeted_observer_isolated() {
    struct Boom;
    impl ObservedProjectionSink for Boom {
        fn on_kernel_event(&self, _event: &KernelEvent) {
            panic!("targeted observer panics");
        }
    }

    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let boom_id = register_rust_observer_muted(&slot, Arc::new(Boom));

    assert!(notify_observer_by_id(&slot, boom_id, &event()));
}
