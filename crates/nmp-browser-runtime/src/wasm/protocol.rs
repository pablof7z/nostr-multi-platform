// Protocol types are fully used on wasm32; on native they are used only
// by `core.rs` (which is `#[cfg(test)]`-primary on that target).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! Wire protocol types for the `NmpWasmRuntime` entry point (#2038 item A).
//!
//! Mirrors `nmp-wasm/src/protocol.rs` but defined here so `nmp-browser-runtime`
//! (the ADR-0067 composition root) stays free of a dep on the `nmp-wasm` ABI
//! crate. Both sets of types share the same JSON wire format consumed by
//! `web/packages/runtime-web/src/protocol.ts`.
//!
//! Always-compiled (no `cfg(wasm32)` gate here): the Serde derives work on
//! native so `NmpRuntimeCore` can be unit-tested without a wasm target.

use serde::{Deserialize, Serialize};

pub(crate) const PROTOCOL_VERSION: u16 = 1;

// ── Inbound request types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WorkerRequest {
    Hello(ClientHello),
    Start(StartConfig),
    ResolveRef(ResolveRef),
    ReleaseRef(ReleaseRef),
    DispatchBytes(DispatchBytesPayload),
    CapabilityResult(CapabilityResultPayload),
    SetIdentity(SetIdentity),
    BeginSign(BeginSign),
    DeliverSignerResponse(DeliverSignerResponse),
    Stop { correlation_id: String },
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClientHello {
    pub app_id: String,
    pub platform: String,
    pub protocol_version: u16,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StartConfig {
    pub app_id: String,
    pub relays: Vec<String>,
    pub relay_bootstrap: Vec<RelayBootstrapEntry>,
    pub database_name: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RelayBootstrapEntry {
    pub url: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetIdentity {
    pub kind: String,
    pub pubkey_hex: String,
    pub correlation_id: String,
    #[serde(default)]
    pub identity_relays: Vec<IdentityRelayPermission>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IdentityRelayPermission {
    pub url: String,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResolveRef {
    pub namespace: u32,
    pub key: String,
    pub consumer_id: String,
    pub shape: u32,
    pub liveness: u32,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub event_author: Option<String>,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReleaseRef {
    pub namespace: u32,
    pub key: String,
    pub consumer_id: String,
    pub correlation_id: String,
}

/// Payload for the binary `dispatch_bytes` request.
///
/// On the JSON path (via `handle_json`) `bytes` is a JSON number array —
/// avoid this path for large payloads. On the binary path (`handle_dispatch_bytes`)
/// the raw `&[u8]` is passed directly, bypassing JSON.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct DispatchBytesPayload {
    #[serde(default)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CapabilityResultPayload {
    pub capability: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BeginSign {
    pub account_pubkey: String,
    pub unsigned_json: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeliverSignerResponse {
    pub correlation_id: String,
    #[serde(default)]
    pub signed_json: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

// ── Outbound event types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WorkerEvent {
    HelloAccepted {
        protocol_version: u16,
        status: RuntimeStatus,
    },
    RuntimeStatus {
        status: RuntimeStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
    ActionAccepted {
        action_type: String,
        correlation_id: String,
    },
    CapabilityFailure {
        capability: String,
        correlation_id: String,
        reason: String,
    },
    SignRequest {
        correlation_id: String,
        account_pubkey: String,
        unsigned_json: String,
    },
    SignCompleted {
        correlation_id: String,
        signed_json: String,
    },
    SignFailed {
        correlation_id: String,
        reason: String,
    },
    Error {
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeStatus {
    Ready,
    Running,
    Stopped,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Serialize a `Vec<WorkerEvent>` to a JSON array string.
///
/// D6 — total: on serialization error returns a minimal JSON error array.
pub(crate) fn serialize_events(events: &[WorkerEvent]) -> String {
    serde_json::to_string(events).unwrap_or_else(|_| {
        r#"[{"type":"error","code":"serialize_failed","message":"worker event serialize error"}]"#
            .to_string()
    })
}

/// Serialize a single `WorkerEvent` wrapped in a JSON array.
///
/// Convenience for single-event responses.
pub(crate) fn serialize_one(event: WorkerEvent) -> String {
    serialize_events(&[event])
}

/// Normalize the relay bootstrap list from `StartConfig`.
///
/// If `relay_bootstrap` is non-empty it is used verbatim (explicit role
/// assignment wins). Otherwise synthesize one entry per URL with `role =
/// "both"` (matches `nmp-wasm::protocol::relay_bootstrap_from_config`).
pub(crate) fn relay_bootstrap_from_config(
    relays: Vec<String>,
    relay_bootstrap: Vec<RelayBootstrapEntry>,
) -> Vec<RelayBootstrapEntry> {
    if !relay_bootstrap.is_empty() {
        return relay_bootstrap;
    }
    relays
        .into_iter()
        .map(|url| RelayBootstrapEntry {
            url,
            role: "both".to_string(),
        })
        .collect()
}
