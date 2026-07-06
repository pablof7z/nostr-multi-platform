// Negative fixture: clean ABI module (no violations of WASM_ABI_ONLY rule).
// This file should pass the wasm_abi_only linter.

use std::sync::Arc;

// Only FlatBuffers param types and narrow ABI bridge types are imported.
pub struct WorkerMessage {
    pub action_id: u32,
    pub payload: Vec<u8>,
}

pub struct WorkerResponse {
    pub action_id: u32,
    pub result: Vec<u8>,
}

pub fn dispatch_action(msg: &WorkerMessage) -> WorkerResponse {
    WorkerResponse {
        action_id: msg.action_id,
        result: vec![],
    }
}

/// ABI bridge: call the action dispatcher with a work message.
pub fn handle_worker_message(msg: WorkerMessage) -> WorkerResponse {
    dispatch_action(&msg)
}

/// FFI callback: invoked when events arrive from the relay.
pub fn on_event_received(event_bytes: Vec<u8>) {
    // Do nothing; relay events are handled by the kernel,
    // not the Worker transport adapter.
}
