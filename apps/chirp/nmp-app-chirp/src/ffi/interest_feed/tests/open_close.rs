//! Open/close lifecycle tests for interest feeds.
//!
//! These tests verify that opening a feed seeds events from the kernel cache
//! and that closing the feed removes the typed projection from the sidecar.

use super::super::*;
use super::harness::*;
use std::ffi::CString;

use nmp_ffi::nmp_app_free;
use nostr::{EventBuilder, Keys, Tag, Timestamp};
use nostr::prelude::JsonUtil;

#[test]
fn author_feed_open_seeds_kind1_and_close_removes_projection() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    let e1 = EventBuilder::text_note("older")
        .custom_created_at(Timestamp::from(10u64))
        .sign_with_keys(&keys)
        .expect("sign e1");
    let e2 = EventBuilder::text_note("newer")
        .custom_created_at(Timestamp::from(20u64))
        .sign_with_keys(&keys)
        .expect("sign e2");

    inject_and_wait(app, &e1.as_json(), &e1.id.to_hex(), &rx);
    inject_and_wait(app, &e2.as_json(), &e2.id.to_hex(), &rx);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);
    let ids = wait_for_feed_cards(app, &key, 2, &rx);
    assert_eq!(ids.len(), 2, "both events present after open");

    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    assert!(
        typed_projection_is_gone(app, &key),
        "author feed projection must be gone after close"
    );
    nmp_app_free(app);
}

#[test]
fn thread_feed_open_seeds_cached_root_and_replies() {
    let (app, rx, _tx_box) = start_app();

    let root_keys = Keys::generate();
    let reply_keys = Keys::generate();

    let root_ev = EventBuilder::text_note("root")
        .custom_created_at(Timestamp::from(10u64))
        .sign_with_keys(&root_keys)
        .expect("sign root");
    let root_id = root_ev.id.to_hex();

    let reply_ev = EventBuilder::text_note("reply")
        .tags([Tag::parse(["e", &root_id]).expect("e tag")])
        .custom_created_at(Timestamp::from(20u64))
        .sign_with_keys(&reply_keys)
        .expect("sign reply");

    inject_and_wait(app, &root_ev.as_json(), &root_id, &rx);
    inject_and_wait(app, &reply_ev.as_json(), &reply_ev.id.to_hex(), &rx);

    let root_c = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());
    let key = thread_feed_key(&root_id);
    let ids = wait_for_feed_cards(app, &key, 2, &rx);
    assert!(ids.contains(&root_id), "root present");
    assert!(ids.contains(&reply_ev.id.to_hex()), "reply present");

    let root_c2 = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_close_thread_feed(app, root_c2.as_ptr());
    assert!(
        typed_projection_is_gone(app, &key),
        "thread feed projection must be gone after close"
    );
    nmp_app_free(app);
}
