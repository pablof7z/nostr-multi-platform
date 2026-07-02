//! FFI-free dispatch core for the byte-action doorway.
//!
//! This is the single typed dispatch function the UniFFI surface
//! (`nmp-uniffi` / `nmp-uniffi-support`) calls into; there is no separate
//! C-ABI crate anymore (`nmp-ffi` was deleted — `nmp-uniffi` is the sole
//! native binding surface).
//!
//! The only externally-visible item is [`dispatch_action_bytes_typed`] which
//! returns a [`DispatchOutcome`] record. `nmp-uniffi-support` converts that
//! record into its own UniFFI-exported `DispatchOutcome` type
//! (`correlation_id` / `error` fields) without changing behaviour.
//!
//! # Preserved invariants (carry-forward from the retired nmp-ffi::action::bytes)
//!
//! * D6 fail-closed: every error surfaces as a populated `DispatchOutcome`,
//!   never a panic or an uninhabited struct.
//! * One terminal per dispatch: `finish_dispatch_typed` honours the
//!   `failure.enqueued` flag exactly as the JSON counterpart did (PR #1676).
//! * ADR-0064 §4 identity contract: `correlation_id` in `DispatchOutcome` is
//!   always the HOST-SUPPLIED envelope id, never a kernel-minted replacement.

use nmp_core::__ffi_internal::ActionExecuteFailure;
use nmp_core::actor::{ActionLedgerCommand, ActorCommand};
use nmp_core::dispatch_envelope::{decode_dispatch_envelope, MAX_DISPATCH_ENVELOPE_BYTES};
use nmp_core::substrate::{ActionContext, ActionRejection, ActionResult};

use crate::NmpApp;

// ── Public typed outcome ─────────────────────────────────────────────────────

/// Typed outcome of a byte-doorway action dispatch.
///
/// Exactly one of `correlation_id` (accepted) or `error` (rejected/failed)
/// will be `Some`. `code` is `Some` only for
/// `ActionRejection::InvalidCoded` rejections — it carries the stable
/// machine-readable token alongside the human-readable `error` text.
///
/// This is the source-of-truth return type; the UniFFI surface
/// (`nmp-uniffi-support`) maps it into its own UniFFI-exported record type
/// with the same fields. See `coded_rejection_tests.rs:122` for the
/// load-bearing test that guards the `code` field.
pub struct DispatchOutcome {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
    /// Machine-readable code for `ActionRejection::InvalidCoded` (issue #1734).
    /// Present iff this is a coded rejection; `None` for plain errors or
    /// accepted dispatches.
    pub code: Option<String>,
}

impl DispatchOutcome {
    fn error(msg: String) -> Self {
        DispatchOutcome {
            correlation_id: None,
            error: Some(msg),
            code: None,
        }
    }

    fn coded_rejection(code: &'static str, message: String) -> Self {
        DispatchOutcome {
            correlation_id: None,
            error: Some(message),
            code: Some(code.to_string()),
        }
    }

    fn accepted(correlation_id: String) -> Self {
        DispatchOutcome {
            correlation_id: Some(correlation_id),
            error: None,
            code: None,
        }
    }

    /// Post-`start()` execute failure: the action was minted (a
    /// `correlation_id` exists) but then execution failed. Both fields are
    /// populated so the host can ACK the stage and show a toast.
    fn post_mint_failure(correlation_id: String, message: String) -> Self {
        DispatchOutcome {
            correlation_id: Some(correlation_id),
            error: Some(message),
            code: None,
        }
    }
}

// ── Public dispatch entry point ──────────────────────────────────────────────

/// FFI-free core of the byte-doorway dispatch.
///
/// Decodes the open
/// [`nmp_core::dispatch_envelope::DispatchEnvelope`] (`bytes`), routes the
/// opaque per-crate payload by `action_namespace` into the registry's typed
/// `start_bytes` / `execute_bytes`, and returns a [`DispatchOutcome`].
///
/// D6 fail-closed: a null-equivalent `bytes` slice, an oversize / malformed /
/// wrong-identifier / wrong-version / namespace-less / correlation-id-less
/// envelope, an unknown namespace, or a not-typed-capable module all surface
/// as a populated `DispatchOutcome::error(…)`. The S2 decoder and the S3
/// registry both `catch_unwind` internally; no new unwind path is added here.
pub fn dispatch_action_bytes_typed(app: &NmpApp, bytes: &[u8]) -> DispatchOutcome {
    // Redundant but cheap: the decoder gates the same ceiling, but checking
    // it here avoids constructing a &[u8] view of a potentially hostile size.
    if bytes.len() > MAX_DISPATCH_ENVELOPE_BYTES {
        use nmp_core::dispatch_envelope::DispatchDecodeError;
        return DispatchOutcome::error(
            DispatchDecodeError::Oversize {
                len: bytes.len(),
                max: MAX_DISPATCH_ENVELOPE_BYTES,
            }
            .to_string(),
        );
    }

    // S2 — decode the open envelope and run its fail-closed gates.
    let decoded = match decode_dispatch_envelope(bytes) {
        Ok(decoded) => decoded,
        Err(err) => return DispatchOutcome::error(err.to_string()),
    };

    // Non-authoritative validation timestamp (same semantics as the retired
    // nmp-ffi implementation this was carried forward from).
    let dispatch_now_ms = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };

    let mut ctx = ActionContext::with_event_store_slot(app.event_store_handle());
    // ADR-0064 §4: the operation identity is the HOST-SUPPLIED envelope id.
    let correlation_id = decoded.correlation_id;

    // S3 — route by namespace into the typed registry doorway.
    match app.action_registry().start_bytes(
        &mut ctx,
        dispatch_now_ms,
        &decoded.action_namespace,
        &decoded.payload,
    ) {
        Ok(_validated) => {
            let outcome = app.action_registry().execute_bytes(
                &ctx,
                &decoded.action_namespace,
                &decoded.payload,
                &correlation_id,
                &|cmd| app.send_cmd(cmd),
            );
            finish_dispatch_typed(app, &correlation_id, outcome)
        }
        Err(rejection) => rejection_to_outcome(rejection),
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Convert an [`ActionRejection`] to a [`DispatchOutcome`], preserving the
/// `code` field for `InvalidCoded` rejections (load-bearing: issue #1734,
/// `coded_rejection_tests.rs:122`).
fn rejection_to_outcome(rejection: ActionRejection) -> DispatchOutcome {
    match rejection {
        ActionRejection::InvalidCoded { code, message } => {
            DispatchOutcome::coded_rejection(code, message)
        }
        ActionRejection::Invalid(s) => DispatchOutcome::error(s),
        ActionRejection::Unauthorized(s) => DispatchOutcome::error(format!("unauthorized: {s}")),
        ActionRejection::Conflict(s) => DispatchOutcome::error(format!("conflict: {s}")),
    }
}

/// Typed equivalent of the retired `nmp-ffi::action::finish_dispatch`.
///
/// Post-`start()` outcome handling. Preserves the one-terminal-per-dispatch
/// invariant (#1676 BUG-A/B/C): when `failure.enqueued` is set the executor
/// already enqueued an `ActorCommand` that owns the terminal, so we report as
/// accepted rather than fanning a second `RecordFailure`.
fn finish_dispatch_typed(
    app: &NmpApp,
    correlation_id: &str,
    outcome: Result<(), ActionExecuteFailure>,
) -> DispatchOutcome {
    match outcome {
        Ok(()) => {
            app.action_registry().deliver_result(ActionResult {
                correlation_id: correlation_id.to_string(),
                result_json: serde_json::Value::Null,
            });
            DispatchOutcome::accepted(correlation_id.to_string())
        }
        Err(failure) if failure.enqueued => {
            // The executor already enqueued a command that carries the terminal
            // verdict — do NOT fan a second RecordFailure. Report as accepted.
            app.action_registry().deliver_result(ActionResult {
                correlation_id: correlation_id.to_string(),
                result_json: serde_json::Value::Null,
            });
            DispatchOutcome::accepted(correlation_id.to_string())
        }
        Err(failure) => {
            // Nothing was enqueued; this fan-in is the sole terminal. Record a
            // Failed stage so the host spinner keyed on the id resolves.
            app.send_cmd(ActorCommand::ActionLedger(
                ActionLedgerCommand::RecordFailure {
                    correlation_id: correlation_id.to_string(),
                    reason: failure.message.clone(),
                },
            ));
            DispatchOutcome::post_mint_failure(correlation_id.to_string(), failure.message)
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "action_dispatch_tests.rs"]
mod tests;
