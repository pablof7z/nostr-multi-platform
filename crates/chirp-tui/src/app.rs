use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::Receiver;

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_unregister, ChirpHandle,
};
use nmp_core::{nmp_app_free, nmp_app_open_timeline, nmp_app_start, NmpApp};
use serde_json::Value;

use crate::bridge::{self, NmpEvent, NmpUpdateBridge};
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Feed,
    Detail,
    Profile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub focused: Pane,
    pub tab: &'static str,
    pub update_count: u64,
    pub blocks: usize,
    pub cards: usize,
    pub status: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focused: Pane::Feed,
            tab: "home",
            update_count: 0,
            blocks: 0,
            cards: 0,
            status: "starting NMP runtime".to_string(),
        }
    }
}

impl AppState {
    pub fn apply_nmp_event(&mut self, runtime: &AppRuntime, event: NmpEvent) {
        self.update_count += 1;
        if let Some(snapshot) = runtime.chirp_snapshot() {
            self.blocks = snapshot
                .get("blocks")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            self.cards = snapshot
                .get("cards")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
        }
        self.status = format!(
            "received NMP update #{} ({} bytes)",
            self.update_count,
            event.payload.len()
        );
    }

    pub fn focus(&mut self, pane: Pane) {
        self.focused = pane;
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_changes_active_pane() {
        let mut state = AppState::default();
        state.focus(Pane::Profile);
        assert_eq!(state.focused, Pane::Profile);
    }
}
