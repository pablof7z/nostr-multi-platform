//! Display-label / semantic-tone derivation for the `signer_state` projection
//! (ADR-0032 / #1099).
//!
//! Display decisions live in Rust (aim.md §6 / AP1, thin-shell rule): the
//! producer (`SignerStateDto::new`) pre-computes `status_label` and
//! `status_tone` once, here, so neither shell (iOS `AccountsView`, Android
//! `SignInScreen`) reconstructs them by switching on the raw `state` token.
//! This mirrors the wallet precedent (`nmp_nip47::status::status_label` /
//! `status_tone`, #623 / PR #1243).

/// Derive the human-readable label and semantic tone for a (canonicalised)
/// signer-state wire token.
///
/// Tone vocabulary mirrors `nmp_nip47::status::tone`: `"active"` (healthy),
/// `"warning"` (transient — reconnecting / awaiting approval), `"error"`
/// (terminal — unavailable / failed), `"inactive"` (no session / unknown).
///
/// `"connected"` is accepted as an alias of `"ready"` for robustness even
/// though [`super::SignerStateDto::from_nip46_connection_state`] canonicalises
/// it before construction.
///
/// `pub(crate)` so the typed-FlatBuffers decoder
/// (`crate::actor::typed_projections::signer_state_fb`) reuses this exact logic
/// as its forward-compat fallback for buffers that predate the tail-appended
/// `status_label` / `status_tone` fields — no mirror, one source of truth (D1).
pub(crate) fn signer_state_label_and_tone(state: &str) -> (String, String) {
    match state {
        "ready" | "connected" => ("Connected".to_string(), "active".to_string()),
        "reconnecting" => ("Reconnecting…".to_string(), "warning".to_string()),
        "awaiting_approval" => ("Waiting for approval…".to_string(), "warning".to_string()),
        "unavailable" => ("Signer unavailable".to_string(), "error".to_string()),
        "failed" => ("Connection failed".to_string(), "error".to_string()),
        _ => ("Unknown".to_string(), "inactive".to_string()),
    }
}
