use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum WasmRuntimeError {
    InvalidConfig(String),
    /// The pure `KernelReducer` returned an unexpected `KernelUpdate` variant
    /// for a `KernelAction` whose contract is single-valued (e.g. `Start`
    /// always yields `Started`).
    KernelContract(String),
}

impl fmt::Display for WasmRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid config: {message}"),
            Self::KernelContract(message) => write!(formatter, "kernel contract: {message}"),
        }
    }
}

impl std::error::Error for WasmRuntimeError {}
