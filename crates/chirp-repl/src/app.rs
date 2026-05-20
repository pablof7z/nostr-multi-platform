use std::ffi::{CStr, CString};
use std::ptr;

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_unregister, ChirpHandle,
};
use nmp_core::{
    nmp_app_add_relay, nmp_app_create_new_account, nmp_app_dispatch_action, nmp_app_follow,
    nmp_app_free, nmp_app_free_string, nmp_app_open_author, nmp_app_open_firehose_tag,
    nmp_app_open_thread, nmp_app_open_timeline, nmp_app_react, nmp_app_remove_relay,
    nmp_app_signin_nsec, nmp_app_start, nmp_app_unfollow, NmpApp,
};
use serde_json::{json, Value};

use crate::Result;

pub struct AppRuntime {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
}

impl std::fmt::Debug for AppRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRuntime")
            .field("app", &(!self.app.is_null()))
            .field("chirp", &(!self.chirp.is_null()))
            .finish()
    }
}

impl AppRuntime {
    pub fn new() -> Self {
        let app = nmp_core::nmp_app_new();
        let chirp = nmp_app_chirp_register(app, ptr::null());
        nmp_app_start(app, 30, 200, 4);
        Self { app, chirp }
    }

    pub fn reset_relays(&self, old: &[String], new: &[String], role: &str) -> Result<()> {
        for url in old {
            self.with_cstr(url, |c| nmp_app_remove_relay(self.app, c.as_ptr()))?;
        }
        for url in new {
            self.add_relay(url, role)?;
        }
        Ok(())
    }

    pub fn add_relay(&self, url: &str, role: &str) -> Result<()> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }

    pub fn sign_in_nsec(&self, nsec: &str) -> Result<()> {
        self.with_cstr(nsec, |c| nmp_app_signin_nsec(self.app, c.as_ptr()))
    }

    pub fn create_account(&self, profile_json: &str, relays_json: &str, mls: bool) -> Result<()> {
        self.with_cstr(profile_json, |profile| {
            self.with_cstr(relays_json, |relays| {
                nmp_app_create_new_account(self.app, profile.as_ptr(), relays.as_ptr(), mls)
            })
        })?
    }

    pub fn open_timeline(&self) {
        nmp_app_open_timeline(self.app);
    }

    pub fn open_author(&self, pubkey: &str) -> Result<()> {
        self.with_cstr(pubkey, |c| nmp_app_open_author(self.app, c.as_ptr()))
    }

    pub fn open_thread(&self, event_id: &str) -> Result<()> {
        self.with_cstr(event_id, |c| nmp_app_open_thread(self.app, c.as_ptr()))
    }

    pub fn open_tag(&self, tag: &str) -> Result<()> {
        self.with_cstr(tag, |c| nmp_app_open_firehose_tag(self.app, c.as_ptr()))
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
        self.with_cstr(event_id, |id| {
            self.with_cstr(reaction, |reaction| {
                nmp_app_react(self.app, id.as_ptr(), reaction.as_ptr())
            })
        })?
    }

    pub fn follow(&self, pubkey: &str, add: bool) -> Result<()> {
        self.with_cstr(pubkey, |c| {
            if add {
                nmp_app_follow(self.app, c.as_ptr());
            } else {
                nmp_app_unfollow(self.app, c.as_ptr());
            }
        })
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

impl Default for AppRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
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
