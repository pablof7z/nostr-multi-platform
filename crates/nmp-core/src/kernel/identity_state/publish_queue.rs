//! Publish-queue helpers — extracted to keep `identity_state` within the 500-LOC ceiling.

use super::RelayAckOutcome;

pub(in crate::kernel) fn publish_entry_can_retry(
    status: &str,
    outcomes: &[RelayAckOutcome],
    has_retry_payload: bool,
) -> bool {
    if !has_retry_payload {
        return false;
    }
    status == "failed"
        || status == "pending_relays_unknown"
        || outcomes.iter().any(|relay| relay.status == "failed")
}
