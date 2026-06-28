use std::ffi::{c_void, CStr, CString};
use std::ptr;

use nmp_app_chirp::ffi::{
    nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
    nmp_app_chirp_register_group_events,
};
use nmp_app_chirp::{
    nmp_app_cancel_bunker_handshake, nmp_app_chirp_create_new_account,
    nmp_app_chirp_identity_sign_in_nsec, nmp_app_chirp_open_tag_feed, nmp_app_nostrconnect_uri,
    nmp_marmot_register_active, nmp_marmot_unregister, send_dm_spec, zap_identifier_spec, zap_spec,
};
use nmp_app_chirp::{nmp_app_chirp_close_group_discovery, nmp_app_chirp_open_group_discovery};
use nmp_chirp_config::CHIRP_MARMOT_KEYRING_SERVICE_ID;
use nmp_ffi::{
    nmp_app_cancel_action, nmp_app_remove_relay, nmp_app_retry_publish, nmp_app_signin_nsec,
    nmp_free_string,
};
use serde_json::{json, Value};

use crate::runtime::AppRuntime;
use crate::Result;

// #1607: nmp_app_wallet_{connect,disconnect,pay_invoice} deleted from the C ABI.
// All wallet operations now route through nmp_app_dispatch_action (D11).
unsafe extern "C" {
    fn nmp_app_remove_account(app: *mut c_void, identity_id: *const std::ffi::c_char);
    fn nmp_app_signin_bunker(app: *mut c_void, uri: *const std::ffi::c_char, make_active: u8);
    fn nmp_app_switch_active(app: *mut c_void, identity_id: *const std::ffi::c_char);
}

impl AppRuntime {
    pub fn sign_in_nsec(&self, nsec: &str) -> Result<()> {
        self.unregister_marmot();
        self.with_cstr(nsec, |c| nmp_app_signin_nsec(self.app_ptr(), c.as_ptr(), 1))
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
        self.unregister_marmot();
        self.with_cstr(uri, |c| unsafe {
            nmp_app_signin_bunker(self.app_ptr().cast(), c.as_ptr(), 1)
        })
    }

    pub fn cancel_bunker(&self) {
        nmp_app_cancel_bunker_handshake(self.app_ptr());
    }

    pub fn nostrconnect_uri(&self) -> Result<String> {
        let callback =
            CString::new("chirp://nip46").map_err(|_| "callback contains NUL byte".to_string())?;
        let ptr = nmp_app_nostrconnect_uri(self.app_ptr(), callback.as_ptr());
        take_broker_string(ptr, "nostrconnect uri")
    }

    pub fn create_account(&self, name: &str, relays: &[String], mls: bool) -> Result<()> {
        self.unregister_marmot();
        let profile = CString::new(json!({ "name": name }).to_string())
            .map_err(|_| "profile JSON contains NUL byte".to_string())?;
        let relays_json: Vec<Value> = relays
            .iter()
            .map(|url| json!([url, "both,indexer"]))
            .collect();
        let relays = CString::new(Value::Array(relays_json).to_string())
            .map_err(|_| "relays JSON contains NUL byte".to_string())?;
        // Chirp-owned wrapper injects Chirp's seed follows in Rust (#1493); the
        // generic nmp_app_create_new_account auto-follows nobody.
        nmp_app_chirp_create_new_account(self.app_ptr(), profile.as_ptr(), relays.as_ptr(), mls, 1);
        Ok(())
    }

    pub fn switch_account(&self, identity_id: &str) -> Result<()> {
        self.unregister_marmot();
        self.with_cstr(identity_id, |c| unsafe {
            nmp_app_switch_active(self.app_ptr().cast(), c.as_ptr())
        })
    }

    pub fn remove_account(&self, identity_id: &str) -> Result<()> {
        self.unregister_marmot();
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
            nmp_app_chirp_open_tag_feed(self.app_ptr(), c.as_ptr())
        })
    }

    pub fn retry_publish(&self, handle: &str) -> Result<()> {
        self.with_cstr(handle, |c| {
            nmp_app_retry_publish(self.app_ptr(), c.as_ptr())
        })
    }

    /// Cancel an in-flight publish. Addressed by the operation `correlation_id`
    /// (S7, #1754) — the outbox UI's publish handle is also accepted (the
    /// kernel's handle↔correlation index self-maps it).
    pub fn cancel_publish(&self, correlation_id: &str) -> Result<()> {
        self.with_cstr(correlation_id, |c| {
            nmp_app_cancel_action(self.app_ptr(), c.as_ptr())
        })
    }

    // ── NIP-47 wallet commands (#1607 — dispatch_action seam) ───────────────
    //
    // The bespoke nmp_app_wallet_{connect,disconnect,pay_invoice} C-ABI symbols
    // were deleted (D11 — one action door). All three operations now route
    // through `dispatch_action`. The bolt11 double-tap guard moved into
    // `WalletPayInvoiceModule` in nmp-nip47.

    pub fn wallet_connect(&self, uri: &str) -> Result<()> {
        let action = json!({ "Connect": { "uri": uri } });
        let action_json = serde_json::to_string(&action)
            .map_err(|e| format!("serialize wallet.connect action: {e}"))?;
        self.dispatch_action("nmp.wallet.connect", &action_json)
            .map(|_| ())
    }

    pub fn wallet_disconnect(&self) {
        let _ = self.dispatch_action("nmp.wallet.disconnect", "\"Disconnect\"");
    }

    pub fn wallet_pay_invoice(&self, bolt11: &str, amount_msats: Option<&str>) -> Result<()> {
        let amount: Option<u64> = amount_msats.and_then(|s| s.parse().ok());
        let action = json!({ "PayInvoice": { "bolt11": bolt11, "amount_msats": amount } });
        let action_json = serde_json::to_string(&action)
            .map_err(|e| format!("serialize wallet.pay_invoice action: {e}"))?;
        self.dispatch_action("nmp.wallet.pay_invoice", &action_json)
            .map(|_| ())
    }

    pub fn send_dm(&self, recipient_pubkey: &str, content: &str) -> Result<String> {
        let spec = send_dm_spec(recipient_pubkey, content, None);
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    /// Dispatch `nmp.nip57.zap`. Rust owns the asynchronous LNURL fetch and
    /// NWC pay-invoice chain and the exact action JSON shape; the TUI only
    /// forwards user intent and optional contextual inputs.
    pub fn zap(
        &self,
        recipient_pubkey: &str,
        amount_msats: u64,
        target_event_id: Option<&str>,
        comment: Option<&str>,
        lnurl: Option<&str>,
    ) -> Result<String> {
        let spec = zap_spec(
            recipient_pubkey,
            amount_msats,
            target_event_id,
            comment,
            lnurl,
            Vec::new(),
        );
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn zap_identifier(
        &self,
        recipient_identifier: &str,
        amount_msats: u64,
        target_event_id: Option<&str>,
        comment: Option<&str>,
    ) -> Result<String> {
        let spec =
            zap_identifier_spec(recipient_identifier, amount_msats, target_event_id, comment);
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn register_dm_inbox(&self) {
        nmp_app_chirp_register_dm_inbox(self.app_ptr());
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

    pub fn register_group_events(&self, relay: &str, local_id: &str) -> Result<()> {
        // NIP-29 owns only the `["h", local_id]` routing (issue #2187); the
        // consumer declares the kinds. The TUI group view is a chat view, so it
        // asks for kinds [9, 11].
        let request = CString::new(
            json!({
                "group": { "host_relay_url": relay, "local_id": local_id },
                "kinds": [9, 11],
            })
            .to_string(),
        )
        .map_err(|_| "group JSON contains NUL byte".to_string())?;
        nmp_app_chirp_register_group_events(self.app_ptr(), request.as_ptr());
        Ok(())
    }

    pub fn discover_groups(&self, relay: &str) -> Result<String> {
        // Close any prior discovery session before opening a new one (relay switch).
        let old = self.discovery.replace(ptr::null_mut());
        if !old.is_null() {
            nmp_app_chirp_close_group_discovery(old);
        }
        let handle = self.with_cstr(relay, |c| {
            nmp_app_chirp_open_group_discovery(self.app_ptr(), c.as_ptr())
        })?;
        self.discovery.set(handle);
        self.dispatch_action_value("nmp.nip29.discover", &json!({ "relay_url": relay }))
    }

    pub fn create_public_group(
        &self,
        relay: &str,
        local_id: &str,
        name: &str,
        about: Option<&str>,
    ) -> Result<String> {
        let mut body = json!({
            "group": { "host_relay_url": relay, "local_id": local_id },
            "name": name,
        });
        if let Some(about) = about.map(str::trim).filter(|value| !value.is_empty()) {
            body["about"] = Value::String(about.to_string());
        }
        let correlation_id = self.dispatch_action_value("nmp.nip29.create_public_group", &body)?;
        self.register_group_events(relay, local_id)?;
        Ok(correlation_id)
    }

    pub fn join_group(&self, relay: &str, local_id: &str) -> Result<String> {
        self.dispatch_action_value(
            "nmp.nip29.join",
            &json!({ "group": { "host_relay_url": relay, "local_id": local_id } }),
        )
    }

    pub fn post_group_message(&self, relay: &str, local_id: &str, content: &str) -> Result<String> {
        // A group chat message is just a kind:9 event published to the group;
        // the generic publish action injects the `h` / `previous` envelope.
        self.dispatch_action_value(
            "nmp.nip29.publish_group_event",
            &json!({
                "group": { "host_relay_url": relay, "local_id": local_id },
                "kind": 9,
                "content": content,
            }),
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

    pub fn marmot_register_active(&self) -> Result<()> {
        if !self.marmot.get().is_null() {
            return Ok(());
        }
        let dir = CString::new(marmot_db_dir())
            .map_err(|_| "marmot DB path contains NUL byte".to_string())?;
        let svc = CString::new(CHIRP_MARMOT_KEYRING_SERVICE_ID)
            .map_err(|_| "marmot service id contains NUL byte".to_string())?;
        let handle = nmp_marmot_register_active(self.app_ptr(), dir.as_ptr(), svc.as_ptr());
        if handle.is_null() {
            return Err("no active Marmot identity".to_string());
        }
        self.marmot.set(handle);
        Ok(())
    }

    pub fn marmot_dispatch_json(&self, action: Value) -> Result<String> {
        self.marmot_register_active()?;
        // ADR-0025 PR 3 (2026-05-23): the legacy bespoke `nmp_marmot_dispatch`
        // C-ABI symbol was deleted. iOS routes Marmot ops through
        // `nmp_app_dispatch_action("nmp.marmot", …)`, but that path returns
        // only `{"correlation_id":"…"}` — the rich per-op envelope the TUI
        // surfaces to the operator (`ok`, per-op detail) is dropped. The
        // Rust-native `MarmotHandle::dispatch` accessor reaches the SAME
        // `ops::dispatch` entry point both seams use, without any C-ABI in
        // the loop. See `crates/chirp-repl/src/app.rs::marmot_dispatch` for
        // the parallel migration.
        //
        // SAFETY: `marmot_register_active` guarantees `self.marmot.get()` is
        // non-null (it returns `Err` otherwise); the handle was boxed by
        // `nmp_marmot_register*` and remains valid until `unregister_marmot`.
        // The TUI runs each command from the single async runtime task that
        // owns `AppRuntime`, so no other thread mutates the pointer.
        let handle_ptr = self.marmot.get();
        let handle = unsafe { &*handle_ptr };
        let value = handle.dispatch(&action);
        Ok(value.to_string())
    }

    pub fn marmot_create_group(
        &self,
        name: &str,
        relays: &[String],
        invitee_text: Option<&str>,
    ) -> Result<String> {
        let mut body = json!({
            "op": "create_group",
            "name": name,
            "relays": relays,
        });
        if let Some(invitees) = invitee_text
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            body["invitee_text"] = Value::String(invitees.to_string());
        }
        self.marmot_dispatch_json(body)
    }

    pub fn marmot_snapshot_text(&self) -> Result<String> {
        self.marmot_register_active()?;
        // V-107 / ADR-0039: use the Rust-native accessor on the MarmotHandle
        // instead of the deprecated C-ABI pull symbol `nmp_marmot_snapshot`.
        // Same data path as the push projection `"nmp.marmot.snapshot"` on the
        // SnapshotFrame.
        //
        // SAFETY: `self.marmot.get()` is non-null (guaranteed by
        // `marmot_register_active()` returning Ok above).
        let handle = unsafe { &*self.marmot.get() };
        serde_json::to_string(&handle.snapshot_rust())
            .map_err(|e| format!("marmot snapshot serialize: {e}"))
    }

    fn unregister_marmot(&self) {
        if !self.marmot.get().is_null() {
            nmp_marmot_unregister(self.marmot.get());
            self.marmot.set(ptr::null_mut());
        }
    }
}

fn marmot_db_dir() -> String {
    crate::keyring::chirp_data_dir()
        .map(|p| p.join("marmot"))
        .and_then(|p| std::fs::create_dir_all(&p).ok().map(|_| p))
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("chirp-tui-marmot-{}", std::process::id()))
        })
        .to_string_lossy()
        .into_owned()
}

fn take_broker_string(ptr: *mut std::ffi::c_char, label: &str) -> Result<String> {
    if ptr.is_null() {
        return Err(format!("{label} returned null"));
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(ptr);
    Ok(text)
}
