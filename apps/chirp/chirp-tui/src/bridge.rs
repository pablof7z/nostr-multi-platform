use std::sync::mpsc::{self, Receiver, Sender};

use nmp_ffi::NmpApp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NmpEvent {
    pub payload: UpdatePayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePayload {
    FlatBuffers(Vec<u8>),
    JsonFixture(String),
}

impl UpdatePayload {
    #[must_use]
    pub fn flatbuffers(bytes: Vec<u8>) -> Self {
        Self::FlatBuffers(bytes)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::FlatBuffers(bytes) => bytes.len(),
            Self::JsonFixture(json) => json.len(),
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn json_fixture(payload: impl Into<String>) -> Self {
        Self::JsonFixture(payload.into())
    }
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
        nmp_ffi::nmp_app_set_update_callback(app, context, Some(on_update));
    }
}

pub fn unregister(app: *mut NmpApp) {
    nmp_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
}

extern "C" fn on_update(context: *mut std::ffi::c_void, payload: *const u8, len: usize) {
    if context.is_null() || payload.is_null() {
        return;
    }

    let bridge = unsafe { &*(context as *const NmpUpdateBridge) };
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) }.to_vec();
    let _ = bridge.tx.send(NmpEvent {
        payload: UpdatePayload::flatbuffers(bytes),
    });
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
                payload: UpdatePayload::json_fixture("{\"ok\":true}"),
            })
            .unwrap();

        assert_eq!(
            rx.recv().unwrap(),
            NmpEvent {
                payload: UpdatePayload::json_fixture("{\"ok\":true}")
            }
        );
    }
}
