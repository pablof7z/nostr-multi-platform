//! Shared helpers for wasm worker request dispatch.

use crate::runtime::DispatchBytesResult;

use super::protocol::{IdentityRelayPermission, WorkerEvent};
use super::ref_routing::signer_not_installed_reason;

/// Maximum number of identity-provided relays merged into the configured list.
const MAX_IDENTITY_RELAYS: usize = 30;

pub(super) fn dispatch_result_events(result: DispatchBytesResult) -> Vec<WorkerEvent> {
    match result {
        DispatchBytesResult::Applied {
            action_type,
            correlation_id,
        } => vec![WorkerEvent::ActionAccepted {
            action_type,
            correlation_id,
        }],
        DispatchBytesResult::SignRequired {
            action_correlation_id,
            correlation_id,
            account_pubkey,
            unsigned_json,
        } => vec![WorkerEvent::SignRequest {
            correlation_id,
            action_correlation_id: Some(action_correlation_id),
            account_pubkey,
            unsigned_json,
        }],
        DispatchBytesResult::Rejected {
            capability,
            correlation_id,
            reason,
        } => vec![WorkerEvent::CapabilityFailure {
            capability,
            correlation_id,
            reason,
        }],
        DispatchBytesResult::NoActiveAccount {
            capability,
            correlation_id,
        } => vec![WorkerEvent::CapabilityFailure {
            capability,
            correlation_id,
            reason: signer_not_installed_reason(),
        }],
        DispatchBytesResult::DecodeError { message } => vec![WorkerEvent::Error {
            code: "dispatch_envelope_rejected".to_string(),
            message,
            correlation_id: None,
        }],
    }
}

pub(super) fn not_started_error(correlation_id: Option<String>) -> Vec<WorkerEvent> {
    vec![WorkerEvent::Error {
        code: "not_started".to_string(),
        message: "runtime not started — send WorkerRequest::Start first".to_string(),
        correlation_id,
    }]
}

/// Map identity relay permissions to canonical `(url, role)` pairs for merging
/// into the kernel's configured relay list (#2139 HIGH 4).
///
/// Mirrors `nmp-wasm/src/runtime/signer.rs`'s `identity_relay_entry` and
/// `role_for_identity_relay`. Skips entries with no read/write permissions and
/// skips non-canonical URLs.
pub(super) fn identity_relays_to_rows(relays: &[IdentityRelayPermission]) -> Vec<(String, String)> {
    relays
        .iter()
        .take(MAX_IDENTITY_RELAYS)
        .filter_map(|r| {
            let role = match (r.read, r.write) {
                (true, true) | (false, true) => "both,indexer",
                (true, false) => "read,indexer",
                (false, false) => return None,
            };
            let url = nmp_core::canonical_relay_url(&r.url)?.to_string();
            Some((url, role.to_string()))
        })
        .collect()
}
