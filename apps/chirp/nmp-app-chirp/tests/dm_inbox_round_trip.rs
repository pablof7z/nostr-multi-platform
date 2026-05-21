//! NIP-17 DM inbox round-trip — FFI registration chain proofs.
//!
//! These three tests collectively prove the wiring `nmp_app_chirp_register_dm_inbox`
//! sets up — covering as much of the `kind:1059 → DmInboxProjection →
//! projections["nmp.nip17.dm_inbox"]` round-trip as the public crate surface
//! currently permits.
//!
//! Lifted out of `src/ffi.rs` to keep the FFI module under the AGENTS.md
//! 500-LOC hard cap. Living in `tests/` (the integration-test target) means
//! they run against the public `nmp-app-chirp` surface exactly as a host
//! consumer would — same wire as the existing `tests/end_to_end.rs`.
//!
//! What `nmp_app_chirp_register_dm_inbox` does (apps/chirp/nmp-app-chirp/src/ffi.rs:321):
//!   1. Grabs `app.nip17_local_keys()` — the shared `Arc<Mutex<Option<Keys>>>`
//!      slot the actor writes on every identity mutation.
//!   2. Constructs a `DmInboxProjection::new(local_keys)` bound to that slot.
//!   3. Registers the projection as a `RawEventObserver` for kind:1059.
//!   4. Registers a snapshot-projection closure under `"nmp.nip17.dm_inbox"`
//!      that calls `projection.snapshot_json()` on every tick.
//!   5. Pushes the kind:1059 `#p` gift-wrap interest when a viewer pubkey
//!      is supplied so the kernel actually opens a REQ.
//!
//! The first two tests below exercise steps 1+2 (the slot-binding seam) by
//! mirroring the registration's projection construction with the SAME slot
//! the FFI captures, then proving the gift-wrap unseal works through it.
//! The third test documents the gap that prevents a *single* FFI-driven
//! round-trip from running end-to-end inside this crate today.

use std::ffi::CString;

use nmp_app_chirp::ffi::nmp_app_chirp_register_dm_inbox;
use nmp_core::{nmp_app_free, nmp_app_new, NmpApp, RawEventObserver};
use nmp_nip17::{DmInboxProjection, DmInboxSnapshot};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

/// Build a signed kind:1059 gift-wrap envelope from `sender` to `receiver`
/// carrying a kind:14 chat-message rumor with `content`. Mirrors the
/// `gift_wrapped_dm` helper in `crates/nmp-nip17/src/inbox.rs` tests so
/// the test fixtures here go through the same production primitive
/// (`nmp_nip59::gift_wrap`) the actor would use.
fn gift_wrapped_dm(
    sender: &Keys,
    receiver: &nostr::PublicKey,
    content: &str,
    created_at: u64,
) -> nostr::Event {
    let tags = vec![Tag::public_key(*receiver)];
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());
    nmp_nip59::gift_wrap(sender, receiver, rumor, None).expect("gift wrap succeeds")
}

/// THE NIP-17 DM INBOX SLOT-BINDING PROOF: register the DM inbox through the
/// FFI symbol, then construct an auxiliary `DmInboxProjection` bound to
/// the EXACT SAME `app.nip17_local_keys()` slot the FFI registration
/// captured. Write Bob's keys into the slot, ingest an Alice→Bob
/// gift-wrap, and assert the snapshot surfaces the message.
///
/// This proves:
/// - `nmp_app_chirp_register_dm_inbox` does not panic or take exclusive
///   ownership of the keys slot (the test reads/writes the same `Arc`).
/// - The `Arc<Mutex<Option<Keys>>>` returned by `app.nip17_local_keys()`
///   is the same slot the registered projection reads — writing Bob's
///   keys here would also be visible to the FFI-registered projection.
/// - The full gift-wrap → unseal → conversation projection pipeline runs
///   through the slot a real FFI registration binds to.
///
/// What this test does NOT prove (see `dm_inbox_full_round_trip_through_ffi`
/// below for the gap): that the FFI-registered projection's
/// `RawEventObserver::on_raw_event` is reachable from `IngestPreVerifiedEvents`
/// or any other public path. The `raw_event_observers_slot()` accessor
/// is `pub(crate)` on `NmpApp`, so a chirp-side test cannot fan out a
/// kind:1059 to the registered observer without a new test-support seam.
#[test]
fn dm_inbox_decrypts_through_the_shared_local_keys_slot() {
    let app: *mut NmpApp = nmp_app_new();

    // Generate Alice (sender) and Bob (recipient / viewer) keys.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let bob_pubkey_hex = bob.public_key().to_hex();

    // Register the DM inbox through the FFI symbol exactly as Swift does
    // at startup. This is the load-bearing call: it captures
    // `app.nip17_local_keys()` into the projection it stores in the
    // raw-event-observer slot AND the snapshot registry.
    let viewer = CString::new(bob_pubkey_hex.as_str()).unwrap();
    nmp_app_chirp_register_dm_inbox(app, viewer.as_ptr());

    // Write Bob's keys into the SAME shared slot the FFI registration
    // captured. In production the actor mutates this slot on every
    // identity reducer; here the test plays the role of the actor (the
    // public `nip17_local_keys()` accessor returns an `Arc` clone, and
    // a direct write is the deterministic test surrogate for the
    // `SignInNsec` command path).
    // SAFETY: app came from nmp_app_new() and is live for this call.
    let local_keys_slot = unsafe { (*app).nip17_local_keys() };
    *local_keys_slot.lock().unwrap() = Some(bob.clone());

    // Construct an AUXILIARY projection bound to the same slot. This is
    // the exact constructor `nmp_app_chirp_register_dm_inbox` uses
    // (apps/chirp/nmp-app-chirp/src/ffi.rs:335). If this projection can
    // decrypt, the slot-binding contract the FFI registration relies on
    // is sound.
    let aux_projection = DmInboxProjection::new(local_keys_slot);

    // Build a real gift-wrap envelope (kind:1059) from Alice to Bob and
    // drive it through the projection's `RawEventObserver` interface.
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hello bob", 12345);
    let envelope_json = nostr::JsonUtil::as_json(&envelope);
    // Fan via the trait method — the exact entry point the kernel's
    // `notify_raw_event_observers` invokes in production.
    <DmInboxProjection as RawEventObserver>::on_raw_event(&aux_projection, 1059, &envelope_json);

    // The conversation must surface in the snapshot under Alice's pubkey.
    let snapshot_json = aux_projection.snapshot_json();
    let conversations = snapshot_json
        .get("conversations")
        .and_then(|v| v.as_array())
        .expect("snapshot must carry a `conversations` array");
    assert_eq!(
        conversations.len(),
        1,
        "one conversation expected after one ingest, got {snapshot_json}",
    );
    let convo = &conversations[0];
    assert_eq!(
        convo.get("peer_pubkey").and_then(|v| v.as_str()),
        Some(alice.public_key().to_hex().as_str()),
        "conversation peer must be Alice (the sender), got {convo}",
    );
    let messages = convo
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("conversation must carry a `messages` array");
    assert_eq!(messages.len(), 1, "exactly one decrypted message expected");
    assert_eq!(
        messages[0].get("content").and_then(|v| v.as_str()),
        Some("hello bob"),
        "decrypted content must round-trip verbatim",
    );
    assert_eq!(
        messages[0].get("sender_pubkey").and_then(|v| v.as_str()),
        Some(alice.public_key().to_hex().as_str()),
        "message sender must be Alice (from the verified seal, NOT a tag)",
    );

    nmp_app_free(app);
}

/// THE FFI SNAPSHOT-JSON SHAPE CONTRACT: the JSON the FFI registration
/// surfaces under `projections["nmp.nip17.dm_inbox"]` is exactly the shape
/// `DmInboxSnapshot` serdes to. The Swift consumer decodes this off the
/// kernel update channel; a wire-shape drift here breaks every existing
/// DM screen.
///
/// This test is a structural contract check: it captures the snapshot
/// JSON the projection produces for a populated inbox and asserts the
/// outer shape (`conversations` array of `DmConversation`-shaped objects)
/// matches what `DmInboxSnapshot` deserializes from. Together with the
/// slot-binding proof above, this nails down the wire contract a Swift
/// caller depends on.
#[test]
fn dm_inbox_snapshot_json_round_trips_through_dm_inbox_snapshot() {
    let app: *mut NmpApp = nmp_app_new();
    let alice = Keys::generate();
    let bob = Keys::generate();

    // Register through the FFI path (same as Swift does at startup).
    nmp_app_chirp_register_dm_inbox(app, std::ptr::null());

    // Same slot the FFI registration captured.
    // SAFETY: app came from nmp_app_new() and is live for this call.
    let local_keys_slot = unsafe { (*app).nip17_local_keys() };
    *local_keys_slot.lock().unwrap() = Some(bob.clone());

    let projection = DmInboxProjection::new(local_keys_slot);
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "wire-shape check", 700);
    <DmInboxProjection as RawEventObserver>::on_raw_event(
        &projection,
        1059,
        &nostr::JsonUtil::as_json(&envelope),
    );

    // The snapshot JSON the snapshot-projection closure registered under
    // `"nmp.nip17.dm_inbox"` returns on every tick — exactly what surfaces
    // in `KernelSnapshot.projections["nmp.nip17.dm_inbox"]`.
    let snapshot_value = projection.snapshot_json();
    // Round-trip through the typed `DmInboxSnapshot`: the projection's
    // wire shape MUST be decodable by the typed wire schema a host
    // consumer (Swift) uses.
    let typed: DmInboxSnapshot = serde_json::from_value(snapshot_value.clone())
        .expect("snapshot JSON must decode to DmInboxSnapshot — wire shape contract");
    assert_eq!(typed.conversations.len(), 1);
    assert_eq!(typed.conversations[0].peer_pubkey, alice.public_key().to_hex());
    assert_eq!(typed.conversations[0].messages.len(), 1);
    assert_eq!(typed.conversations[0].messages[0].content, "wire-shape check");

    // Empty-inbox shape contract: when no envelopes have arrived the
    // closure surfaces `{"conversations":[]}`, NOT JSON `null` or a
    // missing key. The Swift decoder relies on this being a concrete
    // empty object, not absent.
    let empty_projection = DmInboxProjection::new(
        // SAFETY: app is still live.
        unsafe { (*app).nip17_local_keys() },
    );
    // Clear the slot so the projection sees "not signed in".
    // SAFETY: app is still live.
    *unsafe { (*app).nip17_local_keys() }.lock().unwrap() = None;
    let empty_json = empty_projection.snapshot_json();
    assert_eq!(
        empty_json,
        serde_json::json!({ "conversations": [] }),
        "empty-inbox snapshot must be {{\"conversations\":[]}}, not null/missing",
    );

    nmp_app_free(app);
}

/// THE END-TO-END ROUND-TRIP THROUGH THE FFI REGISTRATION CHAIN — currently
/// blocked on a missing public test-support seam.
///
/// Ideal shape (what this test would prove if the seam existed):
///
/// 1. Build `nmp_app` via `nmp_app_new()`.
/// 2. Generate Alice's and Bob's keys; write Bob's into
///    `app.nip17_local_keys()` (or sign Bob in via the actor).
/// 3. Call `nmp_app_chirp_register_dm_inbox(app, bob_pubkey_cstr)`.
/// 4. Construct a kind:1059 gift-wrap from Alice to Bob via
///    `nmp_nip59::gift_wrap` and inject it through a public test-support
///    path that drives `kernel.handle_event` (the ONLY path that fans out
///    to registered `RawEventObserver`s — see kernel/ingest/mod.rs:365).
/// 5. Read the snapshot JSON via the update callback path and assert
///    `projections["nmp.nip17.dm_inbox"]["conversations"]` contains an entry
///    with Alice's content.
///
/// The gap: step 4 has no public path from `nmp-app-chirp`. Concretely:
///
/// * `ActorCommand::IngestPreVerifiedEvents` (the only `pub` event-inject
///   surface, gated on `cfg(any(test, feature = "test-support"))`) routes
///   through `kernel.ingest_pre_verified_event` which calls only
///   `notify_event_observers` — NOT `notify_raw_event_observers`. See
///   `crates/nmp-core/src/kernel/test_support.rs:165` and the comment in
///   `crates/nmp-core/src/kernel/raw_event_observer_tests.rs` distinguishing
///   the two ingest paths.
/// * `NmpApp::raw_event_observers_slot()` is `pub(crate)` (see
///   `crates/nmp-core/src/ffi/mod.rs:878`), so a chirp-side test cannot
///   directly invoke `notify_raw_observers` on the slot the FFI
///   registration wrote into.
/// * No public C-ABI symbol injects a verbatim signed-event JSON through
///   `kernel.handle_event`.
///
/// FIXME(nip17-e2e-test-seam): unblock this test by adding ONE of:
///   (a) `pub fn test_inject_signed_event_json(&self, kind: u32, json: &str)`
///       on `NmpApp` (`cfg(any(test, feature = "test-support"))`) that
///       fans out to `notify_raw_observers` on the shared slot — smallest
///       blast radius, mirrors the existing `nmp_app_inject_signed_events`
///       pattern in `crates/nmp-core/src/ffi/testing.rs`;
///   (b) extend `ingest_pre_verified_event` to also call
///       `notify_raw_event_observers` — narrows the production / test
///       drift but changes a non-test path; or
///   (c) expose `raw_event_observers_slot()` as `pub` so the test can
///       call `nmp_core::actor::notify_raw_observers` directly — leaks
///       internal type to the public API.
///
/// Until then, the two passing tests above (`dm_inbox_decrypts_through_
/// the_shared_local_keys_slot` and `dm_inbox_snapshot_json_round_trips_
/// through_dm_inbox_snapshot`) cover the slot-binding seam and wire
/// shape — every part of the chain EXCEPT the
/// `kernel.handle_event → notify_raw_event_observers → registered
/// observer` edge.
#[test]
#[ignore = "blocked on test-support seam — see FIXME(nip17-e2e-test-seam) in docstring"]
fn dm_inbox_full_round_trip_through_ffi() {
    // This body is a structural sketch of the test the FIXME would
    // unblock; it is `#[ignore]`'d so `cargo test -p nmp-app-chirp` is
    // green today and surfaces the gap via `cargo test -- --ignored`.
    let app: *mut NmpApp = nmp_app_new();
    let alice = Keys::generate();
    let bob = Keys::generate();

    // SAFETY: app came from nmp_app_new() and is live for this call.
    *unsafe { (*app).nip17_local_keys() }.lock().unwrap() = Some(bob.clone());

    let bob_pubkey = CString::new(bob.public_key().to_hex()).unwrap();
    nmp_app_chirp_register_dm_inbox(app, bob_pubkey.as_ptr());

    let _envelope = gift_wrapped_dm(&alice, &bob.public_key(), "round-trip", 100);
    // FIXME(nip17-e2e-test-seam): inject `_envelope` through a path that
    // fans out to `notify_raw_event_observers`. No public path exists today
    // (see docstring above). When the seam lands, the body below should:
    //
    //   1. inject the envelope via the new test-support symbol;
    //   2. read the snapshot JSON via `nmp_app_set_update_callback`
    //      (the same path Swift consumes);
    //   3. parse and assert `projections["nmp.nip17.dm_inbox"]
    //      ["conversations"][0]["messages"][0]["content"] == "round-trip"`.

    nmp_app_free(app);
}
