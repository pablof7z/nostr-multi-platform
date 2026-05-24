use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::Receiver;

use nmp_app_chirp::ffi::{nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list};
use nmp_app_chirp::{
    nmp_marmot_unregister, nmp_app_chirp_register, nmp_app_chirp_snapshot,
    nmp_app_chirp_snapshot_free, nmp_app_chirp_unregister, nmp_signer_broker_init, ChirpHandle,
    MarmotHandle,
};
use nmp_ffi::{
    nmp_app_claim_profile, nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string,
    nmp_app_open_author, nmp_app_open_thread, nmp_app_open_timeline, nmp_app_release_profile,
    nmp_app_start, NmpApp,
};
use serde_json::{json, Value};

use crate::bridge::{self, NmpEvent, NmpUpdateBridge};
use crate::Result;

const VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX: &str = "chirp-tui.visible-author";
const RELATION_COUNT_CLAIMS_UNAVAILABLE: &str =
    "relation-count render claims are not available in nmp-core yet";

pub struct AppRuntime {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    pub(crate) marmot: Cell<*mut MarmotHandle>,
    update_bridge: Option<Box<NmpUpdateBridge>>,
}

impl AppRuntime {
    #[must_use]
    pub fn new() -> Result<(Self, Receiver<NmpEvent>)> {
        let app = nmp_ffi::nmp_app_new();
        if app.is_null() {
            return Err("nmp_app_new returned null".to_string());
        }
        nmp_signer_broker_init(app);

        let chirp = nmp_app_chirp_register(app, ptr::null());
        if chirp.is_null() {
            nmp_app_free(app);
            return Err("nmp_app_chirp_register returned null".to_string());
        }

        let (mut bridge, rx) = NmpUpdateBridge::channel();
        NmpUpdateBridge::register(app, &mut bridge);
        nmp_app_chirp_register_dm_inbox(app);
        nmp_app_chirp_register_follow_list(app, ptr::null());
        nmp_app_start(app, 0, 200, 10);
        nmp_app_open_timeline(app);

        Ok((
            Self {
                app,
                chirp,
                marmot: Cell::new(ptr::null_mut()),
                update_bridge: Some(bridge),
            },
            rx,
        ))
    }

    pub fn add_relay(&self, url: &str, role: &str) -> Result<()> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }

    pub fn open_thread(&self, event_id: &str) -> Result<()> {
        self.with_cstr(event_id, |c| nmp_app_open_thread(self.app, c.as_ptr()))
    }

    pub fn open_author(&self, pubkey: &str) -> Result<()> {
        self.with_cstr(pubkey, |c| nmp_app_open_author(self.app, c.as_ptr()))
    }

    pub fn claim_visible_author_profile(&self, pubkey: &str) -> Result<()> {
        self.with_visible_author_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_claim_profile(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    pub fn release_visible_author_profile(&self, pubkey: &str) -> Result<()> {
        self.with_visible_author_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_release_profile(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    pub fn claim_visible_note_relation_counts(&self, event_id: &str) -> Result<()> {
        validate_hex64("event id", event_id)?;
        Err(RELATION_COUNT_CLAIMS_UNAVAILABLE.to_string())
    }

    pub fn release_visible_note_relation_counts(&self, event_id: &str) -> Result<()> {
        validate_hex64("event id", event_id)?;
        Err(RELATION_COUNT_CLAIMS_UNAVAILABLE.to_string())
    }

    pub fn publish_note(&self, content: &str, reply_to: Option<&str>) -> Result<String> {
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

    pub fn react(&self, event_id: &str, reaction: &str) -> Result<String> {
        let action = json!({ "target_event_id": event_id, "reaction": reaction }).to_string();
        self.dispatch_action("nmp.nip25.react", &action)
    }

    pub fn follow(&self, pubkey: &str, add: bool) -> Result<String> {
        let action = json!({ "pubkey": pubkey }).to_string();
        let namespace = if add {
            "nmp.follow"
        } else {
            "nmp.unfollow"
        };
        self.dispatch_action(namespace, &action)
    }

    pub fn ack_action_stage(&self, correlation_id: &str) -> Result<()> {
        self.with_cstr(correlation_id, |c| {
            nmp_ffi::nmp_app_ack_action_stage(self.app, c.as_ptr())
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

    pub fn dispatch_action_value(&self, namespace: &str, action: &Value) -> Result<String> {
        self.dispatch_action(namespace, &action.to_string())
    }

    pub(crate) fn app_ptr(&self) -> *mut NmpApp {
        self.app
    }

    pub(crate) fn dispatch_action(&self, namespace: &str, action_json: &str) -> Result<String> {
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
        parse_dispatch_envelope(&value)
    }

    pub(crate) fn with_cstr<T>(&self, value: &str, f: impl FnOnce(&CString) -> T) -> Result<T> {
        let c = CString::new(value).map_err(|_| "string contains NUL byte".to_string())?;
        Ok(f(&c))
    }

    fn with_visible_author_profile_args(
        &self,
        pubkey: &str,
        f: impl FnOnce(&CString, &CString),
    ) -> Result<()> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let consumer_id = visible_author_profile_consumer_id(pubkey)?;
        let pubkey = CString::new(pubkey).map_err(|_| "pubkey contains NUL byte".to_string())?;
        let consumer_id = CString::new(consumer_id)
            .map_err(|_| "profile consumer id contains NUL byte".to_string())?;
        f(&pubkey, &consumer_id);
        Ok(())
    }
}

fn parse_dispatch_envelope(value: &Value) -> Result<String> {
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
        if !self.marmot.get().is_null() {
            nmp_marmot_unregister(self.marmot.get());
            self.marmot.set(ptr::null_mut());
        }
        if !self.app.is_null() {
            nmp_app_free(self.app);
            self.app = ptr::null_mut();
        }
    }
}

fn visible_author_profile_consumer_id(pubkey: &str) -> Result<String> {
    validate_hex64("pubkey", pubkey)?;
    Ok(format!("{VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX}:{pubkey}"))
}

fn validate_hex64(label: &str, value: &str) -> Result<()> {
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{label} must be 64 hex characters"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const EVENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn visible_author_profile_consumer_id_is_stable() {
        assert_eq!(
            visible_author_profile_consumer_id(ALICE).unwrap(),
            format!("{VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX}:{ALICE}")
        );
    }

    #[test]
    fn visible_author_profile_claims_reject_invalid_pubkeys() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(
            runtime.claim_visible_author_profile("not-a-pubkey"),
            Err("pubkey must be 64 hex characters".to_string())
        );
        assert_eq!(
            runtime.release_visible_author_profile("not-a-pubkey"),
            Err("pubkey must be 64 hex characters".to_string())
        );
    }

    #[test]
    fn visible_author_profile_claim_release_are_idempotent() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(runtime.claim_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.claim_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.release_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.release_visible_author_profile(ALICE), Ok(()));
    }

    #[test]
    fn note_relation_count_claim_seam_is_explicitly_unavailable() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(
            runtime.claim_visible_note_relation_counts(EVENT),
            Err(RELATION_COUNT_CLAIMS_UNAVAILABLE.to_string())
        );
        assert_eq!(
            runtime.release_visible_note_relation_counts(EVENT),
            Err(RELATION_COUNT_CLAIMS_UNAVAILABLE.to_string())
        );
        assert_eq!(
            runtime.claim_visible_note_relation_counts("bad"),
            Err("event id must be 64 hex characters".to_string())
        );
    }

    #[test]
    fn dispatch_envelope_requires_correlation_id_or_error() {
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"correlation_id": "abc"})),
            Ok("abc".to_string())
        );
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"error": "bad action"})),
            Err("bad action".to_string())
        );
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"ok": true})),
            Err("action dispatch envelope missing correlation_id".to_string())
        );
    }
}
