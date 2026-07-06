// Negative fixture: clean browser-runtime transport adapter (no violations).
// This file should pass the browser_runtime_boundary linter.

use std::sync::Arc;

/// Worker dispatch interface: accepts action and returns response bytes.
pub struct WorkerDispatch {
    action_id: u32,
    payload: Vec<u8>,
}

impl WorkerDispatch {
    pub fn new(action_id: u32, payload: Vec<u8>) -> Self {
        Self { action_id, payload }
    }

    /// Pure adapter: dispatches the action to the kernel.
    pub fn dispatch(&self) -> Vec<u8> {
        // Dispatch logic here; no routing/outbox/policy vocabulary.
        vec![]
    }
}

/// Relay callback: invoked when a relay event arrives.
pub fn on_relay_event(event_bytes: Vec<u8>) {
    // Pure adapter: no routing/subscription planning.
}

/// Snapshot callback: invoked when the app snapshot is ready.
pub fn on_snapshot_ready(snapshot_bytes: Vec<u8>) {
    // Pure adapter: just bridge the bytes to the worker boundary.
}
