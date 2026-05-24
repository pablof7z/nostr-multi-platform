//! NIP-17 DM inbox round-trip — FFI registration chain proofs.
//!
//! These three tests collectively prove the wiring `nmp_app_chirp_register_dm_inbox`
//! sets up — covering the full `kind:1059 → DmInboxProjection →
//! projections["nmp.nip17.dm_inbox"]` round-trip through the public crate
//! surface.
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
//! The first two tests exercise steps 1+2 (the slot-binding seam) by
//! mirroring the registration's projection construction with the SAME slot
//! the FFI captures, then proving the gift-wrap unseal works through it.
//! The third test drives the full FFI-only round-trip — verbatim signed-event
//! injection through `nmp_app_inject_signed_event_json` then snapshot read
//! through `nmp_app_read_projection_json`, both gated on `test-support`.

use nmp_app_chirp::ffi::nmp_app_chirp_register_dm_inbox;
use nmp_core::RawEventObserver;
use nmp_ffi::{
    nmp_app_free, nmp_app_free_string, nmp_app_inject_signed_event_json, nmp_app_new,
    nmp_app_read_projection_json, NmpApp,
};
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
/// Complementary to `dm_inbox_full_round_trip_through_ffi` below: that test
/// proves the full FFI-driven round-trip (verbatim signed-event injection →
/// kernel ingest → `notify_raw_event_observers` → registered observer →
/// snapshot projection); this test proves the auxiliary-projection slot-
/// binding contract the FFI registration relies on.
#[test]
fn dm_inbox_decrypts_through_the_shared_local_keys_slot() {
    let app: *mut NmpApp = nmp_app_new();

    // Generate Alice (sender) and Bob (recipient / viewer) keys.
    let alice = Keys::generate();
    let bob = Keys::generate();

    // Register the DM inbox through the FFI symbol exactly as Swift does
    // at startup. This is the load-bearing call: it captures
    // `app.nip17_local_keys()` into the projection it stores in the
    // raw-event-observer slot AND the snapshot registry.
    nmp_app_chirp_register_dm_inbox(app);

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
    nmp_app_chirp_register_dm_inbox(app);

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

    // Empty-inbox shape contract: when no local keys slot is present the
    // snapshot surfaces `{"conversations":[], "remote_signer_unsupported":true}`,
    // NOT JSON `null` or a missing key. The Swift decoder relies on this
    // being a concrete object — `decodeIfPresent` handles the new field for
    // older kernels (backward compat via V-08 Stage 2 iOS fix).
    let empty_projection = DmInboxProjection::new(
        // SAFETY: app is still live.
        unsafe { (*app).nip17_local_keys() },
    );
    // Clear the slot so the projection sees "not signed in" →
    // remote_signer_unsupported surfaces as true.
    // SAFETY: app is still live.
    *unsafe { (*app).nip17_local_keys() }.lock().unwrap() = None;
    let empty_json = empty_projection.snapshot_json();
    assert_eq!(
        empty_json,
        serde_json::json!({ "conversations": [], "remote_signer_unsupported": true }),
        "empty-inbox (no local keys) must carry remote_signer_unsupported:true",
    );

    nmp_app_free(app);
}

/// THE END-TO-END ROUND-TRIP THROUGH THE FFI REGISTRATION CHAIN.
///
/// Proves the full kind:1059 gift-wrap → kernel ingest → `RawEventObserver`
/// fan-out → `DmInboxProjection` → `projections["nmp.nip17.dm_inbox"]`
/// pipeline is wired correctly, exercised entirely through the public
/// `nmp-core` FFI surface (no private types or pub(crate) accessors).
///
/// The previously-documented seam gap (`FIXME(nip17-e2e-test-seam)`) is now
/// closed by two test-support additions on `nmp-core`:
///
/// * `kernel::Kernel::ingest_pre_verified_event` (test-support path) now also
///   calls `notify_raw_event_observers` on `Inserted|Replaced` outcomes —
///   the kernel-level ingest of injected events is symmetric with the
///   production `handle_event` path.
/// * `nmp_app_inject_signed_event_json` — verbatim signed-event injector
///   that Schnorr-verifies and routes through `IngestPreVerifiedEvents`.
/// * `nmp_app_read_projection_json` — runs every registered snapshot
///   projection and returns the value at a single key as a caller-owned
///   C string (freed via `nmp_app_free_string`).
///
/// Together these let `nmp-app-chirp` (or any per-app crate) prove its
/// registered raw-event observers and snapshot projections fire end-to-end
/// without leaking any internal type onto the production FFI ABI — both
/// symbols are gated on `cfg(any(test, feature = "test-support"))`.
#[test]
fn dm_inbox_full_round_trip_through_ffi() {
    use std::ffi::{CStr, CString};
    use std::time::Duration;

    let app: *mut NmpApp = nmp_app_new();
    let alice = Keys::generate();
    let bob = Keys::generate();

    // Register the DM inbox raw-event observer AND the
    // "nmp.nip17.dm_inbox" snapshot-projection closure through the
    // production FFI symbol — exactly the call Swift makes at startup.
    nmp_app_chirp_register_dm_inbox(app);

    // Write Bob's keys into the shared NIP-17 local-keys slot. In production
    // the actor mutates this on every identity reducer; here the test plays
    // the role of the actor — same surrogate the two preceding tests use.
    //
    // NB: `nmp_app_start` is deliberately NOT called here. The actor thread
    // is already spawned by `nmp_app_new`, and `ActorCommand::IngestPreVerifiedEvents`
    // is dispatched unconditionally regardless of `*ctx.running` (see
    // `actor/dispatch.rs:958`). Calling `Start` would synchronously fire
    // `update_local_key_slots` (`actor/dispatch.rs:213`) which clobbers
    // `nip17_local_keys` with `identity.active_local_keys()` (None here, no
    // sign-in), defeating the slot write below and causing the projection
    // to surface `remote_signer_unsupported:true` instead of decrypting.
    // SAFETY: app came from nmp_app_new() and is live for this call.
    *unsafe { (*app).nip17_local_keys() }.lock().unwrap() = Some(bob.clone());

    // Build the gift-wrap envelope (kind:1059) from Alice to Bob through the
    // exact production primitive (`nmp_nip59::gift_wrap`).
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "round-trip", 100);
    let envelope_json = nostr::JsonUtil::as_json(&envelope);

    // Inject the verbatim signed event. The test-support symbol Schnorr-
    // verifies then routes through `IngestPreVerifiedEvents`, which the
    // actor dispatches to `kernel.ingest_pre_verified_event` — which now
    // fans out to `notify_raw_event_observers` so the registered
    // `DmInboxProjection` sees the kind:1059 envelope.
    let json_cstr = CString::new(envelope_json.as_str())
        .expect("envelope JSON must be NUL-free");
    // The FFI symbol is `extern "C" fn` (no `unsafe`) with
    // `#[allow(clippy::not_unsafe_ptr_arg_deref)]` — pointer validity is
    // upheld by the caller, but the call itself is language-safe.
    let ok = nmp_app_inject_signed_event_json(app, json_cstr.as_ptr());
    assert!(
        ok,
        "nmp_app_inject_signed_event_json must return true for a valid gift-wrap envelope",
    );

    // Give the actor thread time to drain the `IngestPreVerifiedEvents`
    // command. Polling matches the established pattern in
    // `tests/end_to_end.rs` (500 ms between command send and snapshot read).
    std::thread::sleep(Duration::from_millis(500));

    // Read the projection JSON via the symmetric output-side seam. The
    // function runs every registered snapshot-projection closure (same path
    // the kernel's `make_update` drives on each tick) and returns the
    // serialized value at the requested key.
    let key = CString::new("nmp.nip17.dm_inbox").expect("key must be NUL-free");
    let ptr = nmp_app_read_projection_json(app, key.as_ptr());
    assert!(
        !ptr.is_null(),
        "nmp.nip17.dm_inbox projection must be registered after \
         nmp_app_chirp_register_dm_inbox",
    );
    // SAFETY: ptr was returned by nmp_app_read_projection_json and is a
    // heap-owned, NUL-terminated UTF-8 C string. Copy out before freeing
    // so the borrow is decoupled from the allocation lifetime.
    let json_str = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("projection JSON must be valid UTF-8")
        .to_owned();
    nmp_app_free_string(ptr);

    // Decode through the typed `DmInboxSnapshot` to assert the wire shape
    // matches what Swift consumes off the kernel update channel — same
    // round-trip the second test in this file proves directly on
    // `DmInboxProjection`, now driven entirely through the public FFI.
    let snapshot: DmInboxSnapshot = serde_json::from_str(&json_str)
        .expect("nmp.nip17.dm_inbox projection must decode to DmInboxSnapshot");
    assert_eq!(
        snapshot.conversations.len(),
        1,
        "exactly one conversation expected after one ingest, got {json_str}",
    );
    let convo = &snapshot.conversations[0];
    assert_eq!(
        convo.peer_pubkey,
        alice.public_key().to_hex(),
        "conversation peer must be Alice (the sender, taken from the verified seal)",
    );
    assert_eq!(convo.messages.len(), 1, "exactly one decrypted message expected");
    assert_eq!(
        convo.messages[0].content, "round-trip",
        "decrypted content must round-trip verbatim from gift_wrap → DmInboxProjection",
    );

    nmp_app_free(app);
}
