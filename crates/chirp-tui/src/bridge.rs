use std::ffi::CStr;
use std::sync::mpsc::{self, Receiver, Sender};

use nmp_core::NmpApp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NmpEvent {
    pub payload: String,
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
        nmp_core::nmp_app_set_update_callback(app, context, Some(on_update));
    }
}

pub fn unregister(app: *mut NmpApp) {
    nmp_core::nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
}

extern "C" fn on_update(context: *mut std::ffi::c_void, payload: *const std::ffi::c_char) {
    if context.is_null() || payload.is_null() {
        return;
    }

    let bridge = unsafe { &*(context as *const NmpUpdateBridge) };
    let text = unsafe { CStr::from_ptr(payload) }
        .to_string_lossy()
        .into_owned();
    let _ = bridge.tx.send(NmpEvent { payload: text });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_returns_sender_backed_receiver() {
        let (bridge, rx) = NmpUpdateBridge::channel();
        bridge
            .tx
            .send(NmpEvent {
                payload: "{\"ok\":true}".to_string(),
            })
            .unwrap();

        assert_eq!(
            rx.recv().unwrap(),
            NmpEvent {
                payload: "{\"ok\":true}".to_string()
            }
        );
    }
}
