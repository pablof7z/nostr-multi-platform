//! End-to-end regression for FINDING A — read-your-writes for follows.
//!
//! Proves the full production path: a locally-dispatched `nmp.follow` /
//! `nmp.unfollow` against a real signed-in [`NmpApp`] builds, signs, and
//! publishes a kind:3 contact list whose acceptance fans out to a registered
//! [`ObservedProjectionSink`] — so [`ActiveFollowSet`] reflects the follow AND the
//! unfollow *immediately*, without a relay round-trip or an account switch.
//!
//! Before the fix, `Kernel::record_local_contacts_intent` ingested the locally
//! published kind:3 into the contact set but never called
//! `notify_event_observers`, and the later relay echo of the same event id
//! deduped to `Duplicate` (so the relay arm's `Inserted | Replaced` fan-out gate
//! never fired). The active follow set therefore went stale after a tap until
//! restart / account-switch; an unfollowed author's replies kept qualifying.
//! This test drives that exact path through the actor thread and would hang at
//! the `recv_timeout` deadline (observer never fires) on the pre-fix code.
//!
//! # Synchronization — D8 (no polling)
//!
//! `nmp_app_new` runs the actor on a background thread, and every host command
//! (`AddSigner`, the dispatched `Follow`) is fire-and-forget over the actor's
//! command channel. The only deterministic edge back to the test is the
//! observer fan-out itself: a second `ObservedProjectionSink` signals an
//! `mpsc::Sender` from the actor thread when an accepted kind:3 arrives, and the
//! test blocks on `recv_timeout` (an OS-event wait, never a `sleep`+check spin).
//! `ActiveFollowSet` is registered on the SAME slot, so once the signal lands
//! the set is already updated.

use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::{
    ActionPayload, KernelEvent, ObservedProjection, ObservedProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;
use nmp_ffi::{
    nmp_app_dispatch_action_bytes, nmp_app_free, nmp_app_inject_signed_event_json, nmp_app_new,
    nmp_app_signin_nsec, nmp_app_start, nmp_free_string, NmpApp,
};
use nostr::prelude::*;

/// A kind:3-gated observer that signals `tx` from the actor thread each time an
/// accepted event fans out, carrying the event kind. This is the test's only
/// synchronization edge (D8 — the test blocks on `rx.recv_timeout`, never
/// polls). It forwards the kind so the test can wait for the kind:10002 relay
/// list to be ingested (establishing publish targets) before dispatching the
/// follow, then for each kind:3 contact-list fan-out.
struct KindSignal {
    tx: Mutex<mpsc::Sender<u32>>,
    kind3_count: AtomicU32,
}

impl ObservedProjectionSink for KindSignal {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind == 3 {
            self.kind3_count.fetch_add(1, Ordering::SeqCst);
        }
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(event.kind);
        }
    }
}

/// Dispatch a follow/unfollow `pubkey` action at `namespace` through the typed
/// byte doorway and assert the synchronous accept (the doorway echoes the
/// host-supplied `correlation_id`, NOT publish completion — that settles later
/// on the actor thread).
fn dispatch_ok(app: *mut NmpApp, namespace: &str, pubkey: &str) {
    let payload = nmp_nip02::PubkeyAction {
        pubkey: pubkey.to_string(),
    }
    .encode();
    let correlation_id = format!("{namespace}-{pubkey}");
    let envelope = encode_dispatch_envelope(
        &correlation_id,
        namespace,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let ptr = nmp_app_dispatch_action_bytes(app, envelope.as_ptr(), envelope.len());
    assert!(!ptr.is_null(), "{namespace}: dispatch must not return null");
    // SAFETY: `ptr` is a valid heap C string from the FFI; copied then freed.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.get("correlation_id").is_some(),
        "{namespace} must be accepted (got {parsed})"
    );
}

/// Block until an accepted event of `want_kind` fans out (draining any
/// earlier-kind fan-outs), or fail at the deadline. D8: blocks on the observer
/// channel, never sleeps.
fn await_kind(rx: &mpsc::Receiver<u32>, want_kind: u32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or_default();
        match rx.recv_timeout(remaining) {
            Ok(kind) if kind == want_kind => return,
            Ok(_) => continue, // a different kind fanned out — keep waiting
            Err(_) => panic!("kind:{want_kind} must fan out to observers before the deadline"),
        }
    }
}

/// FINDING A: a local follow then unfollow, dispatched through the real signed
/// FFI path, must be reflected live in `ActiveFollowSet` — the read-your-writes
/// contract for the social graph.
#[test]
fn local_follow_then_unfollow_updates_active_follow_set_live() {
    // Fresh local identity — `nmp_app_signin_nsec` is headless (no keyring
    // capability needed) and the local key signs synchronously on the actor
    // thread.
    let keys = nostr::Keys::generate();
    let me = keys.public_key().to_hex();
    let nsec = keys.secret_key().to_bech32().expect("nsec bech32");

    let bob = nostr::Keys::generate().public_key().to_hex();

    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must return a valid app");
    // SAFETY: `app` is a live pointer from `nmp_app_new`; sole `&mut` for the
    // registration call, dropped before any other access.
    unsafe {
        nmp_nip02::register_actions(&mut *app);
    }

    // Wire the in-tree NIP-65 outbox resolver BEFORE start. The default FFI
    // `NmpApp` ships a `NoopOutboxResolver`, so every `Auto` publish fails
    // closed with `PublishEngineError::NoTargets` — the production-correct
    // fail-closed behaviour, NOT something to weaken. The local kind:3 fan-out
    // under test runs ONLY on the publish engine's accept arm
    // (`record_local_publish_intent` is gated on `start_publish` returning
    // `Ok`), so without a resolver the dispatched follow resolves no target, the
    // accept arm is skipped, and the observer never fires (the pre-fix hang).
    // We install the canonical in-tree `TestKind10002OutboxResolver` — the same
    // resolver every nmp-core publish test auto-installs. The actor re-invokes
    // this factory against the kernel's OWN store + slot handles at Start (and on
    // Reset), so the resolver reads the exact shared state the kernel actor
    // writes (D4 sole-writer preserved). It resolves the active account's write
    // targets from the kind:10002 we inject below; wiring the local-write-relays
    // + active-account slots also enables the active-account fallback.
    unsafe { &*app }.set_publish_resolver_factory(
        |store, _indexer, local_write_relays, active_account| {
            std::sync::Arc::new(
                nmp_core::publish::TestKind10002OutboxResolver::new(store)
                    .with_local_relays(local_write_relays, active_account),
            )
        },
    );

    // Register both observed projections BEFORE start. ADR-0049:
    // the actor reads every wiring slot once at kernel construction
    // (`ActorCommand::Start`); a registration after start would be recorded as
    // `DroppedLateWiring` and never bound onto the kernel. Each observer
    // declares the event kinds it needs before receiving fan-out.
    let follow_set = nmp_nip02::ActiveFollowSet::new(unsafe { &*app }.active_account_handle());
    let _set_id = unsafe { &*app }.open_observed_projection(ObservedProjection::from_kinds(
        follow_set.clone(),
        "test.local_publish_follow_set",
        0,
        [3],
        16,
    ));

    // The test's synchronization edge: signal each accepted fan-out with its
    // kind, so we can wait for the kind:10002 relay list (publish targets) and
    // then each kind:3 contact list.
    let (tx, rx) = mpsc::channel::<u32>();
    let signal = Arc::new(KindSignal {
        tx: Mutex::new(tx),
        kind3_count: AtomicU32::new(0),
    });
    let _sig_id = unsafe { &*app }.open_observed_projection(ObservedProjection::from_kinds(
        signal.clone(),
        "test.local_publish_signal",
        1,
        [3, 10002],
        16,
    ));

    // Install a deterministic, advanceable kernel clock BEFORE start. The
    // kernel stamps every published event's `created_at` from this clock
    // (D7 — the host never owns wall-clock time, but tests may inject a
    // controlled clock through the test-support seam, exactly like the in-crate
    // `Kernel::set_clock` deterministic-replay path). Two back-to-back
    // follow/unfollow dispatches would otherwise stamp the SAME wall-clock
    // second; NIP-01 replaceable supersession then drops the second event
    // (`Superseded`) — id-tiebreak, no fan-out. Advancing this clock by one
    // second between the two dispatches gives the unfollow a strictly greater
    // `created_at`, so it deterministically `Replaced`s the follow and fans out.
    // No `sleep` — the advance is a plain atomic store (D8).
    let clock = Arc::new(nmp_core::MonotonicSecondClock::new(
        std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    unsafe { &*app }.set_kernel_clock_for_test(clock.clone());

    // Start the actor: constructs the kernel and binds the observer slot.
    nmp_app_start(app, 256, 4);

    // Sign in (make active) so the dispatched follow has an authoring identity.
    // FIFO on the actor command channel guarantees this `AddSigner` is processed
    // before the later follow dispatch; a local nsec signs synchronously on the
    // actor thread.
    let nsec_c = CString::new(nsec).unwrap();
    nmp_app_signin_nsec(app, nsec_c.as_ptr(), 1);

    // Give the active account a kind:10002 write relay so the publish engine
    // resolves an outbox target (the local fan-out runs only on the publish
    // engine's accept arm — `NoTargets` is a fail-closed Err that skips it).
    // Self-authored, real signature. Block until its fan-out confirms the relay
    // list is ingested BEFORE dispatching the follow (deterministic ordering —
    // no sleep/poll).
    inject_self_kind10002(app, &keys, "wss://write.example");
    await_kind(&rx, 10002);

    // Establish the active account's kind:3 baseline (an empty-but-present
    // contact list) BEFORE the first follow. The native `follow()` path fails
    // CLOSED when the active account's kind:3 has not been ingested yet
    // (issue #1246b): rebuilding a kind:3 from a not-loaded list would silently
    // wipe the user's contacts, so `follow()` publishes nothing and the
    // observer never fires. A real signed-in account always has its kind:3
    // fetched from a relay first; we model that here with a self-authored,
    // real-signature kind:3. Its `created_at` is strictly EARLIER than the
    // kernel clock (1_700_000_000) so the follow's locally-published kind:3
    // deterministically `Replaced`s it (rather than tying on the same second
    // and being dropped as `Superseded` under NIP-01 id-tiebreak). Block on its
    // kind:3 fan-out so the contact list is loaded before the follow dispatch
    // (deterministic ordering — no sleep/poll).
    inject_self_kind3_empty(app, &keys, 1_699_999_999);
    await_kind(&rx, 3);

    // Precondition: BOB is not followed (only self-inclusion).
    assert!(
        !follow_set.predicate()(&bob),
        "BOB must not be followed before the first dispatch"
    );

    // ── Follow BOB ────────────────────────────────────────────────────────────
    dispatch_ok(app, "nmp.follow", &bob);
    await_kind(&rx, 3);
    assert!(
        follow_set.predicate()(&bob),
        "read-your-writes: BOB must be in the active follow set immediately after a local follow"
    );
    assert!(
        follow_set.predicate()(&me),
        "self-inclusion survives the local follow"
    );

    // Advance the kernel clock so the unfollow's kind:3 carries a strictly
    // greater `created_at` than the follow's — otherwise the same-second
    // replaceable supersession would drop it (`Superseded`, NIP-01 id-tiebreak)
    // and nothing would fan out. The `await_kind(&rx, 3)` above already proved
    // the follow's kind:3 was stamped (so the clock is read for the follow
    // BEFORE this advance — FIFO + the observed fan-out guarantee the ordering).
    clock.advance_secs(1);

    // ── Unfollow BOB (the important regression) ────────────────────────────────
    dispatch_ok(app, "nmp.unfollow", &bob);
    await_kind(&rx, 3);
    assert!(
        !follow_set.predicate()(&bob),
        "read-your-writes: a local unfollow must DROP BOB from the active set live"
    );
    assert!(
        follow_set.predicate()(&me),
        "self-inclusion survives the local unfollow"
    );

    // Exactly three accepted kind:3 fan-outs: the injected baseline contact
    // list (the precondition a real signed-in account always has), plus the
    // local follow and the local unfollow — each fanning out exactly once. The
    // duplicate relay echo of a locally-published kind:3 would NOT add a fourth
    // (D4 — covered by the kernel-level test).
    assert_eq!(
        signal.kind3_count.load(Ordering::SeqCst),
        3,
        "baseline + follow + unfollow each fan out exactly once"
    );

    nmp_app_free(app);
}

/// Inject a self-authored, real-signature kind:3 with an EMPTY `p` section so
/// the active account's contact list is "present but empty" in the store —
/// the precondition the native fail-closed `follow()` gate (issue #1246b)
/// requires before any edit. `created_at` is supplied by the caller so the
/// baseline can be stamped strictly EARLIER than the kernel clock, letting the
/// follow's locally-published kind:3 `Replace` it (not tie + `Superseded`).
fn inject_self_kind3_empty(app: *mut NmpApp, keys: &nostr::Keys, created_at: u64) {
    let event = nostr::EventBuilder::new(nostr::Kind::from(3u16), "")
        .custom_created_at(nostr::Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3");
    let json = event.as_json();
    let json_c = CString::new(json).unwrap();
    let ok = nmp_app_inject_signed_event_json(app, json_c.as_ptr());
    assert!(ok, "kind:3 baseline injection must verify and accept");
}

/// Inject a self-authored, real-signature kind:10002 (NIP-65 write relay) so
/// the publish engine has an outbox target for the active account.
fn inject_self_kind10002(app: *mut NmpApp, keys: &nostr::Keys, write_relay: &str) {
    let event = nostr::EventBuilder::new(nostr::Kind::from(10002u16), "")
        .tags([nostr::Tag::parse(["r", write_relay, "write"]).expect("valid r tag")])
        .sign_with_keys(keys)
        .expect("sign kind:10002");
    let json = event.as_json();
    let json_c = CString::new(json).unwrap();
    let ok = nmp_app_inject_signed_event_json(app, json_c.as_ptr());
    assert!(ok, "kind:10002 injection must verify and accept");
}
