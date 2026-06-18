//! Negative no_raw_tap fixture — must produce zero findings.
//!
//! Demonstrates compliant code that uses the canonical replacement API
//! instead of the deleted raw event tap escape hatch.

use std::sync::Arc;

/// Canonical replacement: implement ExternalEventSinkPolicy instead of
/// registering a RawEventObserver.
pub struct MyEventSink;

impl MyEventSink {
    pub fn dispatch_frame(&self, json: Arc<str>) {
        // Forward via ExternalEventSinkPolicy + ExternalEventSinkDispatcher,
        // not via the old raw tap mechanism.
        let _ = json;
    }
}
