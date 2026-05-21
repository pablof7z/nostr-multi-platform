use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Hello(ClientHello),
    Start(StartConfig),
    Dispatch(ActionDispatch),
    ChirpAction(ChirpActionDispatch),
    CapabilityResult(CapabilityResult),
    Stop { correlation_id: String },
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
    pub relays: Vec<String>,
    pub database_name: String,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDispatch {
    pub action_type: String,
    pub payload: Value,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChirpActionDispatch {
    pub action: ChirpAction,
    pub correlation_id: String,
}

impl ChirpActionDispatch {
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
pub enum ChirpAction {
    /// Web host intent for the Rust-owned kind:1 publish path.
    /// The host supplies content and optional parent id; Rust owns target
    /// routing, stamping, signing, and relay selection via `nmp.publish`.
    PublishNote {
        content: String,
        #[serde(default)]
        reply_to_id: Option<String>,
    },
    /// Kind:7 reaction intent. Missing reaction defaults to `"+"` here so
    /// browser UI does not duplicate Chirp's like-button policy.
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

impl ChirpAction {
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
                "chirp.react".to_string(),
                serde_json::json!({
                    "target_event_id": target_event_id,
                    "reaction": reaction,
                }),
            ),
            Self::Follow { pubkey } => (
                "chirp.follow".to_string(),
                serde_json::json!({ "pubkey": pubkey }),
            ),
            Self::Unfollow { pubkey } => (
                "chirp.unfollow".to_string(),
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
    Update {
        envelope: Value,
    },
    CapabilityFailure(CapabilityFailure),
    Error {
        code: String,
        message: String,
        correlation_id: Option<String>,
    },
}
