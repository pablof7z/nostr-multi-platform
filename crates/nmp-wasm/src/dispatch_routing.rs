//! Pure helpers around the runtime's dispatch routing surface.
//!
//! Two responsibilities:
//!
//! 1. [`kernel_action_from_dispatch`] — map a generic
//!    [`crate::protocol::ActionDispatch`] to a [`nmp_core::KernelAction`] if
//!    (and only if) the `action_type` is in the kernel namespace. Returns
//!    `None` for app-namespaced actions, which the runtime surfaces through
//!    the write-path-unavailable error path.
//!
//! 2. Stable, host-pattern-matchable reason strings for the two
//!    write-unavailability states the wasm runtime can honestly report
//!    (`signer_not_installed`, `publish_path_not_wired`) plus the
//!    capability-completion failure reason (`browser_actor_driver_missing`).
//!
//! Split out of `runtime.rs` so the file stays under the 500-LOC ceiling and
//! the routing table has a single owner that codegen / kernel-namespace
//! additions touch directly.

use std::sync::Arc;

use nmp_core::KernelAction;
use nmp_signers::Signer;
use serde_json::Value;

use crate::protocol::ActionDispatch;

/// Single-source reason string for app-level writes that cannot complete on
/// the **synchronous** wasm runtime path. Distinguishes the two honest
/// failure modes the synchronous `handle()` arm can surface:
///
/// - **No signer installed.** The host hasn't called `SetSigner` yet — the
///   user has not signed in. Banner: "sign in to publish".
/// - **Signer installed but synchronous-path-only.** A signer IS installed
///   and the wasm runtime CAN publish — through the asynchronous
///   `NmpWasmRuntime::dispatch_app_action_async(...)` entrypoint Stage 3c
///   added. The synchronous `handle_json` cannot route the same action
///   because `Nip07Signer::sign()` needs to `await` a JS Promise (`window.
///   nostr.signEvent(...)`) the wasm thread cannot block on. The reason
///   string points the host at the async entrypoint so the integration is
///   self-documenting.
///
/// Both strings start with a stable underscore-snake-case prefix the JS host
/// can pattern-match without parsing the full reason text.
pub(crate) fn write_path_unavailable_reason(signer: Option<&Arc<dyn Signer>>) -> String {
    if signer.is_none() {
        return "signer_not_installed: no signer installed; send WorkerRequest::SetSigner \
                with kind = \"nip07\" and the pubkey from window.nostr.getPublicKey() \
                before dispatching app-level writes."
            .to_string();
    }
    "publish_path_not_wired: a signer is installed but app-level writes \
     cannot be routed through the synchronous `handle_json` path — the \
     NIP-07 sign step requires awaiting `window.nostr.signEvent(...)`, \
     which the wasm thread cannot block on. Use \
     `NmpWasmRuntime.dispatch_app_action_async(requestJson)` (returns a \
     Promise) instead. V-01 Stage 3c wired PublishNote (kind:1); React / \
     Follow / Unfollow follow up."
        .to_string()
}

/// Reason string for non-app-action capability completions that cannot be
/// honored without the native actor. Kept stable so JS hosts can
/// pattern-match the `browser_actor_driver_missing` prefix.
pub(crate) fn browser_driver_missing_reason() -> String {
    "browser_actor_driver_missing: capability completions require the native \
     actor (gated behind feature = \"native\"). The wasm runtime accepts the \
     completion to drain the JS-side pending state but cannot route it into \
     a capability handler."
        .to_string()
}

/// Map a generic `ActionDispatch` to its `KernelAction` if (and only if) the
/// `action_type` is in the kernel namespace. Returns `None` for app-namespaced
/// actions, which the caller surfaces via [`write_path_unavailable_reason`]
/// until Stage 3c wires a publish path.
///
/// Kept narrow on purpose: only the actions whose entire implementation lives
/// in the pure reducer are routed. Anything that needs the actor (signed-event
/// publication, capability dispatch, planner driver) returns `None`.
pub(crate) fn kernel_action_from_dispatch(action: &ActionDispatch) -> Option<KernelAction> {
    match action.action_type.as_str() {
        "nmp.kernel.start" => Some(KernelAction::Start),
        "nmp.kernel.stop" => Some(KernelAction::Stop),
        "nmp.kernel.diagnostics" => Some(KernelAction::RunDiagnostics),
        "nmp.kernel.open_uri" => action
            .payload
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| KernelAction::OpenUri { uri: uri.to_string() }),
        "nmp.kernel.open_view" => {
            let namespace = action.payload.get("namespace").and_then(Value::as_str)?;
            let key = action.payload.get("key").and_then(Value::as_str)?;
            Some(KernelAction::OpenView {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
        }
        "nmp.kernel.close_view" => {
            let namespace = action.payload.get("namespace").and_then(Value::as_str)?;
            let key = action.payload.get("key").and_then(Value::as_str)?;
            Some(KernelAction::CloseView {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_path_unavailable_reason_distinguishes_signer_states() {
        assert!(write_path_unavailable_reason(None).starts_with("signer_not_installed"));
        // Build a real Arc<dyn Signer> using the NIP-07 stub so we exercise
        // the `Some` arm honestly. The signer's sign() will return
        // Unsupported on native; we never call sign() here.
        use nmp_signers::Nip07Signer;
        let signer: Arc<dyn Signer> = Arc::new(Nip07Signer::from_cached_pubkey(
            nostr::PublicKey::from_hex(
                "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            )
            .unwrap(),
        ));
        assert!(write_path_unavailable_reason(Some(&signer)).starts_with("publish_path_not_wired"));
    }

    #[test]
    fn kernel_action_routes_kernel_namespace_only() {
        let dispatch = ActionDispatch {
            action_type: "nmp.kernel.start".to_string(),
            payload: serde_json::Value::Null,
            correlation_id: "x".to_string(),
        };
        assert!(matches!(
            kernel_action_from_dispatch(&dispatch),
            Some(KernelAction::Start)
        ));

        let app = ActionDispatch {
            action_type: "nmp.publish".to_string(),
            payload: serde_json::Value::Null,
            correlation_id: "y".to_string(),
        };
        assert!(kernel_action_from_dispatch(&app).is_none());
    }
}
