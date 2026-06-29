//! Real end-to-end cold-start cache-serve coverage for the demand-interest
//! lists (#1643 bookmarks kind:10003 — the clean representative of the
//! follow/bookmark/mute family).
//!
//! # What the merged fix claims (and where it was proven before)
//!
//! On sign-in the bookmark runtime (`BookmarksRuntimeController`, wired by
//! `register_bookmark_runtime`) pushes a `Tailing` demand interest
//! (`authors=[pubkey] / kinds=[10003]`). The kernel's `EnsureInterest` handler
//! runs a **synchronous cache-serve drain** that replays any matching event
//! ALREADY in the local store — but NOT yet in the kernel's RAM event cache —
//! to the observed-projection sinks. So a kind:10003 bookmark list persisted in a
//! prior session surfaces into the `BookmarkListProjection` snapshot WITHOUT any
//! relay delivery.
//!
//! That contract was previously proven only at the observed-projection level
//! (`crates/nmp-core/src/kernel/bookmark_cold_start_tests.rs`, using a
//! `CapturingObserver` and `pub(crate)` kernel APIs). This test lifts it to the
//! **full sign-in → cache-serve → projection-snapshot pipeline** an app actually
//! drives: a real `NmpApp` built through the production composition
//! (`register_substrate` + `register_bookmark_runtime` — the exact pair
//! `register_defaults` invokes for the bookmark tier), a real
//! `nmp_app_signin_nsec` flow, and the real typed `BookmarkListProjection`
//! snapshot the shell reads.
//!
//! # Modelling the cold start faithfully
//!
//! A genuine cold start is a process restart: the on-disk store is warm, but the
//! fresh process's in-RAM kernel caches are EMPTY. The cache-serve drain's
//! live→serve dedup (`cache_serve/continuation.rs`: "already reflected in
//! projections" — it skips any store event already present in the kernel's RAM
//! `events` cache) is precisely what makes that distinction load-bearing: a
//! freshly-restarted process has nothing in the RAM cache, so the stored event
//! is NOT deduped and the demand-interest push replays it.
//!
//! We reproduce that state by seeding the kind:10003 **directly into the kernel
//! store** (the `event_store_handle` write seam — the stand-in for "persisted in
//! a prior session"), so it is warm in the store but absent from the RAM cache
//! — exactly the post-restart shape. We do NOT route the seed through live
//! ingest, because live ingest would also populate the RAM cache and the
//! cache-serve dedup would then correctly skip it (that path models "the event
//! already arrived this session", not a cold start).
//!
//! # Why this is a REAL e2e test, not a kernel shortcut
//!
//! * The kernel runs behind the production actor thread spawned by
//!   `nmp_app_start` — every step crosses the real `ActorCommand` mpsc seam.
//! * The bookmark observer + the per-tick `BookmarksRuntimeController` are
//!   installed by `register_bookmark_runtime`, the SAME composition helper
//!   `nmp_defaults::register_defaults` calls. Nothing in this test pushes the
//!   interest by hand — the runtime's snapshot-tick reconciler does it on
//!   sign-in, exactly as in production.
//! * The assertion reads `BookmarkListProjection::snapshot()` — the typed
//!   projection the shell renders.
//!
//! # No relay delivery
//!
//! The app is started with `.without_initial_relays()`; no relay is ever
//! connected and no REQ is ever answered. The ONLY path from the stored
//! kind:10003 to the projection is the cold-start cache-serve drain triggered by
//! the demand-interest push on sign-in.
//!
//! # Red-proof (non-vacuity)
//!
//! Two guards give the positive test teeth:
//!
//! 1. `cold_start_without_demand_interest_does_not_surface_stored_bookmark`
//!    drives the IDENTICAL pipeline but never signs in, so the runtime never
//!    pushes the demand interest. The stored kind:10003 must NOT appear in the
//!    projection — proving the positive test's pass is caused by the
//!    demand-interest push, not by some always-on side channel.
//! 2. The positive test's `before_signin` precondition asserts the seeded event
//!    is NOT visible until the demand interest is pushed.
//!
//! The MANUAL red-proof for "the demand-interest push itself regressed" is to
//! neuter the `(Some(now), None)` sign-in arm of `BookmarksRuntimeController::tick`
//! (crates/nmp-defaults/src/runtimes/bookmarks_runtime.rs) so it stops sending
//! `EnsureInterest`; `signin_surfaces_stored_bookmark_via_cold_start_cache_serve`
//! then fails with an empty snapshot. Verified RED during authoring; restored.

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use nmp_coverage_gate::CoverageGate;
use nmp_defaults::{register_bookmark_runtime, register_substrate};
use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};
use nmp_nip51::{BookmarkItem, BookmarkListProjection};
use nmp_store::{RawEvent, VerifiedEvent};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

// `NmpApp` parks its update-callback `Sender` in a process-global slot, so
// exactly one app may be live at a time. Serialize the whole-lifecycle tests
// (the established idiom across the `nmp_app_*` integration tests).
static SERIAL: Mutex<()> = Mutex::new(());

// `extern "C"` callbacks cannot capture, so the update `Sender` is parked in a
// process-global slot and a tick is forwarded on every kernel update frame.
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

// A fixed test nsec (the same demo key the login-timeline harness uses).
const ACCOUNT_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const BOOKMARKED_EVENT_ID: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const KIND_BOOKMARK_LIST: u16 = 10_003;

/// A live `NmpApp` driven through the REAL bookmark composition, holding the
/// `BookmarkListProjection` handle the shell reads. Tears the app down on drop.
struct BookmarkApp {
    app: *mut NmpApp,
    projection: Arc<BookmarkListProjection>,
    ticks: Receiver<()>,
}

impl BookmarkApp {
    /// Boot through the production composition, install the update callback, and
    /// return the app with its bookmark projection handle.
    ///
    /// Composition = `register_substrate` (the always-on cache-serve / routing
    /// correctness floor) + `register_bookmark_runtime` (the kind:10003 observer
    /// + the per-tick demand-interest reconciler). This is exactly the pair of
    /// calls `nmp_defaults::register_defaults` makes for the bookmark tier; we
    /// call them directly only to capture the returned `Arc<BookmarkListProjection>`
    /// (which `register_defaults` drops). No hand-built kernel state.
    fn boot() -> Self {
        let mut builder = NmpAppBuilder::new();
        // Substrate tier — routing factory, mailbox/profile/contacts caches,
        // coverage hook, NIP-77 runtime: the cache-serve substrate the
        // cold-start drain rides on.
        register_substrate(&mut builder, CoverageGate::default());
        // Bookmark tier — registers the projection observer BEFORE the first
        // tick (the ordering contract) and the runtime that pushes the demand
        // interest on sign-in. Capture the projection handle.
        let projection = register_bookmark_runtime(&mut builder);

        let app = builder
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .start(RunConfig::default());

        let (tx, ticks) = channel::<()>();
        let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(|bytes: &[u8]| {
            update_signal_callback(std::ptr::null_mut(), bytes.as_ptr(), bytes.len());
        })));

        Self {
            app,
            projection,
            ticks,
        }
    }

    /// Seed a verified kind:10003 event DIRECTLY into the kernel store — the
    /// stand-in for "this bookmark list was persisted in a prior session". It
    /// lands in the store but NOT in the kernel's RAM `events` cache, exactly
    /// like the on-disk-warm / RAM-cold shape a freshly restarted process has.
    /// Returns `true` once the store has the event.
    ///
    /// This deliberately bypasses live ingest: routing the seed through ingest
    /// would also warm the RAM cache, and the cache-serve live→serve dedup would
    /// then (correctly) skip the event — that path models "already arrived this
    /// session", not a cold start.
    fn seed_into_store(&self, event: &nostr::Event) -> bool {
        let raw = RawEvent {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: event.kind.as_u16() as u32,
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
        };
        let verified = match VerifiedEvent::try_from_raw(raw) {
            Ok(v) => v,
            Err(_) => return false,
        };
        // Block until the actor has published the kernel store into the shared
        // slot (it does so right after building the kernel on Start).
        let store = self.wait_for_store(Duration::from_secs(5));
        let Some(store) = store else {
            return false;
        };
        store
            .insert(verified, &"wss://relay.prior-session/".to_string(), 0)
            .is_ok()
    }

    fn wait_for_store(&self, timeout: Duration) -> Option<Arc<dyn nmp_store::EventStore>> {
        let slot = unsafe { &*self.app }.event_store_handle();
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(guard) = slot.lock() {
                if let Some(store) = guard.as_ref() {
                    return Some(Arc::clone(store));
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            // The store is published once at Start; a real kernel tick is the
            // natural wake, but a short sleep here only gates the one-time slot
            // publication, not any product behaviour under test.
            let _ = self.ticks.recv_timeout(Duration::from_millis(50));
        }
    }

    /// Sign in with `nsec`, made active — the production sign-in flow that the
    /// per-tick bookmark runtime reacts to by pushing the demand interest.
    fn sign_in(&self, nsec: &str) {
        // SAFETY: self.app is a valid, non-null pointer.
        unsafe { &*self.app }.signin_nsec_for_test(nsec, true);
    }

    /// The current bookmarked event-ids in the typed projection snapshot.
    fn bookmarked_event_ids(&self) -> Vec<String> {
        self.projection
            .snapshot()
            .items
            .into_iter()
            .filter_map(|item| match item {
                BookmarkItem::Event { event_id, .. } => Some(event_id),
                _ => None,
            })
            .collect()
    }

    /// Block on REAL kernel update ticks, re-reading the projection on each
    /// tick, until `pred` holds against the bookmarked ids or the deadline
    /// elapses. D8-compliant: re-reads ONLY on a genuine tick (a silent
    /// re-read on timeout would mask a dead reactive path).
    fn bookmarks_when(&self, timeout: Duration, pred: impl Fn(&[String]) -> bool) -> Vec<String> {
        let mut last = self.bookmarked_event_ids();
        if pred(&last) {
            return last;
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .ticks
                .recv_timeout(remaining.min(Duration::from_secs(1)))
            {
                Ok(()) => {
                    last = self.bookmarked_event_ids();
                    if pred(&last) {
                        return last;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return last,
            }
        }
        last
    }
}

impl Drop for BookmarkApp {
    fn drop(&mut self) {
        // Clear the listener before stopping (quiescence contract).
        unsafe { &*self.app }.set_update_listener(None);
        if let Some(slot) = UPDATE_TX.get() {
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        unsafe { &*self.app }.stop_runtime();
        // SAFETY: app was allocated by NmpAppBuilder::start (Box::into_raw).
        unsafe { drop(std::boxed::Box::from_raw(self.app)) };
    }
}

/// A real Schnorr-signed kind:10003 bookmark list whose single item is
/// `["e", BOOKMARKED_EVENT_ID]`, authored by `keys`.
fn signed_bookmark_list(keys: &Keys, created_at: u64) -> nostr::Event {
    let bookmarked: nostr::EventId = BOOKMARKED_EVENT_ID.parse().expect("valid hex event id");
    EventBuilder::new(Kind::from(KIND_BOOKMARK_LIST), "")
        .tags(vec![Tag::event(bookmarked)])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:10003 bookmark list")
}

// ---------------------------------------------------------------------------
// PRIMARY — full sign-in → cold-start cache-serve → projection snapshot.
// ---------------------------------------------------------------------------

/// A kind:10003 bookmark list persisted to the store in a prior session
/// surfaces into the `BookmarkListProjection` snapshot on sign-in via the
/// demand-interest cold-start cache-serve drain — end to end, no relay.
#[test]
fn signin_surfaces_stored_bookmark_via_cold_start_cache_serve() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let keys = Keys::parse(ACCOUNT_NSEC).expect("valid account nsec");
    let account_hex = keys.public_key().to_hex();

    let app = BookmarkApp::boot();

    // ── Phase 1: seed the store "from a prior session" (RAM cache stays cold) ─
    let stored = signed_bookmark_list(&keys, 1_800_000_000);
    assert!(
        app.seed_into_store(&stored),
        "the kind:10003 bookmark list must verify and persist to the kernel store"
    );

    // Precondition (non-vacuity): the stored list is NOT in the projection
    // before sign-in. No active account ⇒ no demand-interest push ⇒ nothing
    // bridges the store event to the projection yet.
    let before_signin = app.bookmarks_when(Duration::from_secs(2), |ids| {
        ids.contains(&BOOKMARKED_EVENT_ID.to_string())
    });
    assert!(
        !before_signin.contains(&BOOKMARKED_EVENT_ID.to_string()),
        "PRECONDITION: before sign-in the bookmark must NOT be in the projection \
         (it is store-only, RAM-cold; no demand interest has been pushed). If it \
         appears here the cold-start scenario is vacuous. got: {before_signin:?}"
    );

    // ── Phase 2: sign in — the runtime pushes the demand interest on its tick ─
    //
    // This is the ONLY trigger. There is no relay, so the only way the stored
    // kind:10003 can reach the projection is the cache-serve drain that fires
    // synchronously inside the kernel's EnsureInterest handler.
    app.sign_in(ACCOUNT_NSEC);

    // ── Phase 3: assert the projection now carries the stored bookmark ───────
    let after_signin = app.bookmarks_when(Duration::from_secs(5), |ids| {
        ids.contains(&BOOKMARKED_EVENT_ID.to_string())
    });
    assert!(
        after_signin.contains(&BOOKMARKED_EVENT_ID.to_string()),
        "COLD-START FAIL (#1643, e2e): after sign-in the stored kind:10003 \
         bookmark list must surface in the BookmarkListProjection snapshot via \
         the demand-interest cache-serve drain — WITHOUT any relay delivery. \
         Empty/missing means the runtime did not push the demand interest, or \
         the observer-before-push ordering broke, or the cache-serve drain no \
         longer fires synchronously on EnsureInterest. \
         account={account_hex}, snapshot bookmark ids={after_signin:?}"
    );
}

// ---------------------------------------------------------------------------
// RED-PROOF — no demand interest ⇒ the stored bookmark stays invisible.
// ---------------------------------------------------------------------------

/// Drives the IDENTICAL pipeline but never signs in, so the bookmark runtime
/// never pushes the demand interest (no active account ⇒ the per-tick
/// reconciler's `(None, None)` arm is a no-op). The stored kind:10003 must NOT
/// surface in the projection — proving the positive test's pass is caused by
/// the demand-interest push, not by some always-on side channel.
#[test]
fn cold_start_without_demand_interest_does_not_surface_stored_bookmark() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let keys = Keys::parse(ACCOUNT_NSEC).expect("valid account nsec");

    let app = BookmarkApp::boot();

    // Seed the store, exactly as the positive test does.
    let stored = signed_bookmark_list(&keys, 1_800_000_500);
    assert!(
        app.seed_into_store(&stored),
        "kind:10003 must verify and persist to the store"
    );

    // NO sign-in ⇒ no active account ⇒ the runtime never pushes the demand
    // interest ⇒ no cache-serve drain for the kind:10003 shape.
    let ids = app.bookmarks_when(Duration::from_secs(3), |ids| {
        ids.contains(&BOOKMARKED_EVENT_ID.to_string())
    });
    assert!(
        !ids.contains(&BOOKMARKED_EVENT_ID.to_string()),
        "without a sign-in (no demand-interest push) the stored kind:10003 must \
         NOT appear in the projection — if it does, some path other than the \
         demand-interest cache-serve drain is surfacing it and the positive \
         test is not actually exercising the #1643 fix. got: {ids:?}"
    );
}
