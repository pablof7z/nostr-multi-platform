// Trait implementations for `PublishEngineError`. Split out of `engine.rs`
// to keep that file within the 500-LOC hard cap (AGENTS.md).
use super::PublishEngineError;
use super::super::traits::PublishStoreError;

impl std::fmt::Display for PublishEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateHandle(h) => write!(f, "duplicate publish handle: {:?}", h),
            Self::NoTargets => write!(f, "no relay targets for publish"),
            Self::Store(e) => write!(f, "publish store error: {e}"),
            Self::UnsupportedAction(name) => write!(f, "unsupported action: {name}"),
        }
    }
}

impl std::error::Error for PublishEngineError {}

impl From<PublishStoreError> for PublishEngineError {
    fn from(err: PublishStoreError) -> Self {
        Self::Store(err)
    }
}
