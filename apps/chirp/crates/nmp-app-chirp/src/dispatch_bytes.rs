//! ADR-0064 / Cut-B caller slice (#1756) — the typed **byte** dispatch seam for
//! Chirp's in-repo Rust shells (`nmp-app-chirp`, `chirp-tui`, `chirp-desktop`).
//!
//! The JSON doorway `nmp_app_dispatch_action(app, namespace, json)` is retired
//! from these crates. Every write the Chirp Rust path emits now travels the
//! typed [`nmp_ffi::nmp_app_dispatch_action_bytes`] doorway: a host-minted
//! `correlation_id` + the module's host NAMESPACE + a per-crate typed
//! [`ActionPayload`](nmp_core::substrate::ActionPayload) payload, wrapped in an
//! open [`DispatchEnvelope`](nmp_core::dispatch_envelope) via
//! [`encode_dispatch_envelope`].
//!
//! ## Why a namespace-keyed encoder (and not a JSON pass-through)
//!
//! Chirp's Rust action builders (`crate::action_specs`, the runtime `json!`
//! bodies) are the single source of truth for each action's canonical serde
//! shape — they encode protocol detail (NIP-10 reply tags, the NIP-65
//! `role`→`RelayMarker` collapse, serde defaults for the optional NIP-29 group
//! fields) that must not be re-derived at every call site. Those builders are
//! ALSO consumed by the Rust-native `crate::typed_api::ChirpClient`
//! (chirp-tui / chirp-desktop), so their `(namespace, json)` contract must stay
//! intact.
//!
//! So this seam keeps the builders untouched and converts at the doorway: it
//! deserializes the builder's canonical JSON into the matching per-crate
//! `ActionPayload` type — keyed by the host namespace — and calls
//! [`ActionPayload::encode`] to produce the typed payload bytes. The JSON is an
//! in-process intermediate that NEVER crosses the FFI; only typed bytes do. A
//! namespace with no typed encoder is rejected fail-closed (D6) rather than
//! silently falling back to a JSON dispatch — there is no JSON dispatch left.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde_json::Value;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_ffi::{nmp_app_dispatch_action_bytes, nmp_free_string, NmpApp};

/// Process-local correlation-id source.
///
/// The byte lane echoes a HOST-supplied `correlation_id` verbatim (ADR-0064 §4)
/// — unlike the retired JSON lane, where the kernel minted it. The Chirp Rust
/// crates carry no `uuid`/`rand` dependency, and a write correlation id only has
/// to be unique within one running process for the lifetime of an in-flight
/// operation (the host spinner keys on it until the terminal `action_stages`
/// verdict, then ACKs). A monotone atomic counter satisfies that exactly and
/// keeps the dependency surface unchanged. The `chirp-` prefix namespaces it so
/// it never collides with the kernel's hex correlation ids.
static NEXT_CORRELATION_ID: AtomicU64 = AtomicU64::new(1);

/// Mint a fresh process-local correlation id for a byte-doorway dispatch.
#[must_use]
pub fn mint_correlation_id() -> String {
    let n = NEXT_CORRELATION_ID.fetch_add(1, Ordering::Relaxed);
    format!("chirp-{n}")
}

/// Encode `json` (the canonical serde body produced by a Chirp action builder)
/// into the typed [`ActionPayload`] FlatBuffers bytes for `namespace`.
///
/// `namespace` is the module's HOST namespace (e.g. `nmp.follow`), which the
/// open envelope routes on; it MAY differ from the payload's
/// [`ActionPayload::SCHEMA_ID`] (e.g. both `nmp.follow` and `nmp.unfollow` carry
/// the `nmp.nip02.follow_action` payload). Returns a fail-closed error string
/// (D6) for an unknown namespace or a body that does not deserialize into the
/// namespace's typed action.
fn encode_payload_for_namespace(namespace: &str, json: &str) -> Result<Vec<u8>, String> {
    match namespace {
        "nmp.publish" => encode::<nmp_core::publish::PublishAction>(namespace, json),
        "nmp.nip25.react" => encode::<nmp_nip25::ReactAction>(namespace, json),
        "nmp.nip25.unreact" => encode::<nmp_nip25::UnreactAction>(namespace, json),
        "nmp.follow" | "nmp.unfollow" => encode::<nmp_nip02::PubkeyAction>(namespace, json),
        "nmp.nip17.send" => encode::<nmp_nip17::SendDmInput>(namespace, json),
        "nmp.nip17.publish_relay_list" => {
            encode::<nmp_nip17::PublishDmRelayListInput>(namespace, json)
        }
        "nmp.nip57.zap" => encode::<nmp_nip57::ZapInput>(namespace, json),
        "nmp.app.chirp.zap_identifier" => encode::<crate::ZapIdentifierInput>(namespace, json),
        "nmp.nip65.publish_relay_list" => {
            encode::<nmp_router::PublishRelayListInput>(namespace, json)
        }
        "nmp.nip51.block_relay" => encode::<nmp_router::BlockRelayInput>(namespace, json),
        "nmp.nip51.unblock_relay" => encode::<nmp_router::UnblockRelayInput>(namespace, json),
        "nmp.nip01.visible_note_relations" => {
            encode::<nmp_relations::VisibleNoteRelationsAction>(namespace, json)
        }
        "nmp.nip29.discover" => encode::<nmp_nip29::action::DiscoverGroupsInput>(namespace, json),
        "nmp.nip29.create_public_group" => {
            encode::<nmp_nip29::action::CreatePublicGroupInput>(namespace, json)
        }
        "nmp.nip29.join" => encode::<nmp_nip29::action::JoinGroupInput>(namespace, json),
        "nmp.nip29.leave" => encode::<nmp_nip29::action::LeaveGroupInput>(namespace, json),
        "nmp.nip29.publish_group_event" => {
            encode::<nmp_nip29::action::PublishGroupEventInput>(namespace, json)
        }
        "nmp.nip29.react_in_group" => {
            encode::<nmp_nip29::action::ReactInGroupInput>(namespace, json)
        }
        #[cfg(feature = "wallet")]
        "nmp.wallet.connect" => encode::<nmp_nip47::WalletConnectAction>(namespace, json),
        #[cfg(feature = "wallet")]
        "nmp.wallet.disconnect" => encode::<nmp_nip47::WalletDisconnectAction>(namespace, json),
        #[cfg(feature = "wallet")]
        "nmp.wallet.pay_invoice" => encode::<nmp_nip47::WalletAction>(namespace, json),
        #[cfg(feature = "marmot")]
        "nmp.marmot" => {
            encode::<nmp_marmot::projection::action::MarmotAction>(namespace, json)
        }
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

/// Dispatch a Chirp action through the typed byte doorway.
///
/// Builds the typed payload for `namespace` from `json` (the canonical action
/// body a Chirp builder produced), mints a host correlation id, wraps payload +
/// namespace + id in an open [`DispatchEnvelope`](nmp_core::dispatch_envelope),
/// and hands the finished bytes to [`nmp_app_dispatch_action_bytes`]. Returns
/// the echoed correlation id on accept, or a fail-closed error string (D6) on a
/// null app, an unknown / mis-shaped namespace, or a kernel rejection.
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
    parse_dispatch_envelope(&value)
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
