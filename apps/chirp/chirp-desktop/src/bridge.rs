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

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};

use nmp_app_chirp::ffi::{
    nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
};
use nmp_app_chirp::{
    nmp_app_cancel_bunker_handshake, nmp_app_chirp_close_author_feed,
    nmp_app_chirp_close_home_feed, nmp_app_chirp_close_thread_feed,
    nmp_app_chirp_declare_consumed_projections,
    nmp_app_chirp_open_author_feed, nmp_app_chirp_open_home_feed,
    nmp_app_chirp_open_thread_feed, nmp_app_chirp_register, nmp_app_chirp_unregister,
    nmp_app_nostrconnect_uri, nmp_marmot_unregister,
    nmp_signer_broker_init, ChirpClient, ChirpHandle, MarmotHandle, NmpRegisterStatus,
};
use nmp_nip01::NoteRecord;
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_cancel_publish, nmp_app_dispatch_action, nmp_app_free,
    nmp_app_load_older_feed, nmp_app_remove_relay, nmp_app_retry_publish,
    nmp_app_set_capability_callback, nmp_app_start, nmp_free_string, NmpApp, NmpConfigStatus,
};
use serde_json::Value;
use std::ffi::c_void;

unsafe extern "C" {
    fn nmp_app_wallet_connect(app: *mut c_void, uri: *const std::ffi::c_char);
    fn nmp_app_wallet_disconnect(app: *mut c_void);
}

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
            nmp_app_start(app, 0, 200, 10);
        }
        nmp_app_chirp_open_home_feed(app);

        Some((
            Self {
                app,
                client: ChirpClient::new(app),
                chirp,
                marmot: Cell::new(initial_marmot),
                update_bridge: Some(bridge),
            },
            rx,
        ))
    }

    pub fn app_ptr(&self) -> *mut NmpApp {
        self.app
    }

    // ------------------------------------------------------------------
    // Timeline / view lifecycle
    // ------------------------------------------------------------------

    pub fn open_timeline(&self) {
        if !self.app.is_null() {
            nmp_app_chirp_open_home_feed(self.app);
        }
    }

    pub fn close_timeline(&self) {
        if !self.app.is_null() {
            nmp_app_chirp_close_home_feed(self.app);
        }
    }

    pub fn open_thread(&self, event_id: &str) {
        // M2 (ADR-0042 §5.1, V-112): use the Chirp flat-feed seam instead of the
        // deleted `nmp_app_open_thread` → `OpenThread` kernel machinery.
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(event_id) {
            nmp_app_chirp_open_thread_feed(self.app, c.as_ptr());
        }
    }

    pub fn close_thread(&self, event_id: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(event_id) {
            nmp_app_chirp_close_thread_feed(self.app, c.as_ptr());
        }
    }

    pub fn open_author(&self, pubkey: &str) {
        // M2 (ADR-0042 §5.1, V-112): use the Chirp flat-feed seam instead of the
        // deleted `nmp_app_open_author` → `OpenAuthor` kernel machinery.
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(pubkey) {
            nmp_app_chirp_open_author_feed(self.app, c.as_ptr());
        }
    }

    pub fn close_author(&self, pubkey: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(pubkey) {
            nmp_app_chirp_close_author_feed(self.app, c.as_ptr());
        }
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
        let _ = self.client.sign_in_nsec(secret);
    }

    pub fn connect_bunker(&self, relay_url: &str) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let relay = CString::new(relay_url)
            .map_err(|_| "relay URL contains NUL byte".to_string())?;
        let callback = CString::new("chirp://nip46")
            .map_err(|_| "callback URL contains NUL byte".to_string())?;

        // SAFETY: `app` is valid, relay_ptr is valid, callback_ptr is valid.
        let ptr = unsafe {
            nmp_app_nostrconnect_uri(
                self.app,
                relay.as_ptr(),
                callback.as_ptr(),
            )
        };

        if ptr.is_null() {
            return Err("nostrconnect_uri returned null".to_string());
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
    // Wallet actions (NIP-47 NWC)
    // ------------------------------------------------------------------

    pub fn wallet_connect(&self, nwc_uri: &str) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let uri = CString::new(nwc_uri)
            .map_err(|_| "NWC URI contains NUL byte".to_string())?;
        unsafe {
            nmp_app_wallet_connect(self.app.cast(), uri.as_ptr());
        }
        Ok("wallet_connected".to_string())
    }

    pub fn wallet_disconnect(&self) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        unsafe {
            nmp_app_wallet_disconnect(self.app.cast());
        }
        Ok("wallet_disconnected".to_string())
    }

    // ------------------------------------------------------------------
    // Social actions
    // ------------------------------------------------------------------

    pub fn publish_note(
        &self,
        content: &str,
        reply_to: Option<&NoteRecord>,
    ) -> Result<String, String> {
        self.client.publish_note(content, reply_to)
    }

    pub fn react(&self, event_id: &str, reaction: &str) -> Result<String, String> {
        self.client.react(event_id, reaction)
    }

    pub fn follow(&self, pubkey: &str) -> Result<String, String> {
        self.client.follow(pubkey)
    }

    pub fn unfollow(&self, pubkey: &str) -> Result<String, String> {
        self.client.unfollow(pubkey)
    }

    pub fn repost(&self, event_id: &str, author_pubkey: &str) -> Result<String, String> {
        self.client.repost(event_id, author_pubkey)
    }

    pub fn send_dm(&self, recipient_pubkey: &str, content: &str) -> Result<String, String> {
        self.client.send_dm(recipient_pubkey, content)
    }

    pub fn zap(&self, recipient_pubkey: &str, amount_msats: u64, target_event_id: &str) -> Result<String, String> {
        self.client.zap(recipient_pubkey, amount_msats, target_event_id, "")
    }

    // ------------------------------------------------------------------
    // Account lifecycle
    // ------------------------------------------------------------------

    pub fn switch_account(&self, pubkey: &str) {
        let _ = self.client.switch_account(pubkey);
    }

    pub fn remove_account(&self, pubkey: &str) {
        let _ = self.client.remove_account(pubkey);
    }

    pub fn publish_profile(&self, name: &str, about: &str, picture: &str) -> Result<String, String> {
        self.client.publish_profile(name, about, picture)
    }

    // ------------------------------------------------------------------
    // Relay actions
    // ------------------------------------------------------------------

    pub fn add_relay(&self, url: &str, role: &str) {
        if self.app.is_null() {
            return;
        }
        if let (Ok(url_c), Ok(role_c)) = (CString::new(url), CString::new(role)) {
            unsafe { nmp_app_add_relay(self.app, url_c.as_ptr(), role_c.as_ptr()) };
        }
    }

    pub fn remove_relay(&self, url: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(url_c) = CString::new(url) {
            unsafe { nmp_app_remove_relay(self.app, url_c.as_ptr()) };
        }
    }

    /// Publish the user's NIP-65 relay list (kind:10002) via the existing
    /// `nmp.nip65.publish_relay_list` action. `relays` is the configured-relay
    /// set as `(url, role)` pairs read from the settings UI projection.
    pub fn publish_relay_list(&self, relays: &[(&str, &str)]) -> Result<String, String> {
        self.client.publish_relay_list(relays)
    }

    // ------------------------------------------------------------------
    // Publish lifecycle actions
    // ------------------------------------------------------------------

    pub fn retry_publish(&self, handle: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(handle) {
            unsafe { nmp_app_retry_publish(self.app, c.as_ptr()) };
        }
    }

    pub fn cancel_publish(&self, handle: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(handle) {
            unsafe { nmp_app_cancel_publish(self.app, c.as_ptr()) };
        }
    }

    /// Acknowledge a terminal action stage so the kernel evicts it from the
    /// `action_stages` map.  Must be called after a `"published"`, `"failed"`,
    /// or `"error"` stage has been shown to the user — mirrors the TUI's
    /// `runtime.rs` `ack_action_stage` and the Android FFI pattern.
    pub fn ack_action_stage(&self, correlation_id: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(correlation_id) {
            unsafe { nmp_ffi::nmp_app_ack_action_stage(self.app, c.as_ptr()) };
        }
    }

    // ------------------------------------------------------------------
    // Action dispatch
    // ------------------------------------------------------------------

    pub fn dispatch_action(&self,
        namespace: &str,
        action_json: &str,
    ) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let namespace = CString::new(namespace)
            .map_err(|_| "action namespace contains NUL byte".to_string())?;
        let action =
            CString::new(action_json).map_err(|_| "action JSON contains NUL byte".to_string())?;

        // SAFETY: `app` is a valid, non-null pointer.
        let ptr = unsafe { nmp_app_dispatch_action(self.app, namespace.as_ptr(), action.as_ptr()) };
        if ptr.is_null() {
            return Err("action dispatch returned null".to_string());
        }
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { nmp_free_string(ptr) };
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("action dispatch returned invalid JSON: {e}"))?;
        parse_dispatch_envelope(&value)
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        unregister_callback(self.app);
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

fn parse_dispatch_envelope(value: &Value) -> Result<String, String> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    value
        .get("correlation_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "action dispatch envelope missing correlation_id".to_string())
}
