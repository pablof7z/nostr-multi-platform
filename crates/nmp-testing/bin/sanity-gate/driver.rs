//! Real-kernel driver via the public `nmp_app_*` FFI.
//!
//! This is the sign-in-as-account path the brief asks for. Rather than editing
//! firehose-bench (which drives the DEFAULT actor), this NEW driver uses the
//! public FFI exactly as a native shell would:
//!
//!   nmp_app_new
//!   → nmp_app_chirp_register(viewer)           // canonical NMP composition
//!   → nmp_app_chirp_declare_consumed_projections
//!   → nmp_app_signin_nsec(nsec, make_active=1)  // ← sign-in-as-account
//!   → nmp_app_add_relay(url, "both")            // local nak or live relays
//!   → nmp_app_set_update_callback(capture_cb)
//!   → nmp_app_start(...)
//!   → nmp_app_chirp_open_home_feed              // the real follow feed
//!
//! Update frames are captured into a `Mutex<CaptureState>` via the same
//! `Box::into_raw` ctx pattern ffi-stress uses (`s7_feed_idle.rs`).
//!
//! D0: app lifecycle, composition, identity, relay, and action symbols here
//! are real production FFI exports. Test-support symbols are limited to
//! harness-only injection/read/synchronization seams.

use std::collections::HashSet;
use std::ffi::{CString, c_void};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nmp_app_chirp::{
    ChirpHandle, nmp_app_chirp_declare_consumed_projections, nmp_app_chirp_open_home_feed,
    nmp_app_chirp_register, nmp_app_chirp_unregister,
};
use nmp_core::typed_projections::{ACTION_RESULTS_SCHEMA_ID, decode_action_results};
use nmp_core::{WireProjectionState, decode_snapshot_envelope, decode_snapshot_typed_projections};
use nmp_ffi::{
    NmpApp, nmp_app_add_relay, nmp_app_free, nmp_app_new, nmp_app_set_update_callback,
    nmp_app_signin_nsec, nmp_app_start,
};
use nmp_testing::harness_probe::{FrameProbe, ProbeSignal};

/// Visible/relay state distilled from each captured frame.
#[derive(Clone, Debug, Default)]
pub struct FrameRecord {
    pub at_ms: u64,
    pub visible_items: u64,
    pub events_rx: u64,
    pub note_events: u64,
    pub connected: bool,
    pub serialize_us: u64,
    /// Raw FlatBuffers frame size in bytes (the payload that crossed FFI).
    /// Used by the FFI-boundedness oracle to assert the frame stays bounded
    /// as the underlying store grows.
    pub frame_bytes: u64,
    /// Open wire subscriptions on this frame: (relay_url, state). Used by the
    /// resilience sub-leak oracle (no dangling `open` sub after views close).
    pub wire_subs: Vec<(String, String)>,
}

#[derive(Default)]
pub struct CaptureState {
    pub records: Vec<FrameRecord>,
    action_terminal_ids: HashSet<String>,
    start: Option<Instant>,
}

impl CaptureState {
    fn elapsed_ms(&self) -> u64 {
        self.start
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn latest(&self) -> Option<&FrameRecord> {
        self.records.last()
    }

    pub fn peak_visible(&self) -> u64 {
        self.records
            .iter()
            .map(|r| r.visible_items)
            .max()
            .unwrap_or(0)
    }

    pub fn any_connected(&self) -> bool {
        self.records.iter().any(|r| r.connected)
    }

    /// Peak raw FFI frame size observed (bytes). Bounded-frame oracle reads this.
    pub fn peak_frame_bytes(&self) -> u64 {
        self.records
            .iter()
            .map(|r| r.frame_bytes)
            .max()
            .unwrap_or(0)
    }

    /// Count of subscriptions in the `open` state on the latest frame.
    pub fn latest_open_sub_count(&self) -> usize {
        self.records
            .last()
            .map(|r| r.wire_subs.iter().filter(|(_, s)| s == "open").count())
            .unwrap_or(0)
    }

    pub fn has_action_terminal(&self, correlation_id: &str) -> bool {
        self.action_terminal_ids.contains(correlation_id)
    }
}

/// Callback context: the shared capture buffer plus the event-driven probe
/// signal. Every captured frame updates `state` then fires `signal`, so a
/// waiter blocked in [`FrameProbe::recv_until`] wakes and re-checks: no
/// sleep/check polling (Doctrine D8).
struct CaptureCtx {
    state: Mutex<CaptureState>,
    signal: ProbeSignal,
}

extern "C" fn capture_cb(ctx: *mut c_void, payload: *const u8, payload_len: usize) {
    if ctx.is_null() || payload.is_null() || payload_len == 0 {
        return;
    }
    let ptr = ctx as *mut CaptureCtx;
    // SAFETY: ctx is a live Box::into_raw-ed CaptureCtx.
    let cx = unsafe { &*ptr };
    {
        let Ok(mut state) = cx.state.lock() else {
            return;
        };
        if state.start.is_none() {
            state.start = Some(Instant::now());
        }
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(payload, payload_len) };
        let at_ms = state.elapsed_ms();
        for id in action_terminal_ids(bytes) {
            state.action_terminal_ids.insert(id);
        }
        if let Ok(env) = decode_snapshot_envelope(bytes) {
            let connected = env
                .relay_status
                .as_ref()
                .map(|s| s.connection == "connected")
                .unwrap_or(false)
                || env
                    .relay_statuses
                    .iter()
                    .any(|s| s.connection == "connected");
            let wire_subs = env
                .wire_subscriptions
                .iter()
                .map(|w| (w.relay_url.clone(), w.state.clone()))
                .collect();
            state.records.push(FrameRecord {
                at_ms,
                visible_items: env.visible_items,
                events_rx: env.events_rx,
                note_events: env.note_events,
                connected,
                serialize_us: env.serialize_us,
                frame_bytes: payload_len as u64,
                wire_subs,
            });
        }
    }
    // Wake any waiter AFTER the lock is released so it can read immediately.
    cx.signal.notify();
}

fn action_terminal_ids(bytes: &[u8]) -> Vec<String> {
    let Ok(typed) = decode_snapshot_typed_projections(bytes) else {
        return Vec::new();
    };
    typed
        .into_iter()
        .filter(|entry| {
            entry.state == WireProjectionState::Changed
                && (entry.key == ACTION_RESULTS_SCHEMA_ID
                    || entry.schema_id == ACTION_RESULTS_SCHEMA_ID)
        })
        .filter_map(|entry| decode_action_results(&entry.payload).ok())
        .flat_map(|model| model.results.into_iter())
        .map(|row| row.correlation_id)
        .filter(|id| !id.is_empty())
        .collect()
}

/// A live driven app. Holds the raw FFI handles + the capture ctx; tears them
/// down in the correct order on drop.
pub struct DrivenApp {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    ctx: *mut CaptureCtx,
    /// Waiter half of the frame probe; woken by each `capture_cb` frame.
    probe: FrameProbe,
    // Keep CStrings alive only as long as needed (FFI copies them).
}

impl DrivenApp {
    /// Build, sign in as `nsec` (or ephemeral), connect `relays`, start, and
    /// open the home feed. `viewer_hex` is the account's own pubkey (used by
    /// chirp_register for self-inclusion + the follow-set oracle).
    pub fn launch(nsec: Option<&str>, viewer_hex: Option<&str>, relays: &[String]) -> Self {
        let app = nmp_app_new();

        // Register the canonical Chirp composition (NIP-02/17/57/65 + routing).
        let viewer_c = viewer_hex
            .filter(|h| !h.is_empty())
            .map(|h| CString::new(h).expect("viewer hex has no nul"));
        let viewer_ptr = viewer_c
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(std::ptr::null());
        let mut chirp: *mut ChirpHandle = std::ptr::null_mut();
        let _status = nmp_app_chirp_register(app, viewer_ptr, &mut chirp);

        // Declare projection-consumption intent (ADR-0053) before start.
        nmp_app_chirp_declare_consumed_projections(app);

        // Sign in as the account (make_active = 1). Ephemeral if no nsec.
        let nsec_owned = nsec.map(|s| s.to_string()).unwrap_or_else(generate_nsec);
        if let Ok(c) = CString::new(nsec_owned) {
            nmp_app_signin_nsec(app, c.as_ptr(), 1);
        }

        // Connect relays as `both` (read + write).
        let role = CString::new("both").unwrap();
        for url in relays {
            if let Ok(c) = CString::new(url.as_str()) {
                nmp_app_add_relay(app, c.as_ptr(), role.as_ptr());
            }
        }

        // Wire the capture callback before start so no frame is missed. The
        // probe's signal half rides in the ctx; the waiter half stays here.
        let (signal, probe) = FrameProbe::new();
        let ctx = Box::into_raw(Box::new(CaptureCtx {
            state: Mutex::new(CaptureState::default()),
            signal,
        }));
        nmp_app_set_update_callback(app, ctx as *mut c_void, Some(capture_cb));

        // Start: visible_limit 500 (TIMELINE_CACHE_LIMIT), emit 4 Hz (cold_start parity).
        nmp_app_start(app, 500, 4);
        nmp_app_chirp_open_home_feed(app);

        DrivenApp {
            app,
            chirp,
            ctx,
            probe,
        }
    }

    /// Borrow the capture state under its lock for reading.
    pub fn with_state<R>(&self, f: impl FnOnce(&CaptureState) -> R) -> R {
        // SAFETY: ctx is live until Drop.
        let guard = unsafe { &*self.ctx }.state.lock().expect("capture lock");
        f(&guard)
    }

    /// Block until `pred(&CaptureState)` holds or `timeout` elapses. Event
    /// driven (Doctrine D8: no polling): blocks on the frame probe and
    /// re-checks `pred` only when `capture_cb` signals a new frame, instead of
    /// a fixed-interval sleep/check loop. Returns the elapsed ms when
    /// satisfied, or `None` on timeout.
    pub fn wait_until(
        &self,
        timeout: Duration,
        mut pred: impl FnMut(&CaptureState) -> bool,
    ) -> Option<u64> {
        self.probe
            .recv_until(timeout, || self.with_state(|s| pred(s)))
            .then(|| self.with_state(|s| s.elapsed_ms()))
    }

    pub fn wait_barrier(&self, timeout: Duration) -> bool {
        nmp_ffi::nmp_app_wait_barrier(self.app, timeout.as_millis() as u64)
    }

    pub fn wait_for_action_terminal(&self, correlation_id: &str, timeout: Duration) -> bool {
        self.wait_until(timeout, |s| s.has_action_terminal(correlation_id))
            .is_some()
    }

    pub fn raw(&self) -> *mut NmpApp {
        self.app
    }

    /// `true` iff the kernel actor thread is still alive (panic-safety oracle).
    pub fn is_alive(&self) -> bool {
        nmp_ffi::nmp_app_is_alive(self.app) != 0
    }

    /// Read the kernel's recent routing-decisions ledger as parsed JSON. Used by
    /// the privacy + outbox-routing oracles to inspect `publishes[]` targets
    /// (kind / urls) and `subscriptions[]` without scraping the churning store.
    pub fn routing_decisions(&self) -> Option<serde_json::Value> {
        let ptr = nmp_ffi::nmp_app_recent_routing_decisions(self.app);
        if ptr.is_null() {
            return None;
        }
        // SAFETY: ptr is a heap-owned NUL-terminated string from the FFI; we
        // copy it out and then free it via the matching FFI free.
        let json = unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .and_then(|s| serde_json::from_str(s).ok());
        nmp_ffi::nmp_free_string(ptr);
        json
    }
}

impl Drop for DrivenApp {
    fn drop(&mut self) {
        // Detach the callback before reclaiming the ctx so no in-flight
        // invocation dereferences freed memory (mirror ffi-stress teardown).
        nmp_app_set_update_callback(self.app, std::ptr::null_mut(), None);
        if !self.chirp.is_null() {
            nmp_app_chirp_unregister(self.chirp);
        }
        nmp_app_free(self.app);
        if !self.ctx.is_null() {
            // SAFETY: we created it with Box::into_raw and the callback is detached.
            drop(unsafe { Box::from_raw(self.ctx) });
        }
    }
}

/// Generate a fresh `nsec1…` for the ephemeral-account path.
fn generate_nsec() -> String {
    use nostr::{Keys, ToBech32};
    Keys::generate()
        .secret_key()
        .to_bech32()
        .unwrap_or_default()
}
