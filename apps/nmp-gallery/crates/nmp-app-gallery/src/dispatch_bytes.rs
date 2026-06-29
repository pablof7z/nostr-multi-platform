//! ADR-0064 / Cut-B caller slice (#1756) — the typed **byte** dispatch seam for
//! the NmpGallery composition root's Android JNI shell.
//!
//! The JSON doorway `nmp_app_dispatch_action(app, namespace, json)` is retired
//! from this crate. Every write the gallery's `nativeDispatchAction` JNI entry
//! point emits now travels the typed
//! [`nmp_native_runtime::dispatch_action_bytes_typed`] doorway: a host-minted
//! `correlation_id` + the action's host NAMESPACE + a typed
//! [`ActionPayload`](nmp_core::substrate::ActionPayload) payload, wrapped in an
//! open [`DispatchEnvelope`](nmp_core::dispatch_envelope) via
//! [`encode_dispatch_envelope`].
//!
//! ## Why a namespace-keyed encoder (and not a JSON pass-through)
//!
//! The gallery shell still hands the seam an `(action, payload-json)` pair —
//! the Kotlin UI builds the canonical serde body for a write and the JNI entry
//! point receives both halves. This seam deserializes that JSON into the
//! matching typed `ActionPayload`, keyed by the host namespace, and calls
//! [`ActionPayload::encode`] to produce the typed payload bytes. The JSON is an
//! in-process intermediate that NEVER crosses the FFI; only typed bytes do.
//!
//! ## D0 — no per-NIP dependency
//!
//! The gallery is a generic showcase: its `Cargo.toml` names only
//! `nmp-native-runtime`, `nmp-uniffi`, `nmp-defaults`, `nmp-core`, `nmp-content`,
//! and `serde_json` — never a per-NIP crate. The typed payload types are therefore
//! reached through the [`nmp_defaults::action_payloads`] re-export surface, which
//! mirrors the gallery's explicit composition.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_defaults::action_payloads;
use nmp_native_runtime::{dispatch_action_bytes_typed, NmpApp};

/// Process-local correlation-id source.
///
/// The byte lane echoes a HOST-supplied `correlation_id` verbatim (ADR-0064 §4).
/// A monotone atomic counter satisfies uniqueness-within-process-lifetime and
/// keeps the dependency surface unchanged. The `gallery-` prefix namespaces it
/// so it never collides with the kernel's hex correlation ids.
static NEXT_CORRELATION_ID: AtomicU64 = AtomicU64::new(1);

/// Mint a fresh process-local correlation id for a byte-doorway dispatch.
#[must_use]
pub fn mint_correlation_id() -> String {
    let n = NEXT_CORRELATION_ID.fetch_add(1, Ordering::Relaxed);
    format!("gallery-{n}")
}

/// Encode `json` (the canonical serde body the gallery shell produced) into the
/// typed [`ActionPayload`] FlatBuffers bytes for `namespace`.
fn encode_payload_for_namespace(namespace: &str, json: &str) -> Result<Vec<u8>, String> {
    match namespace {
        "nmp.publish" => encode::<action_payloads::PublishAction>(namespace, json),
        "nmp.nip22.post_comment" => {
            encode::<action_payloads::PostCommentAction>(namespace, json)
        }
        "nmp.nip25.react" => encode::<action_payloads::ReactAction>(namespace, json),
        "nmp.nip25.unreact" => encode::<action_payloads::UnreactAction>(namespace, json),
        "nmp.follow" | "nmp.unfollow" => encode::<action_payloads::PubkeyAction>(namespace, json),
        "nmp.follow_many" => encode::<action_payloads::FollowManyAction>(namespace, json),
        "nmp.nip17.send" => encode::<action_payloads::SendDmInput>(namespace, json),
        "nmp.nip17.publish_relay_list" => {
            encode::<action_payloads::PublishDmRelayListInput>(namespace, json)
        }
        "nmp.nip51.add_bookmark" | "nmp.nip51.remove_bookmark" => {
            encode::<action_payloads::BookmarkUpdateInput>(namespace, json)
        }
        "nmp.nip65.publish_relay_list" => {
            encode::<action_payloads::PublishRelayListInput>(namespace, json)
        }
        "nmp.nip51.block_relay" => encode::<action_payloads::BlockRelayInput>(namespace, json),
        "nmp.nip51.unblock_relay" => encode::<action_payloads::UnblockRelayInput>(namespace, json),
        other => Err(format!(
            "no typed payload encoder for action namespace '{other}' (byte doorway has no JSON fallback)"
        )),
    }
}

/// Deserialize `json` into `P` and encode it to typed [`ActionPayload`] bytes.
fn encode<P>(namespace: &str, json: &str) -> Result<Vec<u8>, String>
where
    P: ActionPayload + DeserializeOwned,
{
    let action: P = serde_json::from_str(json).map_err(|e| {
        format!("action body for '{namespace}' does not match its typed payload shape: {e}")
    })?;
    Ok(action.encode())
}

/// Dispatch a gallery action through the typed byte doorway.
///
/// Builds the typed payload for `namespace` from `json`, mints a host correlation id,
/// wraps payload + namespace + id in an open [`DispatchEnvelope`](nmp_core::dispatch_envelope),
/// and hands the finished bytes to [`dispatch_action_bytes_typed`]. Returns the
/// correlation id string on accept, or a fail-closed error string (D6) on a null app,
/// an unknown / mis-shaped namespace, or a kernel rejection.
///
/// # Safety
/// `app` must be a valid non-null `*mut NmpApp` (a null `app` returns an error
/// string, never a crash).
pub fn dispatch_action_bytes_for(
    app: *mut NmpApp,
    namespace: &str,
    json: &str,
) -> Result<String, String> {
    if app.is_null() {
        return Err("runtime app is not available".to_string());
    }
    let payload = encode_payload_for_namespace(namespace, json)?;
    let correlation_id = mint_correlation_id();
    let envelope = encode_dispatch_envelope(
        &correlation_id,
        namespace,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );

    // SAFETY: `app` is a valid, non-null pointer (checked above).
    let app_ref = unsafe { &*app };
    let outcome = dispatch_action_bytes_typed(app_ref, &envelope);
    if let Some(err) = outcome.error {
        return Err(err);
    }
    // Return the echoed correlation_id (host-supplied, per ADR-0064 §4).
    outcome
        .correlation_id
        .ok_or_else(|| "action dispatch returned no correlation_id".to_string())
}

/// Parse a dispatch result envelope returned by the byte doorway.
///
/// The doorway returns `{"correlation_id":"<id>"}` on accept or `{"error":"<message>"}` on rejection.
/// Kept as a test-visible helper; the main dispatch path uses [`DispatchOutcome`] directly.
pub fn parse_dispatch_envelope(value: &serde_json::Value) -> Result<String, String> {
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(error.to_string());
    }
    value
        .get("correlation_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "action dispatch envelope missing correlation_id".to_string())
}

#[cfg(test)]
#[path = "dispatch_bytes_tests.rs"]
mod tests;
