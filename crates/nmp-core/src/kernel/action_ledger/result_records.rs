//! `ActionResultRecord` — the per-tick terminal-verdict shape that makes the
//! `ActionLedger` the SINGLE source of `action_results` (S11 slice 2, #1758).
//!
//! Every terminal recorded via [`crate::kernel::action_ledger::ActionLedger::record_terminal`]
//! appends one [`ActionResultRecord`] onto the ledger's `terminal_results` drain
//! buffer. `action_results` is serialised by draining that buffer via
//! [`crate::kernel::action_ledger::ActionLedger::take_terminal_results`].
//!
//! These types are stored ALONGSIDE the stage history (not re-derived from it)
//! because the row's `error` is the terminal's own verbatim string and
//! `result_json` / `event_id` never enter the substrate stage type.

/// One drained `action_results` row — the per-tick terminal-verdict record the
/// ledger is now the SINGLE source of.
///
/// Appended by [`super::ActionLedger::record_terminal`] on every terminal write and
/// drained once per emit by [`super::ActionLedger::take_terminal_results`]. Carries the
/// already-mapped WIRE `status` (`"published"` / `"failed"` / `"cancelled"` —
/// the `ok → published` mapping is resolved at record time, not at serialise
/// time) plus the fields the host reads off the row: the verbatim `error`, the
/// opaque `result_json` (ADR-0043 Decision 4, forwarded into `result`), and the
/// signed `event_id` (#1702). These are stored ALONGSIDE the stage history
/// rather than re-derived from it because the row's `error` is the terminal's
/// own verbatim string (a `failed` terminal can carry an `error` distinct from
/// the stage's prose `reason`), and `result_json` / `event_id` never enter the
/// substrate stage type.
#[derive(Clone, Debug)]
pub(crate) struct ActionResultRecord {
    /// Correlation id of the dispatched action this terminal reports on.
    pub(super) correlation_id: String,
    /// Already-mapped wire status: `"published"`, `"failed"`, or `"cancelled"`.
    pub(super) status: &'static str,
    /// Verbatim failure string (`None` on success / cancel).
    pub(super) error: Option<String>,
    /// Opaque structured result body, forwarded verbatim into the row's
    /// `result` field. `None` unless the action attached one.
    pub(super) result_json: Option<String>,
    /// The signed event's id, when one backs this terminal (#1702). `None` for
    /// off-band terminals where no event was ever signed.
    pub(super) event_id: Option<String>,
}

impl ActionResultRecord {
    /// Serialise this record into the exact `action_results` row JSON the prior
    /// `pending_terminals`-sourced drain produced: `{correlation_id, status,
    /// error}` always (the `error` key present, `null` when `None`), plus an
    /// optional `result` (forwarded `result_json`, parsed to a JSON object when
    /// it parses, else carried as a raw string) and an optional `event_id`.
    pub(super) fn to_row(&self) -> serde_json::Value {
        let mut row = serde_json::json!({
            "correlation_id": self.correlation_id,
            "status": self.status,
            "error": self.error,
        });
        // ADR-0043 Decision 4 — forward the opaque structured result body
        // verbatim under `result`. Re-parse so the host reads a JSON object (not
        // a JSON-encoded string); a non-JSON body forwards as a raw string. This
        // is forwarding, NOT interpretation (D0).
        if let Some(result_json) = &self.result_json {
            let value = serde_json::from_str::<serde_json::Value>(result_json)
                .unwrap_or_else(|_| serde_json::Value::String(result_json.clone()));
            if let Some(obj) = row.as_object_mut() {
                obj.insert("result".to_string(), value);
            }
        }
        // #1702 — surface the published event's id under `event_id`, omitted
        // entirely when absent (mirrors `result`).
        if let Some(event_id) = &self.event_id {
            if let Some(obj) = row.as_object_mut() {
                obj.insert(
                    "event_id".to_string(),
                    serde_json::Value::String(event_id.clone()),
                );
            }
        }
        row
    }
}
