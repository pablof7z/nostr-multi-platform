//! Private utilities shared by the NWC runtime — extracted to keep `runtime.rs`
//! within the 500-LOC ceiling.

/// Serialize a JSON value to a string for the outbound wire queue.
///
/// V-63: replaces the prior `serde_json::to_string(...).unwrap_or_default()`
/// call sites. Returns `Err` on the rare serialization failure so callers can
/// surface an error rather than pushing an empty `""` frame.
pub(super) fn encode_frame(value: &serde_json::Value) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}
