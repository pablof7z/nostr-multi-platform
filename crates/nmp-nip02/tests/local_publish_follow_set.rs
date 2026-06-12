//! End-to-end regression for FINDING A — read-your-writes for follows.
//!
//! Proves the full production path: a locally-dispatched `nmp.follow` /
//! `nmp.unfollow` against a real signed-in [`NmpApp`] builds, signs, and
//! publishes a kind:3 contact list whose acceptance fans out to a registered
//! [`KernelEventObserver`] — so [`ActiveFollowSet`] reflects the follow AND the
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
//! observer fan-out itself: a second `KernelEventObserver` signals an
//! `mpsc::Sender` from the actor thread when an accepted kind:3 arrives, and the
//! test blocks on `recv_timeout` (an OS-event wait, never a `sleep`+check spin).
//! `ActiveFollowSet` is registered on the SAME slot, so once the signal lands
//! the set is already updated.

use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_ffi::{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_inject_signed_event_json, nmp_app_new,
    nmp_app_signin_nsec, nmp_free_string, NmpApp,
};
use nostr::prelude::*;

/// A kind:3-gated observer that signals `tx` from the actor thread each time an
/// accepted contact list fans out. This is the test's only synchronization edge
/// (D8 — the test blocks on `rx.recv_timeout`, never polls).
struct Kind3Signal {
    tx: Mutex<mpsc::Sender<()>>,
    count: AtomicU32,
}

impl KernelEventObserver for Kind3Signal {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != 3 {
            return;
        }
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(());
        }
    }
}

/// Dispatch `namespace`/`body` through the generic action door and assert the
/// synchronous accept (a 32-hex `correlation_id`, NOT publish completion — that
/// settles later on the actor thread).
fn dispatch_ok(app: *mut NmpApp, namespace: &str, body: &str) {
    let ns = CString::new(namespace).unwrap();
    let payload = CString::new(body).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), payload.as_ptr());
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

/// Block until the next accepted kind:3 fans out, or fail at the deadline.
/// D8: blocks on the observer channel, never sleeps.
fn await_kind3(rx: &mpsc::Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(5))
        .expect("locally published kind:3 must fan out to observers before the deadline");
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

    // Sign in (make active) so the dispatched follow has an authoring identity.
    let nsec_c = CString::new(nsec).unwrap();
    nmp_app_signin_nsec(app, nsec_c.as_ptr(), 1);

    // Give the active account a kind:10002 write relay so the publish engine
    // resolves a target (realistic outbox path). Self-authored, real signature.
    inject_self_kind10002(app, &keys, "wss://write.example");

    // The follow-set producer, registered on the app's shared observer slot —
    // exactly as the composition root (nmp-defaults) wires it in production.
    let follow_set = nmp_nip02::ActiveFollowSet::new(unsafe { &*app }.active_account_handle());
    let _set_id = unsafe { &*app }.register_event_observer(follow_set.clone());

    // The test's synchronization edge: signal on each accepted kind:3 fan-out.
    let (tx, rx) = mpsc::channel::<()>();
    let signal = Arc::new(Kind3Signal {
        tx: Mutex::new(tx),
        count: AtomicU32::new(0),
    });
    let _sig_id = unsafe { &*app }.register_event_observer(signal.clone());

    // Precondition: BOB is not followed (only self-inclusion).
    assert!(
        !follow_set.predicate()(&bob),
        "BOB must not be followed before the first dispatch"
    );

    // ── Follow BOB ────────────────────────────────────────────────────────────
    dispatch_ok(app, "nmp.follow", &format!(r#"{{"pubkey":"{bob}"}}"#));
    await_kind3(&rx);
    assert!(
        follow_set.predicate()(&bob),
        "read-your-writes: BOB must be in the active follow set immediately after a local follow"
    );
    assert!(
        follow_set.predicate()(&me),
        "self-inclusion survives the local follow"
    );

    // ── Unfollow BOB (the important regression) ────────────────────────────────
    dispatch_ok(app, "nmp.unfollow", &format!(r#"{{"pubkey":"{bob}"}}"#));
    await_kind3(&rx);
    assert!(
        !follow_set.predicate()(&bob),
        "read-your-writes: a local unfollow must DROP BOB from the active set live"
    );
    assert!(
        follow_set.predicate()(&me),
        "self-inclusion survives the local unfollow"
    );

    // Exactly two accepted kind:3 fan-outs (follow + unfollow); the duplicate
    // relay echo would NOT add a third (D4 — covered by the kernel-level test).
    assert_eq!(
        signal.count.load(Ordering::SeqCst),
        2,
        "follow + unfollow each fan out exactly once"
    );

    nmp_app_free(app);
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
