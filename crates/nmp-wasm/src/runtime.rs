use std::fmt;

use crate::protocol::{
    CapabilityFailure, DegradedMode, RuntimeStatus, StartConfig, WorkerEvent, WorkerRequest,
};

const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Default)]
pub struct WasmRuntime {
    started: bool,
}

impl WasmRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, request: WorkerRequest) -> Result<WorkerEvent, WasmRuntimeError> {
        match request {
            WorkerRequest::Hello(hello) => {
                if hello.protocol_version != PROTOCOL_VERSION {
                    return Ok(WorkerEvent::Error {
                        code: "protocol_mismatch".to_string(),
                        message: format!(
                            "expected protocol {PROTOCOL_VERSION}, got {}",
                            hello.protocol_version
                        ),
                        correlation_id: None,
                    });
                }
                Ok(WorkerEvent::HelloAccepted {
                    protocol_version: PROTOCOL_VERSION,
                    status: RuntimeStatus::Ready,
                })
            }
            WorkerRequest::Start(config) => self.start(config),
            WorkerRequest::ChirpAction(action) => {
                self.handle(WorkerRequest::Dispatch(action.into_action_dispatch()))
            }
            WorkerRequest::Dispatch(action) => {
                Ok(WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: action.action_type,
                    correlation_id: action.correlation_id,
                    reason: "browser actor driver is not linked yet".to_string(),
                }))
            }
            WorkerRequest::CapabilityResult(result) => {
                Ok(WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: result.capability,
                    correlation_id: result.correlation_id,
                    reason: "capability completions require a running actor".to_string(),
                }))
            }
            WorkerRequest::Stop { correlation_id } => {
                self.started = false;
                Ok(WorkerEvent::RuntimeStatus {
                    status: RuntimeStatus::Stopped,
                    correlation_id: Some(correlation_id),
                })
            }
        }
    }

    fn start(&mut self, config: StartConfig) -> Result<WorkerEvent, WasmRuntimeError> {
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
        self.started = true;
        Ok(WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Degraded(DegradedMode::BrowserActorDriverMissing),
            correlation_id: Some(config.correlation_id),
        })
    }
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
