//! ADR-0064 / Cut-B caller slice (#1756) — the typed **byte** dispatch seam for
//! the NmpGallery composition root's Android JNI shell.
//!
//! The JSON doorway `nmp_app_dispatch_action(app, namespace, json)` is retired
//! from this crate. Every write the gallery's `nativeDispatchAction` JNI entry
//! point emits now travels the typed
//! [`nmp_ffi::nmp_app_dispatch_action_bytes`] doorway: a host-minted
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
//! The gallery is a generic showcase: its `Cargo.toml` names only `nmp-ffi`,
//! `nmp-defaults`, `nmp-core`, `nmp-content`, and `serde_json` — never a
//! per-NIP crate. The typed payload types are therefore reached through the
//! [`nmp_defaults::action_payloads`] re-export surface, which `nmp-defaults`
//! owns precisely because it is the crate whose
//! [`register_defaults`](nmp_defaults::register_defaults) installs the matching
//! action modules. The namespaces this seam covers are exactly the ones the
//! default bundle wires; a namespace the gallery cannot dispatch (e.g. NIP-29
//! groups, which `register_defaults` never installs) is rejected fail-closed
//! (D6) rather than falling back to a JSON dispatch — there is no JSON dispatch
//! left.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde_json::Value;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_defaults::action_payloads;
use nmp_ffi::{nmp_app_dispatch_action_bytes, nmp_free_string, NmpApp};

/// Process-local correlation-id source.
///
/// The byte lane echoes a HOST-supplied `correlation_id` verbatim (ADR-0064 §4)
/// — unlike the retired JSON lane, where the kernel minted it. The gallery crate
/// carries no `uuid`/`rand` dependency, and a write correlation id only has to
/// be unique within one running process for the lifetime of an in-flight
/// operation (the host spinner keys on it until the terminal `action_stages`
/// verdict, then ACKs). A monotone atomic counter satisfies that exactly and
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
///
/// `namespace` is the action's HOST namespace (e.g. `nmp.follow`), which the
/// open envelope routes on; it MAY differ from the payload's
/// [`ActionPayload::SCHEMA_ID`] (e.g. both `nmp.follow` and `nmp.unfollow` carry
/// the `nmp.nip02.follow_action` payload). Returns a fail-closed error string
/// (D6) for an unknown namespace or a body that does not deserialize into the
/// namespace's typed action.
///
/// Coverage is exactly the action set [`nmp_defaults::register_defaults`]
/// installs (the only modules a gallery `NmpApp` has registered).
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
        "nmp.nip57.zap" => encode::<action_payloads::ZapInput>(namespace, json),
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
/// Builds the typed payload for `namespace` from `json` (the canonical action
/// body the gallery shell produced), mints a host correlation id, wraps payload
/// + namespace + id in an open [`DispatchEnvelope`](nmp_core::dispatch_envelope),
/// and hands the finished bytes to [`nmp_app_dispatch_action_bytes`]. Returns
/// the result envelope JSON on accept/reject (`{"correlation_id":…}` /
/// `{"error":…}`), or a fail-closed error string (D6) on a null app, an unknown
/// / mis-shaped namespace, or a kernel rejection.
///
/// # Safety
/// `app` must be a valid non-null `*mut NmpApp` from `nmp_app_new` (a null `app`
/// returns an error string, never a crash).
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

    // SAFETY: `app` is a valid, non-null pointer (checked above); `envelope` is a
    // live, fully-initialised byte buffer for the duration of the call. The
    // doorway reads the bytes but never retains or frees them.
    let ptr = nmp_app_dispatch_action_bytes(app, envelope.as_ptr(), envelope.len());
    if ptr.is_null() {
        return Err("action dispatch returned null".to_string());
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(ptr);

    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("action dispatch returned invalid JSON: {e}"))?;
    parse_dispatch_envelope(&value).map(|_| text)
}

/// Parse a dispatch result envelope returned by the byte doorway.
///
/// The doorway returns `{"correlation_id":"<id>"}` on accept (the host-supplied
/// id echoed verbatim) or `{"error":"<message>"}` on rejection.
pub fn parse_dispatch_envelope(value: &Value) -> Result<String, String> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    value
        .get("correlation_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "action dispatch envelope missing correlation_id".to_string())
}

#[cfg(test)]
#[path = "dispatch_bytes_tests.rs"]
mod tests;
