//! Account-lifecycle methods on [`AppRuntime`] — split out of `bridge.rs` to
//! keep that file under the 500-LOC hard ceiling (#1493). This is a second
//! `impl AppRuntime` block; Rust unifies the inherent impl across files.

use std::collections::HashMap;
use std::ffi::CString;

use nmp_app_chirp::nmp_app_chirp_create_new_account;
use serde_json::json;

use crate::bridge::AppRuntime;

impl AppRuntime {
    /// Create a new local account.
    ///
    /// Routes through the Chirp-owned C-ABI wrapper (`nmp_app_chirp_create_new_account`),
    /// NOT the generic create-account action path, so the fresh account
    /// auto-follows Chirp's seed set — injected in Rust from `nmp-chirp-config`
    /// (#1493). The seed pubkeys never transit this shell.
    pub fn create_account(&self, profile: HashMap<String, String>, relays: Vec<(String, String)>) {
        let relays_json = json!(relays.iter().map(|(u, r)| json!([u, r])).collect::<Vec<_>>());
        let (Some(profile_c), Ok(relays_c)) = (
            serde_json::to_string(&profile)
                .ok()
                .and_then(|s| CString::new(s).ok()),
            CString::new(relays_json.to_string()),
        ) else {
            return;
        };
        if !self.app_ptr().is_null() {
            nmp_app_chirp_create_new_account(
                self.app_ptr(),
                profile_c.as_ptr(),
                relays_c.as_ptr(),
                false,
                1,
            );
        }
    }
}
