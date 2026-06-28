//! Action-dispatch methods for [`AppRuntime`] (wallet / social / account /
//! relay / publish-lifecycle).
//!
//! Split out of `bridge.rs` so that file stays under the 500-LOC hard ceiling
//! (AGENTS.md). This is the same inherent `impl AppRuntime` — Rust unifies the
//! impl across files. As a child module of `bridge` it can reach the private
//! `app` / `client` fields and the `dispatch_action` helper directly.

use std::ffi::CString;

use nmp_nip01::NoteRecord;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_cancel_action, nmp_app_remove_account, nmp_app_remove_relay,
    nmp_app_retry_publish, nmp_app_switch_active,
};

use super::AppRuntime;

impl AppRuntime {
    // ------------------------------------------------------------------
    // Wallet actions (NIP-47 NWC)
    // ------------------------------------------------------------------
    //
    // ── NIP-47 wallet commands (#1607 — dispatch_action seam) ──────────────
    //
    // The bespoke nmp_app_wallet_{connect,disconnect} C-ABI symbols were
    // deleted (D11 — one action door). Both operations now route through
    // nmp_app_dispatch_action.

    pub fn wallet_connect(&self, nwc_uri: &str) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let action = serde_json::json!({ "Connect": { "uri": nwc_uri } });
        let action_json = serde_json::to_string(&action)
            .map_err(|e| format!("serialize wallet.connect action: {e}"))?;
        self.dispatch_action("nmp.wallet.connect", &action_json)
    }

    pub fn wallet_disconnect(&self) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        self.dispatch_action("nmp.wallet.disconnect", "\"Disconnect\"")
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

    pub fn zap(
        &self,
        recipient_pubkey: &str,
        amount_msats: u64,
        target_event_id: &str,
    ) -> Result<String, String> {
        self.client
            .zap(recipient_pubkey, amount_msats, target_event_id, "")
    }

    // ------------------------------------------------------------------
    // Account lifecycle
    // ------------------------------------------------------------------

    pub fn switch_account(&self, pubkey: &str) {
        // Canonical account path: the dedicated C-ABI symbol (matches Android/
        // TUI), NOT the dead `nmp.switch_account` JSON doorway. Fire-and-forget
        // UI action: silent return on null app or NUL byte in the pubkey.
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(pubkey) {
            nmp_app_switch_active(self.app, c.as_ptr());
        }
    }

    pub fn remove_account(&self, pubkey: &str) {
        // Canonical account path: the dedicated C-ABI symbol (matches Android/
        // TUI), NOT the dead `nmp.remove_account` JSON doorway. Fire-and-forget
        // UI action: silent return on null app or NUL byte in the pubkey.
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(pubkey) {
            nmp_app_remove_account(self.app, c.as_ptr());
        }
    }

    pub fn publish_profile(
        &self,
        name: &str,
        about: &str,
        picture: &str,
    ) -> Result<String, String> {
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

    /// Cancel an in-flight publish, addressed by the operation `correlation_id`
    /// (S7, #1754). The outbox UI's publish handle is also accepted (the
    /// kernel's handle↔correlation index self-maps it).
    pub fn cancel_publish(&self, correlation_id: &str) {
        if self.app.is_null() {
            return;
        }
        if let Ok(c) = CString::new(correlation_id) {
            unsafe { nmp_app_cancel_action(self.app, c.as_ptr()) };
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
}
