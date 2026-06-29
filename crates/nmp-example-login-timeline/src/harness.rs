//! Headless host driver for the login → following-timeline → live-update proof.
//!
//! This is the Rust-driven stand-in for a platform shell (the honesty clause's
//! "headless equivalent that proves the projection→render contract"). It does
//! what `KernelBridge.swift` does on iOS: construct an [`NmpApp`] through
//! [`NmpAppBuilder`], inherit the canonical composition, start the kernel, sign
//! in, open the following timeline, and read the typed `nmp.feed.home`
//! projection — reacting to the kernel's update ticks, never polling.
//!
//! The seeding seam is the test-only synthetic-injection path
//! [`nmp_ffi::nmp_app_inject_signed_event_json`] — a REAL Schnorr-signed event
//! routed through the REAL kernel ingest gate (verify → store → observer
//! fan-out → OP-feed engine → projection). It is the sanctioned, `cfg`-gated
//! escape hatch (docs/escape-hatches.md); a production shell ships without it
//! and receives the identical events from the kernel's relay transport. The
//! point this harness proves is upstream of the wire: a signed event from a
//! followed author surfaces in the rendered projection WITHOUT the shell writing
//! one line of relay/subscription/cache code.
//!
//! ## Reactive, not polling (doctrine D8)
//!
//! The render loop blocks on the kernel's update callback (`recv_timeout` on a
//! per-tick channel) and re-reads the projection on each tick — exactly what a
//! reactive UI's reconciler does. There is no `sleep`+check loop (D8 forbids
//! polling). Because the callback Sender lives in a process-global slot,
//! exactly one [`DemoApp`] may be live at a time; callers serialize (the CI
//! gate holds a `Mutex` for the duration of each test).
//!
//! This module is `harness`-feature-gated so the doctrine-clean shell in
//! `lib.rs` never links any of it.

use std::ffi::CString;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nmp_ffi::{nmp_app_inject_signed_event_json, nmp_app_signin_nsec};
use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};
use nostr::{EventBuilder, JsonUtil, Keys, Kind, PublicKey, Tag, Timestamp};

use crate::{register, register_following_timeline, render_home_rows, TimelineRow};

// `extern "C"` callbacks cannot capture, so the update `Sender` is parked in a
// process-global slot and a tick is forwarded on every kernel update frame.
// Mirrors the proven pattern in `nmp-ffi`'s `active_account_handle_tests.rs` and
// `real_relay_nip17_cold_start_kernel.rs`.
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

fn update_signal_callback() {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

/// A live, signed-in headless app with the following timeline open.
///
/// Owns the `*mut NmpApp` and tears it down on drop. Construct with
/// [`DemoApp::login`]; read rendered rows with [`DemoApp::rows`] /
/// [`DemoApp::rows_when`]; seed events with [`DemoApp::ingest`].
pub struct DemoApp {
    app: *mut NmpApp,
    viewer: String,
    ticks: Receiver<()>,
}

impl DemoApp {
    /// The full login flow, exactly as a platform shell performs it:
    ///
    /// 1. `NmpAppBuilder` → [`register`] (inherit the canonical composition) →
    ///    in-memory store → consume the built-in projections → no built-in
    ///    relays → `start`.
    /// 2. Install the update callback (so renders react to kernel ticks).
    /// 3. [`register_following_timeline`] — open the following timeline (BEFORE
    ///    sign-in so the engine's identity observer catches the first
    ///    account-change and self-seeds the follow set).
    /// 4. `nmp_app_signin_nsec` — sign in with a local nsec, made active.
    /// 5. Block on update ticks until the kernel has written the active-account
    ///    slot, so follow-set / self-inclusion logic is live before the first
    ///    event is ingested.
    ///
    /// Panics if `nsec` is not a valid secret key (this is a fixture entry point,
    /// not a user-input boundary).
    pub fn login(nsec: &str) -> Self {
        let keys = Keys::parse(nsec).expect("harness: valid nsec");
        let viewer = keys.public_key().to_hex();

        // Step 1 — builder → tutorial compatibility register → start.
        let mut builder = NmpAppBuilder::new();
        register(&mut builder);
        let app = builder
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .start(RunConfig::default());

        // Step 2 — install the per-tick update channel.
        let (tx, ticks) = channel::<()>();
        let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        // SAFETY: `start` returns a valid, non-null `*mut NmpApp`.
        unsafe { &*app }.set_update_listener(Some(Arc::new(|_| update_signal_callback())));

        // Step 3 — open the following timeline before sign-in.
        // SAFETY: `start` returns a valid, non-null `*mut NmpApp`.
        register_following_timeline(unsafe { &*app }, viewer.clone());

        // Step 4 — sign in (active).
        let secret = CString::new(nsec).expect("nsec has no interior NUL");
        nmp_app_signin_nsec(app, secret.as_ptr(), 1);

        let demo = Self { app, viewer, ticks };
        // Step 5 — block on ticks until the active-account slot is populated.
        demo.wait_until(Duration::from_secs(5), |d| {
            d.ffi()
                .active_account_handle()
                .lock()
                .map(|g| g.is_some())
                .unwrap_or(false)
        });
        demo
    }

    /// The signed-in account's hex pubkey (the timeline viewer).
    #[must_use]
    pub fn viewer(&self) -> &str {
        &self.viewer
    }

    /// Borrow the FFI handle for projection reads.
    ///
    /// SAFETY: `self.app` is a valid `*mut NmpApp` for the lifetime of `self`.
    #[must_use]
    pub fn ffi(&self) -> &NmpApp {
        unsafe { &*self.app }
    }

    /// Ingest a signed event through the real kernel gate. Returns `true` if the
    /// event verified and was enqueued.
    pub fn ingest(&self, event: &nostr::Event) -> bool {
        let json = event.as_json();
        let c = CString::new(json).expect("event JSON has no interior NUL");
        nmp_app_inject_signed_event_json(self.app, c.as_ptr())
    }

    /// The current rendered following timeline (one decode of the typed
    /// projection).
    #[must_use]
    pub fn rows(&self) -> Vec<TimelineRow> {
        render_home_rows(self.ffi())
    }

    /// Block on kernel update ticks, re-rendering on each REAL tick, until `pred`
    /// holds or the deadline elapses; returns the last-read rows either way.
    ///
    /// D8-compliant *and* load-bearing on the callback path: the projection is
    /// re-read ONLY on a genuine update tick (`Ok(())` — the kernel's update
    /// callback actually fired). A `recv_timeout` Timeout does NOT re-read: a
    /// silent re-read on timeout would degrade this into a poll and MASK a
    /// broken/missing update-callback path, letting the reactive (G2) proof pass
    /// even if the kernel never delivered a single tick. By only advancing `last`
    /// on a real tick, a dead callback path leaves `last` at its pre-loop value,
    /// `pred` never becomes satisfied through a tick, and the live-update gate
    /// correctly FAILS.
    pub fn rows_when(
        &self,
        timeout: Duration,
        pred: impl Fn(&[TimelineRow]) -> bool,
    ) -> Vec<TimelineRow> {
        let mut last = self.rows();
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
                // A REAL kernel update tick fired: only now do we re-read the
                // projection. The reactive proof depends on this callback path.
                Ok(()) => {
                    last = self.rows();
                    if pred(&last) {
                        return last;
                    }
                }
                // No tick in this slice. Do NOT silently re-read (that would
                // mask a dead callback path); keep waiting for a genuine tick
                // until the deadline. If none ever comes, we return the stale
                // pre-loop `last`, and the caller's predicate fails.
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return last,
            }
        }
        last
    }

    /// Block on update ticks until a `&self` predicate holds or the deadline
    /// elapses. The render-free sibling of [`Self::rows_when`] (used to wait for
    /// the active-account slot).
    fn wait_until(&self, timeout: Duration, pred: impl Fn(&Self) -> bool) {
        if pred(self) {
            return;
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .ticks
                .recv_timeout(remaining.min(Duration::from_secs(1)))
            {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pred(self) {
                        return;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

impl Drop for DemoApp {
    fn drop(&mut self) {
        // SAFETY: `self.app` is owned by this `DemoApp` until it is converted
        // back into a `Box<NmpApp>` below.
        let app = unsafe { &*self.app };
        app.set_update_listener(None);
        if let Some(slot) = UPDATE_TX.get() {
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        app.stop_runtime();
        app.shutdown();
        // SAFETY: `NmpAppBuilder::start` transfers ownership as a raw
        // `Box<NmpApp>` pointer; this `DemoApp` is the unique owner.
        drop(unsafe { Box::from_raw(self.app) });
    }
}

// ─── Signed-event fixtures (a stand-in for "events arriving from relays") ──────

/// A signed kind:1 text note with no reply markers — a thread ROOT. Surfaces as
/// a timeline card.
#[must_use]
pub fn note(keys: &Keys, created_at: u64, content: &str) -> nostr::Event {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:1 note")
}

/// A signed kind:1 reply (NIP-10 `root`/`reply` markers pointing at `root_id`).
/// In the OP-centric feed a reply does not get its own row; it attributes back
/// to the root IF its author is in the active account's follow set.
#[must_use]
pub fn reply(keys: &Keys, created_at: u64, root_id: &str, content: &str) -> nostr::Event {
    let root_marker = Tag::parse(["e", root_id, "", "root"]).expect("valid NIP-10 root e-tag");
    let reply_marker = Tag::parse(["e", root_id, "", "reply"]).expect("valid NIP-10 reply e-tag");
    EventBuilder::text_note(content)
        .tags([root_marker, reply_marker])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:1 reply")
}

/// A signed kind:3 contact list naming `follows` — the active account's follow
/// set. Ingesting this (authored by the signed-in account) is how the host says
/// "I follow these people"; the kernel's `ActiveFollowSet` picks it up and the
/// following timeline narrows to them. The host writes NO subscription code.
#[must_use]
pub fn contact_list(keys: &Keys, created_at: u64, follows: &[PublicKey]) -> nostr::Event {
    let tags = follows.iter().map(|pk| Tag::public_key(*pk));
    EventBuilder::new(Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3 contact list")
}

// ─── The end-to-end demo the runnable example prints ──────────────────────────

/// A fixed demo secret key (a well-known throwaway nsec). The viewer / signed-in
/// account for the runnable demo and the gate.
pub const DEMO_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// The result of running the demo journey: the rows rendered after the initial
/// backfill and after the live update.
pub struct DemoResult {
    /// The followed author's hex pubkey.
    pub followed_author: String,
    /// Rows rendered after login + follow + the followed author's first note.
    pub after_login: Vec<TimelineRow>,
    /// Rows rendered after a live second note from the followed author.
    pub after_live_update: Vec<TimelineRow>,
}

/// Drive the whole journey: login → follow an author → render the following
/// timeline → receive a live note → re-render. Returns the rows at each stage.
///
/// This is the body of `examples/login_timeline.rs` and the spine of the CI
/// gate. The followed author is a fresh random key so the run is self-contained.
#[must_use]
pub fn run_demo() -> DemoResult {
    let demo = DemoApp::login(DEMO_NSEC);
    let viewer_keys = Keys::parse(DEMO_NSEC).expect("valid demo nsec");
    let author = Keys::generate();
    let author_hex = author.public_key().to_hex();

    // The host declares its follow set: V follows the author. No relay, no
    // subscription — a signed kind:3 through the store, the canonical write path.
    let follow_list = contact_list(&viewer_keys, 1_000, &[author.public_key()]);
    assert!(demo.ingest(&follow_list), "follow list must verify");

    // The followed author posts a note. It arrives through the kernel and
    // surfaces in the rendered following timeline.
    let first = note(&author, 1_100, "gm - first note from someone I follow");
    assert!(demo.ingest(&first), "first note must verify");
    let after_login = demo.rows_when(Duration::from_secs(5), |rows| {
        rows.iter().any(|r| r.author_pubkey == author_hex)
    });

    // LIVE UPDATE: a brand-new note from the same followed author shows up
    // without the host doing any subscription work.
    let second = note(
        &author,
        1_200,
        "live update - a fresh note, no refresh button",
    );
    assert!(demo.ingest(&second), "second note must verify");
    let after_live_update = demo.rows_when(Duration::from_secs(5), |rows| {
        rows.iter()
            .filter(|r| r.author_pubkey == author_hex)
            .count()
            >= 2
    });

    DemoResult {
        followed_author: author_hex,
        after_login,
        after_live_update,
    }
}
