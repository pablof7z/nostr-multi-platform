use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Hello(ClientHello),
    Start(StartConfig),
    Dispatch(ActionDispatch),
    #[serde(rename = "chirp_action")]
    AppAction(AppActionDispatch),
    CapabilityResult(CapabilityResult),
    /// V-01 Stage 3b — install a signer for app-level write actions.
    ///
    /// The browser host runs the asynchronous half of the handshake itself
    /// (e.g. `await window.nostr.getPublicKey()` for NIP-07) and supplies the
    /// already-known pubkey hex in this request. The wasm runtime then
    /// constructs the matching [`nmp_signers::Signer`] synchronously and
    /// stores it in its signer slot. Subsequent app-level writes that need
    /// signing (PublishNote, React, Follow, Unfollow) call into the slot's
    /// `sign()` method, which on wasm32 dispatches the actual signing call
    /// (`window.nostr.signEvent(...)`) through `wasm-bindgen-futures`.
    ///
    /// `kind`: `"nip07"` — the only kind wired in Stage 3b. Other kinds
    /// return [`WorkerEvent::CapabilityFailure`] with `unsupported_signer_kind`.
    SetSigner(SetSigner),
    Stop {
        correlation_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub app_id: String,
    pub platform: String,
    pub protocol_version: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartConfig {
    pub app_id: String,
    #[serde(default = "nmp_chirp_config::chirp_default_relay_urls")]
    pub relays: Vec<String>,
    #[serde(default = "default_relay_bootstrap")]
    pub relay_bootstrap: Vec<RelayBootstrapEntry>,
    pub database_name: String,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayBootstrapEntry {
    pub url: String,
    pub role: String,
}

impl From<&nmp_chirp_config::ChirpRelayBootstrapEntry> for RelayBootstrapEntry {
    fn from(entry: &nmp_chirp_config::ChirpRelayBootstrapEntry) -> Self {
        Self {
            url: entry.url.to_string(),
            role: entry.role.to_string(),
        }
    }
}

fn default_relay_bootstrap() -> Vec<RelayBootstrapEntry> {
    nmp_chirp_config::chirp_default_relay_bootstrap()
        .iter()
        .map(Into::into)
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDispatch {
    pub action_type: String,
    pub payload: Value,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppActionDispatch {
    pub action: AppAction,
    pub correlation_id: String,
}

impl AppActionDispatch {
    #[must_use]
    pub fn into_action_dispatch(self) -> ActionDispatch {
        let (action_type, payload) = self.action.into_dispatch_parts();
        ActionDispatch {
            action_type,
            payload,
            correlation_id: self.correlation_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AppAction {
    PublishNote {
        content: String,
        #[serde(default)]
        reply_to_id: Option<String>,
    },
    React {
        target_event_id: String,
        #[serde(default = "default_reaction")]
        reaction: String,
    },
    Follow {
        pubkey: String,
    },
    Unfollow {
        pubkey: String,
    },
}

impl AppAction {
    #[must_use]
    pub fn into_dispatch_parts(self) -> (String, Value) {
        match self {
            Self::PublishNote {
                content,
                reply_to_id,
            } => (
                "nmp.publish".to_string(),
                serde_json::json!({
                    "PublishNote": {
                        "content": content,
                        "reply_to_id": reply_to_id,
                        "target": "Auto",
                    }
                }),
            ),
            Self::React {
                target_event_id,
                reaction,
            } => (
                "nmp.nip25.react".to_string(),
                serde_json::json!({
                    "target_event_id": target_event_id,
                    "reaction": reaction,
                }),
            ),
            Self::Follow { pubkey } => (
                "nmp.follow".to_string(),
                serde_json::json!({ "pubkey": pubkey }),
            ),
            Self::Unfollow { pubkey } => (
                "nmp.unfollow".to_string(),
                serde_json::json!({ "pubkey": pubkey }),
            ),
        }
    }
}

fn default_reaction() -> String {
    "+".to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub capability: String,
    pub correlation_id: String,
    pub payload: Value,
}

/// Payload for [`WorkerRequest::SetSigner`].
///
/// `kind` is the discriminator the runtime uses to select a [`nmp_signers::Signer`]
/// constructor. Stage 3b ships `"nip07"` only; other kinds are honestly rejected
/// rather than silently dropped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetSigner {
    /// Backend kind. Currently must be `"nip07"`.
    pub kind: String,
    /// Hex-encoded public key the host already obtained from the backend.
    ///
    /// For NIP-07 this is the result of `await window.nostr.getPublicKey()`.
    /// Supplied by the host so the wasm runtime's install path stays
    /// synchronous — the async getPublicKey() round-trip happens in JS, before
    /// the request is sent.
    pub pubkey_hex: String,
    /// Correlation id echoed back in [`WorkerEvent::ActionAccepted`] (or
    /// [`WorkerEvent::CapabilityFailure`] on failure) so the host can match
    /// the outcome to the request that triggered it.
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityFailure {
    pub capability: String,
    pub correlation_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Ready,
    Running,
    Degraded(DegradedMode),
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradedMode {
    BrowserActorDriverMissing,
    CapabilityRejected,
    ProtocolMismatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    HelloAccepted {
        protocol_version: u16,
        status: RuntimeStatus,
    },
    RuntimeStatus {
        status: RuntimeStatus,
        correlation_id: Option<String>,
    },
    ActionAccepted {
        action_type: String,
        correlation_id: String,
    },
    UpdateBytes {
        bytes: Vec<u8>,
    },
    CapabilityFailure(CapabilityFailure),
    Error {
        code: String,
        message: String,
        correlation_id: Option<String>,
    },
}
