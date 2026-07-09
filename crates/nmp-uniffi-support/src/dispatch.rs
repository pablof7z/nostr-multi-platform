//! Action-dispatch mechanics (split out of `lib.rs` for file-size discipline).

use nmp_native_runtime::{dispatch_action_bytes_typed, NmpApp};

/// Typed outcome of a `dispatch_action` call.
///
/// Exactly one of `correlation_id` (accepted) or `error` (rejected/failed)
/// will be `Some`. `code` is `Some` only for coded rejections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
    pub code: Option<String>,
}

impl From<nmp_native_runtime::DispatchOutcome> for DispatchOutcome {
    fn from(out: nmp_native_runtime::DispatchOutcome) -> Self {
        DispatchOutcome {
            correlation_id: out.correlation_id,
            error: out.error,
            code: out.code,
        }
    }
}

impl DispatchOutcome {
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        DispatchOutcome {
            correlation_id: None,
            error: Some(message.into()),
            code: None,
        }
    }
}

/// Dispatch an NMPD FlatBuffers action envelope through the native runtime.
#[must_use]
pub fn dispatch_action(app: &NmpApp, envelope: &[u8]) -> DispatchOutcome {
    dispatch_action_bytes_typed(app, envelope).into()
}

/// Owned-`Vec` convenience for UniFFI facade methods.
#[must_use]
pub fn dispatch_action_vec(app: &NmpApp, envelope: Vec<u8>) -> DispatchOutcome {
    dispatch_action(app, &envelope)
}
