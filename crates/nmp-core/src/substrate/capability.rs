//! Capability module trait and wire types for the capability-callback socket.
//!
//! `CapabilityModule` is the typed contract a platform capability (e.g. the
//! OS keyring) implements. Kernel modules use `CapabilityRequest` /
//! `CapabilityEnvelope` as the JSON-over-FFI carrier; `capability_socket.rs`
//! and `ffi/capability.rs` handle the routing. See `docs/product-spec/doctrine.md`
//! D7 — capability bridges are report-only; all policy lives in the issuing module.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Request: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result: Clone + Serialize + DeserializeOwned + Send + 'static;

    fn callback_interface_name() -> &'static str;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityRequest {
    pub namespace: String,
    pub correlation_id: String,
    pub payload_json: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityEnvelope {
    pub namespace: String,
    pub correlation_id: String,
    pub result_json: String,
}
