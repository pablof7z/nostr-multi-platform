//! Shared publish-failure recording helpers.
//!
//! Split out of `commands::publish` (file-size ownership): every publish
//! handler guards on an active account and on a valid target, and on the unhappy
//! path must perform the same **dual write** — set the last-error toast AND, when
//! a dispatched action is waiting on a `correlation_id`, record the matching
//! `Failed` terminal so the host spinner clears (the #1735 broken-promise fix).
//! Centralising the three one-liner exits here keeps that contract uniform and
//! removes the risk of a new handler honouring only one leg.

use crate::kernel::Kernel;
use crate::relay::OutboundMessage;

fn set_publish_error(kernel: &mut Kernel, code: &'static str, fallback: String) {
    kernel.set_last_error_token(
        &crate::ui_token::UiToken::error(code, fallback.clone()).with_detail(fallback),
    );
}

/// Set a "no active account" toast and — when a dispatched action is waiting
/// on a `correlation_id` — record the matching `Failed` terminal so the host
/// spinner clears.
///
/// Every publish handler guards on `identity.active_pubkey()` and exits early
/// when no account is signed in. Threading the `correlation_id` through that
/// exit is the broken-promise fix the per-handler arms already honour ad-hoc;
/// centralising it here keeps the pattern uniform and removes the risk of a new
/// handler forgetting the second leg.
///
/// The lifecycle `reason` is the bare `"no active account"` prose; #1735 also
/// sets the curated `LIFECYCLE_NO_ACTIVE_ACCOUNT` code the host localizes.
pub(super) fn toast_no_account(
    kernel: &mut Kernel,
    action: &str,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    let toast = format!("cannot {action}: no active account — sign in first");
    kernel.set_last_error_token(
        &crate::ui_token::UiToken::error(crate::ui_token::codes::PUBLISH_NO_ACTIVE_ACCOUNT, toast)
            .with_subject(action),
    );
    if let Some(id) = correlation_id {
        let code = crate::ui_token::codes::LIFECYCLE_NO_ACTIVE_ACCOUNT;
        kernel.record_action_failure_coded(id, "no active account".into(), Some(code), None);
    }
    Vec::new()
}

/// Set `reason` as the last-error toast and — when a dispatched action is
/// waiting on a `correlation_id` — record the matching `Failed` terminal so
/// the host spinner clears. Returns an empty outbound vec so call sites stay
/// `return fail_publish(...);` one-liners.
///
/// This is the generic twin of [`fail_invalid_target`] — same dual-write
/// contract, but the toast text is supplied verbatim by the caller rather
/// than templated with the `"explicit publish target rejected:"` prefix.
/// Used by sign-setup and sign-error branches across every publish handler;
/// previously these were ~3-line `set_last_error_toast` + `if let Some(id)`
/// copy-pastes (with one branch in `publish_unsigned_event_to_relays`
/// silently DROPPING the `correlation_id`, which orphaned the host spinner on
/// a dispatched NIP-29 group-message sign failure — fixed by this consolidation).
pub(super) fn fail_publish(
    kernel: &mut Kernel,
    reason: String,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    set_publish_error(
        kernel,
        crate::ui_token::codes::PUBLISH_SIGN_FAILED,
        reason.clone(),
    );
    if let Some(id) = correlation_id {
        // Prose-only (#1735): caller-supplied diagnostic text, not curated copy.
        kernel.record_action_failure(id, reason);
    }
    Vec::new()
}

pub(super) fn fail_ownership(
    kernel: &mut Kernel,
    reason: String,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    set_publish_error(
        kernel,
        crate::ui_token::codes::PUBLISH_OWNERSHIP_REJECTED,
        reason.clone(),
    );
    if let Some(id) = correlation_id {
        kernel.record_action_failure_coded(
            id,
            reason,
            Some(crate::ui_token::codes::PUBLISH_OWNERSHIP_REJECTED),
            None,
        );
    }
    Vec::new()
}

pub(super) fn fail_invalid_target(
    kernel: &mut Kernel,
    reason: String,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    let toast = format!("explicit publish target rejected: {reason}");
    set_publish_error(
        kernel,
        crate::ui_token::codes::PUBLISH_INVALID_TARGET,
        toast.clone(),
    );
    if let Some(id) = correlation_id {
        // Prose-only (#1735): wraps caller-supplied upstream diagnostic text.
        kernel.record_action_failure(id, toast);
    }
    Vec::new()
}
