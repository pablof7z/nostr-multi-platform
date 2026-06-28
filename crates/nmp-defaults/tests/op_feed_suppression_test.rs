//! OP-feed suppression composition tests.

use std::sync::Mutex;

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::ObservedProjectionSink;
mod common;
use common::*;

static SERIAL: Mutex<()> = Mutex::new(());

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000abc";

fn op_event(id: &str, author: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: Vec::new(),
        content: "visible note".to_string(),
        relay_provenance: Vec::new(),
    }
}

fn kind3(author: &str, follows: &[&str]) -> KernelEvent {
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000003".to_string(),
        ),
        author: author.to_string(),
        kind: 3,
        created_at: 100,
        tags: follows
            .iter()
            .map(|pk| vec!["p".to_string(), (*pk).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn mute_event(author: &str, muted: &[&str]) -> KernelEvent {
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000001000".to_string(),
        ),
        author: author.to_string(),
        kind: 10000,
        created_at: 200,
        tags: muted
            .iter()
            .map(|pk| vec!["p".to_string(), (*pk).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn set_app_active(app: *mut NmpApp, active: Option<&str>) {
    let handle = unsafe { &*app }.active_account_handle();
    *handle.lock().expect("active-account slot") = active.map(str::to_string);
}

#[test]
fn mute_replacement_resets_visible_op_feed_immediately() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = new_app_ptr();
    assert!(!app.is_null(), "nmp_app_new returned null");
    set_app_active(app, Some(ALICE));

    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults::default(),
    );
    let mute = handles.mute.expect("social defaults install mute runtime");
    let defaults = nmp_native_runtime::register_op_feed_defaults_with_mute(
        unsafe { &*app },
        ALICE.to_string(),
        vec![1],
        mute.clone(),
    );

    defaults.follow_set.on_kernel_event(&kind3(ALICE, &[BOB]));
    defaults.engine.on_kernel_event(&op_event(OP_ID, BOB, 300));
    assert_eq!(
        defaults
            .engine
            .snapshot(&nmp_feed::FeedRequest::default())
            .cards
            .len(),
        1,
        "precondition: Bob's row is visible before Alice mutes Bob"
    );

    mute.on_kernel_event(&mute_event(ALICE, &[BOB]));
    assert!(
        defaults
            .engine
            .snapshot(&nmp_feed::FeedRequest::default())
            .cards
            .is_empty(),
        "active-account mute replacement must reset the current feed window immediately"
    );

    free_app_ptr(app);
}
