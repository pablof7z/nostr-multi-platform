use std::fmt;

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
