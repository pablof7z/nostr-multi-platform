//! FFI bridge — boots the Chirp kernel and dispatches actions.
//!
//! Mirrors the TUI's `runtime.rs` and `bridge.rs` patterns exactly:
//! - `NmpUpdateBridge` sets up a C callback that pipes FlatBuffer bytes
//!   through an `mpsc` channel.
//! - `AppRuntime` constructs the kernel via FFI, registers Chirp projections,
//!   starts the actor, and exposes typed action dispatch methods.

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};

use nmp_app_chirp::ffi::{
    nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
};
use nmp_app_chirp::{
    nmp_app_cancel_bunker_handshake, nmp_app_chirp_register, nmp_app_chirp_unregister,
    nmp_app_nostrconnect_uri, nmp_broker_free_string, nmp_marmot_unregister, nmp_signer_broker_init,
    ChirpHandle, MarmotHandle,
};
use nmp_ffi::{
    nmp_app_dispatch_action,
    nmp_app_free, nmp_app_free_string, nmp_app_load_older_feed,
    nmp_app_open_author, nmp_app_open_thread, nmp_app_open_timeline,
    nmp_app_start, nmp_app_add_relay, nmp_app_remove_relay, NmpApp,
};
use serde_json::{json, Value};

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
    chirp: *mut ChirpHandle,
    marmot: Cell<*mut MarmotHandle>,
    /// Keep the update bridge alive — the FFI callback stores a raw pointer
    /// to this box, so dropping it would cause a use-after-free / SIGSEGV
    /// when the actor thread fires `on_update`.
    update_bridge: Option<Box<NmpUpdateBridge>>,
}

impl AppRuntime {
    #[must_use]
    pub fn new() -> Option<(Self, Receiver<NmpEvent>)> {
        let app = unsafe { nmp_ffi::nmp_app_new() };
        if app.is_null() {
            return None;
        }
        // SAFETY: `app` is a valid, non-null pointer from `nmp_app_new`.
        unsafe {
            nmp_signer_broker_init(app);
        }

        let chirp = unsafe { nmp_app_chirp_register(app, ptr::null()) };
        if chirp.is_null() {
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

        // SAFETY: `app` is valid.
        unsafe {
            nmp_app_start(app, 0, 200, 10);
            nmp_app_open_timeline(app);
        }

        Some((
            Self {
                app,
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
            unsafe { nmp_app_open_timeline(self.app) };
        }
    }

    pub fn open_thread(&self, event_id: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(event_id) {
            unsafe { nmp_app_open_thread(self.app, c.as_ptr()) };
        }
    }

    pub fn open_author(&self, pubkey: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(pubkey) {
            unsafe { nmp_app_open_author(self.app, c.as_ptr()) };
        }
    }

    pub fn close_thread(&self) {
        if !self.app.is_null() {
            unsafe { nmp_app_open_timeline(self.app) };
        }
    }

    pub fn close_author(&self) {
        if !self.app.is_null() {
            unsafe { nmp_app_open_timeline(self.app) };
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

    pub fn create_account(
        &self,
        profile: std::collections::HashMap<String, String>,
        relays: Vec<(String, String)>,
    ) {
        let profile_json = json!(profile).to_string();
        let relays_json: Vec<serde_json::Value> = relays
            .into_iter()
            .map(|(url, role)| json!({ "url": url, "role": role }))
            .collect();
        let action = json!({
            "CreateAccount": {
                "profile": serde_json::from_str::<Value>(&profile_json).unwrap_or(Value::Null),
                "relays": relays_json,
                "mls": false
            }
        })
        .to_string();
        let _ = self.dispatch_action("nmp.create_account", &action);
    }

    pub fn sign_in_nsec(&self, secret: String) {
        let action = json!({ "SignInNsec": { "secret": secret } }).to_string();
        let _ = self.dispatch_action("nmp.sign_in_nsec", &action);
    }

    // ------------------------------------------------------------------
    // Social actions
    // ------------------------------------------------------------------

    pub fn publish_note(&self, content: &str, reply_to: Option<&str>) -> Result<String, String> {
        let action = json!({
            "PublishNote": {
                "content": content,
                "reply_to_id": reply_to,
                "target": "Auto"
            }
        })
        .to_string();
        self.dispatch_action("nmp.publish", &action)
    }

    pub fn react(&self, event_id: &str, reaction: &str) -> Result<String, String> {
        let action = json!({ "target_event_id": event_id, "reaction": reaction }).to_string();
        self.dispatch_action("nmp.nip25.react", &action)
    }

    pub fn follow(&self, pubkey: &str) -> Result<String, String> {
        let action = json!({ "pubkey": pubkey }).to_string();
        self.dispatch_action("nmp.follow", &action)
    }

    pub fn unfollow(&self, pubkey: &str) -> Result<String, String> {
        let action = json!({ "pubkey": pubkey }).to_string();
        self.dispatch_action("nmp.unfollow", &action)
    }

    pub fn zap(&self, recipient_pubkey: &str, amount_msats: u64, target_event_id: &str) -> Result<String, String> {
        let action = json!({
            "recipient_pubkey": recipient_pubkey,
            "amount_msats": amount_msats,
            "target_event_id": target_event_id,
            "comment": ""
        }).to_string();
        self.dispatch_action("nmp.nip57.zap", &action)
    }

    // ------------------------------------------------------------------
    // Account lifecycle
    // ------------------------------------------------------------------

    pub fn switch_account(&self, pubkey: &str) {
        let action = json!({ "pubkey": pubkey }).to_string();
        let _ = self.dispatch_action("nmp.switch_account", &action);
    }

    pub fn remove_account(&self, pubkey: &str) {
        let action = json!({ "pubkey": pubkey }).to_string();
        let _ = self.dispatch_action("nmp.remove_account", &action);
    }

    pub fn publish_profile(&self, name: &str, about: &str, picture: &str) -> Result<String, String> {
        let mut fields = serde_json::Map::new();
        if !name.is_empty() {
            fields.insert("name".to_string(), Value::String(name.to_string()));
        }
        if !about.is_empty() {
            fields.insert("about".to_string(), Value::String(about.to_string()));
        }
        if !picture.is_empty() {
            fields.insert("picture".to_string(), Value::String(picture.to_string()));
        }
        let action = json!({ "PublishProfile": { "fields": Value::Object(fields) } }).to_string();
        self.dispatch_action("nmp.publish", &action)
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
        unsafe { nmp_app_free_string(ptr) };
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
