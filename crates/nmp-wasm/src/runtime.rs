use std::fmt;

use crate::protocol::{
    ActionDispatch, CapabilityFailure, ChirpAction, RelayBootstrapEntry, RuntimeStatus,
    StartConfig, WorkerEvent, WorkerRequest,
};

const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Default)]
pub struct WasmRuntime {
    started: bool,
    relay_bootstrap: Vec<RelayBootstrapEntry>,
    notes: Vec<LocalNote>,
    next_note: u64,
}

impl WasmRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, request: WorkerRequest) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        match request {
            WorkerRequest::Hello(hello) => {
                if hello.protocol_version != PROTOCOL_VERSION {
                    return Ok(vec![WorkerEvent::Error {
                        code: "protocol_mismatch".to_string(),
                        message: format!(
                            "expected protocol {PROTOCOL_VERSION}, got {}",
                            hello.protocol_version
                        ),
                        correlation_id: None,
                    }]);
                }
                Ok(vec![WorkerEvent::HelloAccepted {
                    protocol_version: PROTOCOL_VERSION,
                    status: RuntimeStatus::Ready,
                }])
            }
            WorkerRequest::Start(config) => self.start(config),
            WorkerRequest::ChirpAction(action) => {
                self.chirp_action(action.action, action.correlation_id)
            }
            WorkerRequest::Dispatch(action) => self.dispatch(action),
            WorkerRequest::CapabilityResult(result) => {
                Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: result.capability,
                    correlation_id: result.correlation_id,
                    reason: "capability completions require a running actor".to_string(),
                })])
            }
            WorkerRequest::Stop { correlation_id } => {
                self.started = false;
                Ok(vec![WorkerEvent::RuntimeStatus {
                    status: RuntimeStatus::Stopped,
                    correlation_id: Some(correlation_id),
                }])
            }
        }
    }

    fn start(&mut self, config: StartConfig) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        if config.app_id.trim().is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "app_id is required".to_string(),
            ));
        }
        if config.database_name.trim().is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "database_name is required".to_string(),
            ));
        }
        if config.relays.is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "at least one relay is required".to_string(),
            ));
        }
        self.relay_bootstrap = relay_bootstrap_from_config(config.relays, config.relay_bootstrap);
        self.started = true;
        Ok(vec![
            WorkerEvent::RuntimeStatus {
                status: RuntimeStatus::Running,
                correlation_id: Some(config.correlation_id),
            },
            WorkerEvent::Update {
                envelope: self.snapshot_envelope(),
            },
        ])
    }

    fn chirp_action(
        &mut self,
        action: ChirpAction,
        correlation_id: String,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        match action {
            ChirpAction::PublishNote {
                content,
                reply_to_id,
            } => {
                let action_type = "nmp.publish".to_string();
                if self.add_note(content, reply_to_id).is_none() {
                    return Ok(vec![self.rejected_action(
                        action_type,
                        correlation_id,
                        "publish note content is empty",
                    )]);
                }
                Ok(vec![
                    WorkerEvent::ActionAccepted {
                        action_type,
                        correlation_id,
                    },
                    WorkerEvent::Update {
                        envelope: self.snapshot_envelope(),
                    },
                ])
            }
            other => Ok(vec![self.unsupported_action(
                other.into_dispatch_parts().0,
                correlation_id,
            )]),
        }
    }

    fn dispatch(&mut self, action: ActionDispatch) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        if action.action_type == "nmp.publish" {
            if let Some(content) = action
                .payload
                .get("PublishNote")
                .and_then(|value| value.get("content"))
                .and_then(|value| value.as_str())
            {
                let reply_to_id = action
                    .payload
                    .get("PublishNote")
                    .and_then(|value| value.get("reply_to_id"))
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                if self.add_note(content.to_string(), reply_to_id).is_none() {
                    return Ok(vec![self.rejected_action(
                        action.action_type,
                        action.correlation_id,
                        "publish note content is empty",
                    )]);
                }
                return Ok(vec![
                    WorkerEvent::ActionAccepted {
                        action_type: action.action_type,
                        correlation_id: action.correlation_id,
                    },
                    WorkerEvent::Update {
                        envelope: self.snapshot_envelope(),
                    },
                ]);
            }
        }
        Ok(vec![self.unsupported_action(
            action.action_type,
            action.correlation_id,
        )])
    }

    fn add_note(&mut self, content: String, reply_to_id: Option<String>) -> Option<()> {
        let content = content.trim().to_string();
        if content.is_empty() {
            return None;
        }
        self.next_note = self.next_note.saturating_add(1);
        self.notes.insert(
            0,
            LocalNote {
                id: format!("web-local-{}", self.next_note),
                content,
                reply_to_id,
                created_at: self.next_note,
            },
        );
        self.notes.truncate(100);
        Some(())
    }

    fn unsupported_action(&self, action_type: String, correlation_id: String) -> WorkerEvent {
        self.rejected_action(
            action_type,
            correlation_id,
            "the browser wasm facade accepts publish-note intents; live relay-backed actions require the full actor driver",
        )
    }

    fn rejected_action(
        &self,
        action_type: String,
        correlation_id: String,
        reason: &str,
    ) -> WorkerEvent {
        WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: reason.to_string(),
        })
    }

    fn snapshot_envelope(&self) -> serde_json::Value {
        let cards: Vec<serde_json::Value> = self
            .notes
            .iter()
            .map(|note| {
                serde_json::json!({
                    "id": note.id,
                    "author_pubkey": "browser-local",
                    "author_display": {
                        "source": "fallback",
                        "name": "Browser demo",
                        "picture_url": null,
                    },
                    "kind": 1,
                    "created_at": note.created_at,
                    "content": note.content,
                    "content_tree": { "nodes": [] },
                    "relation_counts": {
                        "replies": { "status": "loading" },
                        "reactions": { "status": "loading" },
                        "reposts": { "status": "loading" },
                    },
                    "reply_to_id": note.reply_to_id,
                })
            })
            .collect();
        serde_json::json!({
            "t": "snapshot",
            "v": {
                "schema_version": 1,
                "update_kind": "ViewBatch",
                "running": self.started,
                "projections": {
                    "relay_diagnostics": self.relay_bootstrap.iter().map(|relay| {
                        serde_json::json!({
                            "url": relay.url,
                            "role": relay.role,
                            "status": "configured",
                        })
                    }).collect::<Vec<_>>()
                }
            },
            "chirpTimeline": {
                "blocks": [],
                "cards": cards,
            }
        })
    }
}

#[derive(Clone, Debug)]
struct LocalNote {
    id: String,
    content: String,
    reply_to_id: Option<String>,
    created_at: u64,
}

fn relay_bootstrap_from_config(
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

#[derive(Debug, PartialEq, Eq)]
pub enum WasmRuntimeError {
    InvalidConfig(String),
}

impl fmt::Display for WasmRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid config: {message}"),
        }
    }
}

impl std::error::Error for WasmRuntimeError {}
