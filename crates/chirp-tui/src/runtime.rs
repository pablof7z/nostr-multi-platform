use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::Receiver;

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_unregister, ChirpHandle,
};
use nmp_core::{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_open_author,
    nmp_app_open_thread, nmp_app_open_timeline, nmp_app_start, NmpApp,
};
use serde_json::{json, Value};

use crate::bridge::{self, NmpEvent, NmpUpdateBridge};
use crate::Result;

pub struct AppRuntime {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    update_bridge: Option<Box<NmpUpdateBridge>>,
}

impl AppRuntime {
    pub fn new() -> Result<(Self, Receiver<NmpEvent>)> {
        let app = nmp_core::nmp_app_new();
        if app.is_null() {
            return Err("nmp_app_new returned null".to_string());
        }

        let chirp = nmp_app_chirp_register(app, ptr::null());
        if chirp.is_null() {
            nmp_app_free(app);
            return Err("nmp_app_chirp_register returned null".to_string());
        }

        let (mut bridge, rx) = NmpUpdateBridge::channel();
        NmpUpdateBridge::register(app, &mut bridge);
        nmp_app_start(app, 0, 200, 10);
        nmp_app_open_timeline(app);

        Ok((
            Self {
                app,
                chirp,
                update_bridge: Some(bridge),
            },
            rx,
        ))
    }

    pub fn add_relay(&self, url: &str, role: &str) -> Result<()> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_core::nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }

    pub fn open_thread(&self, event_id: &str) -> Result<()> {
        self.with_cstr(event_id, |c| nmp_app_open_thread(self.app, c.as_ptr()))
    }

    pub fn open_author(&self, pubkey: &str) -> Result<()> {
        self.with_cstr(pubkey, |c| nmp_app_open_author(self.app, c.as_ptr()))
    }

    pub fn publish_note(&self, content: &str, reply_to: Option<&str>) -> Result<()> {
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

    pub fn react(&self, event_id: &str, reaction: &str) -> Result<()> {
        let action = json!({ "target_event_id": event_id, "reaction": reaction }).to_string();
        self.dispatch_action("chirp.react", &action)
    }

    pub fn follow(&self, pubkey: &str, add: bool) -> Result<()> {
        let action = json!({ "pubkey": pubkey }).to_string();
        let namespace = if add {
            "chirp.follow"
        } else {
            "chirp.unfollow"
        };
        self.dispatch_action(namespace, &action)
    }

    pub fn chirp_snapshot(&self) -> Option<Value> {
        if self.chirp.is_null() {
            return None;
        }
        let ptr = nmp_app_chirp_snapshot(self.chirp);
        if ptr.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        nmp_app_chirp_snapshot_free(ptr);
        serde_json::from_str(&text).ok()
    }

    fn dispatch_action(&self, namespace: &str, action_json: &str) -> Result<()> {
        let namespace = CString::new(namespace)
            .map_err(|_| "action namespace contains NUL byte".to_string())?;
        let action =
            CString::new(action_json).map_err(|_| "action JSON contains NUL byte".to_string())?;
        let ptr = nmp_app_dispatch_action(self.app, namespace.as_ptr(), action.as_ptr());
        if ptr.is_null() {
            return Err("action dispatch returned null".to_string());
        }
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        nmp_app_free_string(ptr);
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("action dispatch returned invalid JSON: {e}"))?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            Err(error.to_string())
        } else {
            Ok(())
        }
    }

    fn with_cstr<T>(&self, value: &str, f: impl FnOnce(&CString) -> T) -> Result<T> {
        let c = CString::new(value).map_err(|_| "string contains NUL byte".to_string())?;
        Ok(f(&c))
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        if !self.app.is_null() {
            bridge::unregister(self.app);
        }
        self.update_bridge.take();
        if !self.chirp.is_null() {
            nmp_app_chirp_unregister(self.chirp);
            self.chirp = ptr::null_mut();
        }
        if !self.app.is_null() {
            nmp_app_free(self.app);
            self.app = ptr::null_mut();
        }
    }
}
