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
    SearchOpen(SearchOpen),
    SearchClose(SearchClose),
    GroupDiscoveryOpen(GroupDiscoveryOpen),
    GroupDiscoveryClose(GroupDiscoveryClose),
    GroupEventsOpen(GroupEventsOpen),
    GroupEventsClose(GroupEventsClose),
    NotificationsOpen(NotificationsOpen),
    NotificationsClose(NotificationsClose),
    NotificationsMarkRead(NotificationsMarkRead),
    RelayConfig(RelayConfig),
    PublishRelayPreferences(PublishRelayPreferences),
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

#[derive(Deserialize)]
pub(crate) struct SetIdentity {
    pub kind: String,
    #[serde(default)]
    pub pubkey_hex: String,
    #[serde(default)]
    pub secret_key_bech32: Option<String>,
    #[serde(default)]
    pub bunker_uri: Option<String>,
    pub correlation_id: String,
    #[serde(default)]
    pub identity_relays: Vec<IdentityRelayPermission>,
}

impl std::fmt::Debug for SetIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetIdentity")
            .field("kind", &self.kind)
            .field("pubkey_hex", &self.pubkey_hex)
            .field(
                "secret_key_bech32",
                &self.secret_key_bech32.as_ref().map(|_| "[redacted]"),
            )
            .field(
                "bunker_uri",
                &self.bunker_uri.as_ref().map(|_| "[redacted]"),
            )
            .field("correlation_id", &self.correlation_id)
            .field("identity_relays", &self.identity_relays)
            .finish()
    }
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
pub(crate) struct SearchOpen {
    pub session_id: String,
    pub query: String,
    pub scope: SearchScope,
    pub targets: SearchTargets,
    #[serde(default)]
    pub relays: Vec<String>,
    #[serde(default)]
    pub max_hits: Option<usize>,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchScope {
    Notes,
    Profiles,
    Longform,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchTargets {
    UserPreferred,
    AppDefault,
    Explicit,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchClose {
    pub session_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GroupDiscoveryOpen {
    pub session_id: String,
    pub relay_url: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GroupDiscoveryClose {
    pub session_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GroupEventsOpen {
    pub session_id: String,
    pub relay_url: String,
    pub group_id: String,
    /// Consumer-declared kind selection (issue #2187). Empty = all h-tagged
    /// group events; a chat view sends `[9, 11]`. NIP-29 owns only the
    /// `["h", local_id]` routing; the consumer picks the kinds.
    pub kinds: Vec<u32>,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GroupEventsClose {
    pub session_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotificationsOpen {
    pub session_id: String,
    pub account_pubkey: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotificationsClose {
    pub session_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotificationsMarkRead {
    pub session_id: String,
    #[serde(default)]
    pub event_ids: Vec<String>,
    #[serde(default)]
    pub all_visible: bool,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RelayConfig {
    pub action: RelayConfigAction,
    pub url: String,
    #[serde(default)]
    pub role: Option<String>,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PublishRelayPreferences {
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RelayConfigAction {
    Add,
    Remove,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        action_correlation_id: Option<String>,
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
/// "both"`.
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
