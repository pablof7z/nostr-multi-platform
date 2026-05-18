//! REPL error type. One enum with a `Display` impl — never crosses a panic
//! boundary, always rendered as a single-line user-facing message.

use std::fmt;

#[derive(Debug)]
pub enum ReplError {
    /// Parse failure already rendered as user message.
    Parse(String),
    /// User asked for a variable that requires state we don't have yet.
    Variable(String),
    /// Network / wire failure with a single-line summary.
    Network(String),
    /// NIP-05 resolution failure.
    Nip05(String),
    /// Plan compilation failure.
    Planner(String),
    /// I/O / readline / generic.
    Io(String),
    /// Catch-all.
    Other(String),
}

impl fmt::Display for ReplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplError::Parse(s) => write!(f, "{s}"),
            ReplError::Variable(s) => write!(f, "{s}"),
            ReplError::Network(s) => write!(f, "network error: {s}"),
            ReplError::Nip05(s) => write!(f, "nip-05 error: {s}"),
            ReplError::Planner(s) => write!(f, "planner error: {s}"),
            ReplError::Io(s) => write!(f, "io error: {s}"),
            ReplError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ReplError {}

impl From<std::io::Error> for ReplError {
    fn from(e: std::io::Error) -> Self {
        ReplError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ReplError>;
