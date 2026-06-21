//! ADR-0063 D7 (#1671 Lane H) — structural feed-author auto-resolve tests.
//!
//! The coverage hole: a dynamic author/thread feed used to register ONLY a typed
//! sidecar and NO feed-author provider, so its rendered authors never
//! auto-resolved (blank avatars). The structural-pairing fix routes BOTH lanes
//! through `register_feed_render_source`, so a sidecar can't exist without its
//! provider. These tests prove the provider is present while the feed is open and
//! gone after close (no leak), for BOTH the author and thread feeds.

use super::super::*;
use super::harness::*;
use std::ffi::CString;

use nmp_ffi::nmp_app_free;
use nostr::{EventBuilder, Keys, Tag, Timestamp};
use nostr::prelude::JsonUtil;

/// Consumer id the kernel auto-resolves a feed's authors under.
fn feed_author_consumer(feed_key: &str) -> String {
    format!("feed-author:{feed_key}")
}

/// **Author feed structural pairing**: opening an author feed registers BOTH its
/// typed sidecar AND its feed-author provider under the same key; closing
/// removes both. The provider returns the feed's visible author (the injected
/// note's author), proving the rows auto-resolve through `resolve_ref`.
#[test]
fn author_feed_registers_author_provider_structurally_and_releases_on_close() {
    let (app, rx, _tx_box) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let ev = EventBuilder::text_note("hi")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign");
    inject_and_wait(app, &ev.as_json(), &ev.id.to_hex(), &rx);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);
    // Wait until the card is actually in the feed window (so the provider has an
    // author to surface).
    let _ = wait_for_feed_cards(app, &key, 1, &rx);

    // STRUCTURAL: both lanes are registered under the SAME key.
    assert!(
        app_ref.registered_typed_projection_keys().contains(&key),
        "author feed typed sidecar must be registered"
    );
    let providers = app_ref.registered_feed_author_provider_keys();
    assert!(
        providers.contains(&key),
        "author feed MUST have a structurally-paired author provider (was the coverage hole)"
    );

    // The provider surfaces the feed's visible author → it auto-resolves.
    assert_eq!(
        app_ref.run_feed_author_provider_for_test(&key),
        vec![pubkey.clone()],
        "the author provider returns the visible author (auto-resolved via resolve_ref)"
    );
    // The consumer id is the documented feed-author scheme.
    assert_eq!(feed_author_consumer(&key), format!("feed-author:{key}"));

    // Close → BOTH lanes gone (no leak).
    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    assert!(
        !app_ref.registered_typed_projection_keys().contains(&key),
        "typed sidecar gone after close"
    );
    assert!(
        !app_ref.registered_feed_author_provider_keys().contains(&key),
        "author provider released after close (no leak)"
    );
    nmp_app_free(app);
}

/// **Thread feed structural pairing**: same guarantee for a thread feed — its
/// rendered authors (root author + reply author) auto-resolve via a
/// structurally-paired provider, released on close.
#[test]
fn thread_feed_registers_author_provider_structurally_and_releases_on_close() {
    let (app, rx, _tx_box) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };

    let root_keys = Keys::generate();
    let reply_keys = Keys::generate();
    let root_pubkey = root_keys.public_key().to_hex();
    let reply_pubkey = reply_keys.public_key().to_hex();

    let root_ev = EventBuilder::text_note("root")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&root_keys)
        .expect("sign root");
    let root_id = root_ev.id.to_hex();
    let reply_ev = EventBuilder::text_note("reply")
        .tags([Tag::parse(["e", &root_id]).expect("e tag")])
        .custom_created_at(Timestamp::from(2_000u64))
        .sign_with_keys(&reply_keys)
        .expect("sign reply");

    inject_and_wait(app, &root_ev.as_json(), &root_id, &rx);
    inject_and_wait(app, &reply_ev.as_json(), &reply_ev.id.to_hex(), &rx);

    let root_c = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());
    let key = thread_feed_key(&root_id);
    let _ = wait_for_feed_cards(app, &key, 2, &rx);

    // STRUCTURAL pairing present.
    assert!(app_ref.registered_typed_projection_keys().contains(&key));
    assert!(
        app_ref.registered_feed_author_provider_keys().contains(&key),
        "thread feed MUST have a structurally-paired author provider"
    );

    // Both rendered authors (root + reply) are surfaced for auto-resolve.
    let authors = app_ref.run_feed_author_provider_for_test(&key);
    assert!(
        authors.contains(&root_pubkey),
        "thread root author auto-resolves (got {authors:?})"
    );
    assert!(
        authors.contains(&reply_pubkey),
        "thread reply author auto-resolves (got {authors:?})"
    );

    // Close → released.
    let root_c2 = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_close_thread_feed(app, root_c2.as_ptr());
    assert!(!app_ref.registered_typed_projection_keys().contains(&key));
    assert!(
        !app_ref.registered_feed_author_provider_keys().contains(&key),
        "thread author provider released after close (no leak)"
    );
    nmp_app_free(app);
}
