use std::ffi::{c_void, CStr, CString};
use std::ptr;

use nmp_app_chirp::ffi::{
    nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
    nmp_app_chirp_register_group_chat, nmp_app_chirp_register_group_discovery,
};
use nmp_app_chirp::{
    nmp_app_cancel_bunker_handshake, nmp_app_chirp_identity_sign_in_nsec,
    nmp_app_chirp_marmot_dispatch, nmp_app_chirp_marmot_register_active,
    nmp_app_chirp_marmot_snapshot, nmp_app_chirp_marmot_string_free,
    nmp_app_chirp_marmot_unregister, nmp_app_nostrconnect_uri, nmp_broker_free_string,
};
use nmp_core::{
    nmp_app_cancel_publish, nmp_app_create_new_account, nmp_app_open_firehose_tag,
    nmp_app_remove_relay, nmp_app_retry_publish, nmp_app_signin_nsec,
};
use serde_json::{json, Value};

use crate::runtime::AppRuntime;
use crate::Result;

unsafe extern "C" {
    fn nmp_app_remove_account(app: *mut c_void, identity_id: *const std::ffi::c_char);
    fn nmp_app_signin_bunker(app: *mut c_void, uri: *const std::ffi::c_char);
    fn nmp_app_switch_active(app: *mut c_void, identity_id: *const std::ffi::c_char);
    fn nmp_app_wallet_connect(app: *mut c_void, uri: *const std::ffi::c_char);
    fn nmp_app_wallet_disconnect(app: *mut c_void);
    fn nmp_app_wallet_pay_invoice(
        app: *mut c_void,
        bolt11: *const std::ffi::c_char,
        amount_msats_or_null: *const std::ffi::c_char,
    );
}

impl AppRuntime {
    pub fn sign_in_nsec(&self, nsec: &str) -> Result<()> {
        self.with_cstr(nsec, |c| nmp_app_signin_nsec(self.app_ptr(), c.as_ptr()))
    }

    pub fn sign_in_nsec_with_marmot(&self, nsec: &str) -> Result<()> {
        self.unregister_marmot();
        let secret = CString::new(nsec).map_err(|_| "secret contains NUL byte".to_string())?;
        let dir = CString::new(marmot_db_dir())
            .map_err(|_| "marmot DB path contains NUL byte".to_string())?;
        let handle =
            nmp_app_chirp_identity_sign_in_nsec(self.app_ptr(), secret.as_ptr(), dir.as_ptr());
        if handle.is_null() {
            return Err("marmot sign-in returned null".to_string());
        }
        self.marmot.set(handle);
        Ok(())
    }

    pub fn sign_in_bunker(&self, uri: &str) -> Result<()> {
        self.with_cstr(uri, |c| unsafe {
            nmp_app_signin_bunker(self.app_ptr().cast(), c.as_ptr())
        })
    }

    pub fn cancel_bunker(&self) {
        nmp_app_cancel_bunker_handshake(self.app_ptr());
    }

    pub fn nostrconnect_uri(&self) -> Result<String> {
        let callback = CString::new("chirp://nip46").expect("static callback has no NUL");
        let ptr = nmp_app_nostrconnect_uri(self.app_ptr(), ptr::null(), callback.as_ptr());
        take_broker_string(ptr, "nostrconnect uri")
    }

    pub fn create_account(&self, name: &str, relays: &[String], mls: bool) -> Result<()> {
        let profile = CString::new(json!({ "name": name }).to_string())
            .map_err(|_| "profile JSON contains NUL byte".to_string())?;
        let relays_json: Vec<Value> = relays
            .iter()
            .map(|url| json!([url, "both,indexer"]))
            .collect();
        let relays = CString::new(Value::Array(relays_json).to_string())
            .map_err(|_| "relays JSON contains NUL byte".to_string())?;
        nmp_app_create_new_account(self.app_ptr(), profile.as_ptr(), relays.as_ptr(), mls);
        Ok(())
    }

    pub fn switch_account(&self, identity_id: &str) -> Result<()> {
        self.with_cstr(identity_id, |c| unsafe {
            nmp_app_switch_active(self.app_ptr().cast(), c.as_ptr())
        })
    }

    pub fn remove_account(&self, identity_id: &str) -> Result<()> {
        self.with_cstr(identity_id, |c| unsafe {
            nmp_app_remove_account(self.app_ptr().cast(), c.as_ptr())
        })
    }

    pub fn publish_profile_fields(&self, fields: Value) -> Result<String> {
        self.dispatch_action_value(
            "nmp.publish",
            &json!({ "PublishProfile": { "fields": fields } }),
        )
    }

    pub fn remove_relay(&self, url: &str) -> Result<()> {
        self.with_cstr(url, |c| nmp_app_remove_relay(self.app_ptr(), c.as_ptr()))
    }

    pub fn publish_dm_relay_list(&self, relays: Vec<String>) -> Result<String> {
        self.dispatch_action_value("nmp.nip17.publish_relay_list", &json!({ "relays": relays }))
    }

    pub fn open_tag(&self, tag: &str) -> Result<()> {
        self.with_cstr(tag, |c| {
            nmp_app_open_firehose_tag(self.app_ptr(), c.as_ptr())
        })
    }

    pub fn retry_publish(&self, handle: &str) -> Result<()> {
        self.with_cstr(handle, |c| {
            nmp_app_retry_publish(self.app_ptr(), c.as_ptr())
        })
    }

    pub fn cancel_publish(&self, handle: &str) -> Result<()> {
        self.with_cstr(handle, |c| {
            nmp_app_cancel_publish(self.app_ptr(), c.as_ptr())
        })
    }

    pub fn wallet_connect(&self, uri: &str) -> Result<()> {
        self.with_cstr(uri, |c| unsafe {
            nmp_app_wallet_connect(self.app_ptr().cast(), c.as_ptr())
        })
    }

    pub fn wallet_disconnect(&self) {
        unsafe { nmp_app_wallet_disconnect(self.app_ptr().cast()) };
    }

    pub fn wallet_pay_invoice(&self, bolt11: &str, amount_msats: Option<&str>) -> Result<()> {
        let bolt11 = CString::new(bolt11).map_err(|_| "invoice contains NUL byte".to_string())?;
        let amount = amount_msats
            .map(CString::new)
            .transpose()
            .map_err(|_| "amount contains NUL byte".to_string())?;
        unsafe {
            nmp_app_wallet_pay_invoice(
                self.app_ptr().cast(),
                bolt11.as_ptr(),
                amount.as_ref().map_or(ptr::null(), |a| a.as_ptr()),
            );
        }
        Ok(())
    }

    pub fn send_dm(&self, recipient_pubkey: &str, content: &str) -> Result<String> {
        self.dispatch_action_value(
            "nmp.nip17.send",
            &json!({ "recipient_pubkey": recipient_pubkey, "content": content }),
        )
    }

    pub fn register_dm_inbox(&self) {
        nmp_app_chirp_register_dm_inbox(self.app_ptr(), ptr::null());
    }

    pub fn register_follow_list(&self, active_pubkey: Option<&str>) -> Result<()> {
        if let Some(pubkey) = active_pubkey {
            self.with_cstr(pubkey, |c| {
                nmp_app_chirp_register_follow_list(self.app_ptr(), c.as_ptr())
            })
        } else {
            nmp_app_chirp_register_follow_list(self.app_ptr(), ptr::null());
            Ok(())
        }
    }

    pub fn register_group_chat(&self, relay: &str, local_id: &str) -> Result<()> {
        let group =
            CString::new(json!({ "host_relay_url": relay, "local_id": local_id }).to_string())
                .map_err(|_| "group JSON contains NUL byte".to_string())?;
        nmp_app_chirp_register_group_chat(self.app_ptr(), group.as_ptr());
        Ok(())
    }

    pub fn discover_groups(&self, relay: &str) -> Result<String> {
        self.with_cstr(relay, |c| {
            nmp_app_chirp_register_group_discovery(self.app_ptr(), c.as_ptr())
        })?;
        self.dispatch_action_value("nmp.nip29.discover", &json!({ "relay_url": relay }))
    }

    pub fn join_group(&self, relay: &str, local_id: &str) -> Result<String> {
        self.dispatch_action_value(
            "nmp.nip29.join",
            &json!({ "group": { "host_relay_url": relay, "local_id": local_id } }),
        )
    }

    pub fn post_group_message(&self, relay: &str, local_id: &str, content: &str) -> Result<String> {
        self.dispatch_action_value(
            "nmp.nip29.post_chat_message",
            &json!({ "group": { "host_relay_url": relay, "local_id": local_id }, "content": content }),
        )
    }

    pub fn react_group_message(
        &self,
        relay: &str,
        local_id: &str,
        event_id: &str,
        author_pubkey: Option<&str>,
        reaction: &str,
    ) -> Result<String> {
        let mut body = json!({
            "group": { "host_relay_url": relay, "local_id": local_id },
            "target_event_id": event_id,
            "content": reaction,
        });
        if let Some(author) = author_pubkey {
            body["target_author_pubkey"] = Value::String(author.to_string());
        }
        self.dispatch_action_value("nmp.nip29.react_in_group", &body)
    }

    pub fn reply_group_message(
        &self,
        relay: &str,
        local_id: &str,
        parent_event_id: &str,
        content: &str,
    ) -> Result<String> {
        self.dispatch_action_value(
            "nmp.nip29.comment_in_group",
            &json!({
                "group": { "host_relay_url": relay, "local_id": local_id },
                "parent_event_id": parent_event_id,
                "content": content
            }),
        )
    }

    pub fn marmot_register_active(&self) -> Result<()> {
        if !self.marmot.get().is_null() {
            return Ok(());
        }
        let dir = CString::new(marmot_db_dir())
            .map_err(|_| "marmot DB path contains NUL byte".to_string())?;
        let handle = nmp_app_chirp_marmot_register_active(self.app_ptr(), dir.as_ptr());
        if handle.is_null() {
            return Err("no active Marmot identity".to_string());
        }
        self.marmot.set(handle);
        Ok(())
    }

    pub fn marmot_dispatch_json(&self, action: Value) -> Result<String> {
        self.marmot_register_active()?;
        let action = CString::new(action.to_string())
            .map_err(|_| "marmot action JSON contains NUL byte".to_string())?;
        let ptr = nmp_app_chirp_marmot_dispatch(self.marmot.get(), action.as_ptr());
        take_marmot_string(ptr, "marmot dispatch")
    }

    pub fn marmot_snapshot_text(&self) -> Result<String> {
        self.marmot_register_active()?;
        let ptr = nmp_app_chirp_marmot_snapshot(self.marmot.get());
        take_marmot_string(ptr, "marmot snapshot")
    }

    fn unregister_marmot(&self) {
        if !self.marmot.get().is_null() {
            nmp_app_chirp_marmot_unregister(self.marmot.get());
            self.marmot.set(ptr::null_mut());
        }
    }
}

fn marmot_db_dir() -> String {
    std::env::temp_dir()
        .join(format!("chirp-tui-marmot-{}", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

fn take_marmot_string(ptr: *mut std::ffi::c_char, label: &str) -> Result<String> {
    if ptr.is_null() {
        return Err(format!("{label} returned null"));
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_app_chirp_marmot_string_free(ptr);
    Ok(text)
}

fn take_broker_string(ptr: *mut std::ffi::c_char, label: &str) -> Result<String> {
    if ptr.is_null() {
        return Err(format!("{label} returned null"));
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_broker_free_string(ptr);
    Ok(text)
}
