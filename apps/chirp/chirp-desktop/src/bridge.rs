//! FFI bridge — boots the Chirp kernel and dispatches actions.
//!
//! Mirrors the TUI's `runtime.rs` and `bridge.rs` patterns exactly:
//! - `NmpUpdateBridge` sets up a C callback that pipes FlatBuffer bytes
//!   through an `mpsc` channel.
//! - `AppRuntime` constructs the kernel via FFI, registers Chirp projections,
//!   starts the actor, and exposes typed action dispatch methods.
//!
//! Typed social actions (publish_note, react, follow, etc.) are delegated to
//! [`nmp_app_chirp::ChirpClient`] instead of re-implementing dispatch JSON
//! construction here. Raw FFI methods (add_relay, open_timeline, etc.) remain
//! unchanged.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};

use nmp_app_chirp::ffi::{nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list};
use nmp_app_chirp::{
    nmp_app_cancel_bunker_handshake, nmp_app_chirp_declare_consumed_projections,
    nmp_app_chirp_register, nmp_app_chirp_unregister, nmp_app_nostrconnect_uri,
    nmp_marmot_unregister, nmp_signer_broker_init, ChirpClient, ChirpHandle, MarmotHandle,
    NmpRegisterStatus,
};
use nmp_ffi::{
    nmp_app_free, nmp_app_load_older_feed, nmp_app_release_profile_ref,
    nmp_app_resolve_profile_card_live, nmp_app_resolve_profile_ref,
    nmp_app_set_capability_callback, nmp_app_signin_nsec, nmp_app_start, nmp_free_string, NmpApp,
    NmpConfigStatus,
};

// ADR-0063 (#1671 Lane F) — typed resolve_ref / release_ref consumer ids.
/// Consumer id for feed/list-row author refs (profile.ref / CacheOk). Shared
/// across rows — the kernel dedupes per (namespace, key); release on view change.
const FEED_AUTHOR_CONSUMER: &str = "chirp-desktop.feed-author";
/// Consumer id for the open profile screen's ref (profile.card / Live).
const OPEN_PROFILE_CONSUMER: &str = "chirp-desktop.open-profile";

// #1607: nmp_app_wallet_{connect,disconnect} deleted from the C ABI.
// Wallet operations now route through nmp_app_dispatch_action (D11).

// Wallet / social / account / relay / publish-lifecycle `impl AppRuntime`
// methods (split out to keep this file under the 500-LOC hard ceiling).
mod actions;
mod feed;

// ---------------------------------------------------------------------------
// Update bridge (mirrors chirp-tui/src/bridge.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NmpEvent {
    pub payload: Vec<u8>,
}

pub struct NmpUpdateBridge {
    tx: Sender<NmpEvent>,
}

impl NmpUpdateBridge {
    #[must_use]
    pub fn channel() -> (Box<Self>, Receiver<NmpEvent>) {
        let (tx, rx) = mpsc::channel();
        (Box::new(Self { tx }), rx)
    }

    pub fn register(app: *mut NmpApp, bridge: &mut Box<Self>) {
        let context = bridge.as_mut() as *mut Self as *mut std::ffi::c_void;
        // SAFETY: `app` is a valid, non-null pointer from `nmp_app_new`.
        // `context` points to the bridge instance stored in AppRuntime.
        unsafe {
            nmp_ffi::nmp_app_set_update_callback(app, context, Some(on_update));
        }
    }
}

pub fn unregister_callback(app: *mut NmpApp) {
    // SAFETY: clearing the callback is safe even if app is null.
    unsafe {
        nmp_ffi::nmp_app_set_update_callback(app, ptr::null_mut(), None);
    }
}

extern "C" fn on_update(context: *mut std::ffi::c_void, payload: *const u8, len: usize) {
    if context.is_null() || payload.is_null() {
        return;
    }
    let bridge = unsafe { &*(context as *const NmpUpdateBridge) };
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) }.to_vec();
    let _ = bridge.tx.send(NmpEvent { payload: bytes });
}

// ---------------------------------------------------------------------------
// AppRuntime (mirrors chirp-tui/src/runtime.rs)
// ---------------------------------------------------------------------------

pub struct AppRuntime {
    app: *mut NmpApp,
    /// Typed action client — delegates social/account dispatch to ChirpClient
    /// instead of re-implementing JSON construction here.
    client: ChirpClient,
    chirp: *mut ChirpHandle,
    marmot: Cell<*mut MarmotHandle>,
    feed_handles: RefCell<feed::FeedHandles>,
    /// Owns the FFI callback box registered with the actor thread.
    update_bridge: Option<Box<NmpUpdateBridge>>,
}

impl AppRuntime {
    #[must_use]
    pub fn new() -> Option<(Self, Receiver<NmpEvent>)> {
        let app = unsafe { nmp_ffi::nmp_app_new() };
        if app.is_null() {
            return None;
        }
        unsafe {
            if nmp_signer_broker_init(app) != NmpConfigStatus::Ok as u32 {
                nmp_app_free(app);
                return None;
            }
            nmp_app_set_capability_callback(
                app,
                ptr::null_mut(),
                Some(crate::keyring::keyring_handler),
            );
        }

        // V-73: nmp_app_chirp_register now returns a status code; the handle
        // is written through the out-parameter.  Null viewer_pubkey (no viewer
        // at startup) always succeeds.
        let mut chirp: *mut ChirpHandle = ptr::null_mut();
        let register_status = unsafe { nmp_app_chirp_register(app, ptr::null(), &mut chirp) };
        if register_status != NmpRegisterStatus::Ok as u32 || chirp.is_null() {
            unsafe { nmp_app_free(app) };
            return None;
        }

        let (mut bridge, rx) = NmpUpdateBridge::channel();
        NmpUpdateBridge::register(app, &mut bridge);
        // SAFETY: FFI calls with valid pointers.
        unsafe {
            nmp_app_chirp_register_dm_inbox(app);
            nmp_app_chirp_register_follow_list(app, ptr::null());
        }

        let marmot = None;
        let initial_marmot = marmot.unwrap_or(ptr::null_mut());

        // ADR-0053/E4 — declare projection-consumption intent BEFORE start
        // (chirp-desktop is a full client; undeclared start is a loud bug).
        nmp_app_chirp_declare_consumed_projections(app);

        // SAFETY: `app` is valid.
        unsafe {
            nmp_app_start(app, 200, 10);
        }
        let home_feed_handle = feed::open_home_feed(app).ok();

        Some((
            Self {
                app,
                client: ChirpClient::new(app),
                chirp,
                marmot: Cell::new(initial_marmot),
                feed_handles: RefCell::new(feed::FeedHandles {
                    home: home_feed_handle,
                    ..feed::FeedHandles::default()
                }),
                update_bridge: Some(bridge),
            },
            rx,
        ))
    }

    pub fn app_ptr(&self) -> *mut NmpApp {
        self.app
    }

    /// ADR-0063 (#1671 Lane F): resolve a feed/list-row author at `profile.ref` /
    /// `CacheOk` so its avatar/name renders. Origin-blind and deduped per pubkey;
    /// idempotent. Best-effort (D6: invalid args are a silent no-op in the FFI).
    pub fn resolve_feed_author_ref(&self, pubkey: &str) {
        self.resolve_profile_ref(pubkey, FEED_AUTHOR_CONSUMER);
    }

    /// Release a feed/list-row author's `profile.ref` claim when it scrolls off /
    /// the view closes (D5 — bounded by what is open).
    pub fn release_feed_author_ref(&self, pubkey: &str) {
        self.release_ref(FEED_AUTHOR_CONSUMER, pubkey);
    }

    fn resolve_profile_card_live(&self, pubkey: &str) {
        if self.app.is_null() {
            return;
        }
        let (Ok(key), Ok(consumer)) = (CString::new(pubkey), CString::new(OPEN_PROFILE_CONSUMER))
        else {
            return;
        };
        nmp_app_resolve_profile_card_live(self.app, key.as_ptr(), consumer.as_ptr());
    }

    fn resolve_profile_ref(&self, pubkey: &str, consumer: &str) {
        if self.app.is_null() {
            return;
        }
        let (Ok(key), Ok(consumer)) = (CString::new(pubkey), CString::new(consumer)) else {
            return;
        };
        nmp_app_resolve_profile_ref(self.app, key.as_ptr(), consumer.as_ptr());
    }

    fn release_ref(&self, consumer: &str, pubkey: &str) {
        if self.app.is_null() {
            return;
        }
        let (Ok(key), Ok(consumer)) = (CString::new(pubkey), CString::new(consumer)) else {
            return;
        };
        nmp_app_release_profile_ref(self.app, key.as_ptr(), consumer.as_ptr());
    }

    pub fn load_older_timeline(&self) {
        if self.app.is_null() {
            return;
        }
        let key = CString::new("nmp.feed.home").expect("static feed key has no NUL byte");
        unsafe { nmp_app_load_older_feed(self.app, key.as_ptr()) };
    }

    // ------------------------------------------------------------------
    // Account lifecycle
    // ------------------------------------------------------------------
    //
    // `create_account` lives in the sibling `bridge_accounts` module (another
    // `impl AppRuntime` block) — it routes through the Chirp-owned C-ABI
    // create-account wrapper and would otherwise push this file past the
    // 500-LOC ceiling (#1493).

    pub fn sign_in_nsec(&self, secret: &str) {
        // Canonical account path: the dedicated C-ABI signer symbol (matches
        // Android/TUI), NOT the dead `nmp.sign_in_nsec` JSON doorway. Fire-and-
        // forget UI action: silent return on null app or NUL byte in the secret.
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(secret) {
            nmp_app_signin_nsec(self.app, c.as_ptr(), 1);
        }
    }

    /// Generate a `nostrconnect://` URI. Rust selects the relay from the
    /// kernel's relay configuration (D3: relay policy is Rust-owned).
    /// Returns `Err` when no write relay is configured or the broker is not
    /// yet initialised.
    pub fn connect_bunker(&self) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let callback = CString::new("chirp://nip46")
            .map_err(|_| "callback URL contains NUL byte".to_string())?;

        // SAFETY: `app` is valid, callback.as_ptr() is valid for the call.
        let ptr = unsafe { nmp_app_nostrconnect_uri(self.app, callback.as_ptr()) };

        if ptr.is_null() {
            return Err(
                "nostrconnect_uri returned null — broker not initialised or no relay configured"
                    .to_string(),
            );
        }

        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { nmp_free_string(ptr) };

        Ok(text)
    }

    pub fn cancel_bunker_handshake(&self) {
        if !self.app.is_null() {
            unsafe { nmp_app_cancel_bunker_handshake(self.app) };
        }
    }

    // ------------------------------------------------------------------
    // Wallet / social / account / relay / publish-lifecycle actions
    // ------------------------------------------------------------------
    //
    // Those methods live in the `bridge::actions` child module (same inherent
    // `impl AppRuntime`) so this file stays under the 500-LOC hard ceiling.

    // ------------------------------------------------------------------
    // Action dispatch
    // ------------------------------------------------------------------

    /// Dispatch a Chirp action through the typed byte doorway.
    ///
    /// `action_json` is the canonical action body produced by a Chirp builder;
    /// the shared `nmp_app_chirp::dispatch_bytes` seam encodes it into the
    /// namespace's typed [`ActionPayload`] bytes, wraps it in a host-minted
    /// dispatch envelope, and calls `nmp_app_dispatch_action_bytes` — the JSON
    /// never crosses the FFI (ADR-0064 / Cut-B, #1756). The desktop bridge only
    /// dispatches the NIP-47 wallet verbs directly through here; the social
    /// verbs go through the embedded `ChirpClient` (which uses the same seam).
    pub fn dispatch_action(&self, namespace: &str, action_json: &str) -> Result<String, String> {
        nmp_app_chirp::dispatch_action_bytes_for(self.app, namespace, action_json)
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        unregister_callback(self.app);
        self.close_all_feeds();
        // Explicitly drop the bridge before freeing the app so the FFI callback
        // never fires after the NmpApp is gone.
        self.update_bridge.take();
        if !self.chirp.is_null() {
            unsafe { nmp_app_chirp_unregister(self.chirp) };
            self.chirp = ptr::null_mut();
        }
        if !self.marmot.get().is_null() {
            unsafe { nmp_marmot_unregister(self.marmot.get()) };
            self.marmot.set(ptr::null_mut());
        }
        if !self.app.is_null() {
            unsafe { nmp_app_free(self.app) };
            self.app = ptr::null_mut();
        }
    }
}

// The dispatch-result envelope parser moved into `nmp_app_chirp::dispatch_bytes`
// alongside the byte-doorway call that produces the envelope (ADR-0064 / Cut-B,
// #1756); it is unit-tested there. The desktop bridge no longer owns a copy.
