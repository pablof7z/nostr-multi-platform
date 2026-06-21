//! `load_older` / viewport-growth tests for interest feeds.
//!
//! Verifies that `load_older_feed` drains a page over the pull substrate and
//! grows the visible window past the first page boundary.

use super::super::*;
use super::harness::*;
use std::ffi::CString;

use nmp_ffi::nmp_app_free;
use nostr::{EventBuilder, Keys, Timestamp};
use nostr::prelude::JsonUtil;

/// `load_older` drains a page over the pull substrate and grows the viewport.
#[test]
fn author_feed_load_older_grows_visible_window_past_first_page() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    let total = nmp_feed::DEFAULT_FEED_WINDOW_LIMIT + 20;
    let mut ids_in_order: Vec<String> = Vec::new();
    for i in 0..total {
        let ev = EventBuilder::text_note(format!("n{i}"))
            .custom_created_at(Timestamp::from(1_000u64 + i as u64))
            .sign_with_keys(&keys)
            .expect("sign");
        let id = ev.id.to_hex();
        inject_and_wait(app, &ev.as_json(), &id, &rx);
        ids_in_order.push(id);
    }

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);

    // First page: DEFAULT_FEED_WINDOW_LIMIT events.
    let first = wait_for_feed_cards(app, &key, nmp_feed::DEFAULT_FEED_WINDOW_LIMIT, &rx);
    assert_eq!(
        first.len(),
        nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        "sidecar emits only the first page before load_older"
    );

    let app_ref: &NmpApp = unsafe { &*app };
    assert!(
        app_ref.load_older_feed(&key),
        "load_older must drain + grow the viewport"
    );
    let grown = wait_for_feed_cards(app, &key, total, &rx);
    assert!(
        grown.len() > first.len(),
        "the emitted projection grew after load_older"
    );

    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    nmp_app_free(app);
}
