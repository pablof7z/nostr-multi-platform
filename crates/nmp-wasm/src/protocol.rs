use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Hello(ClientHello),
    Start(StartConfig),
    Dispatch(ActionDispatch),
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
    #[serde(default = "nmp_chirp_config::chirp_default_relay_urls")]
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
    Update { envelope: Value },
    CapabilityFailure(CapabilityFailure),
    Error {
        code: String,
        message: String,
        correlation_id: Option<String>,
    },
}
