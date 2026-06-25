use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Hello(ClientHello),
    Start(StartConfig),
    /// ADR-0063 reference-resolution control message.
    ///
    /// This is deliberately separate from the app-write doorway. It carries the
    /// raw-key `resolve_ref` fields as typed JSON data so components can mount
    /// profile/event interests without reopening the retired `action_type +
    /// Value` command surface.
    ResolveRef(ResolveRef),
    /// ADR-0063 reference-release control message.
    ReleaseRef(ReleaseRef),
    /// ADR-0064 / S2 (#1750) — the **binary write doorway**. The host posts the
    /// raw bytes of a finished `DispatchEnvelope` (`correlation_id` + generated
    /// `action_namespace` + typed FlatBuffers `payload`). This is the SAME open
    /// transport the native FFI (`nmp_app_dispatch_action_bytes`) uses — there
    /// is no wasm-specific write vocabulary (the hand-rolled `AppAction` enum +
    /// `"app_action"` envelope were deleted in #1743 Cut A).
    DispatchBytes(DispatchBytes),
    CapabilityResult(CapabilityResult),
    /// Set the active identity for app-level writes.
    ///
    /// The browser host runs the asynchronous half of the handshake itself
    /// (e.g. `await window.nostr.getPublicKey()` for NIP-07) and supplies the
    /// already-known pubkey hex in this request. The wasm runtime validates +
    /// canonicalizes the pubkey and seeds the kernel's active account so
    /// active-follows resolution and bootstrap interests know whose data to
    /// load.
    ///
    /// **No persistent signer is installed** (ADR-0064 §5): signing is the
    /// ADR-0050 capability round-trip ([`Self::BeginSign`] →
    /// [`WorkerEvent::SignRequest`] → [`Self::DeliverSignerResponse`]), not an
    /// `Arc<dyn Signer>` awaited inside the publish flow. `kind`: `"nip07"` is
    /// the only kind wired; other kinds return [`WorkerEvent::CapabilityFailure`]
    /// with `unsupported_signer_kind`.
    SetIdentity(SetIdentity),
    /// #1753 S6 — begin a NIP-07 sign capability round-trip.
    ///
    /// The worker parks a sign op (ADR-0050 §D1) bound to `account_pubkey` and
    /// emits a [`WorkerEvent::SignRequest`] the **main-thread JS bridge** (the
    /// broker — Web Workers have no `window.nostr`) fulfils by calling
    /// `window.nostr.signEvent(unsigned)`. The bridge posts the signed bytes
    /// back as [`WorkerRequest::DeliverSignerResponse`]. Pure message re-entry:
    /// no polling, no tick-dependence (D8).
    BeginSign(BeginSign),
    /// #1753 S6 — the main-thread broker delivers a signer response (the
    /// `sign`-verb fulfiller feeding the ADR-0050 §D3b `DeliverSignerResponse`
    /// command). `signed_json` is the flat-NIP-01 event from
    /// `window.nostr.signEvent`; `error` is set instead when the user rejected
    /// or `window.nostr` was unavailable. Account-pinned: the worker rejects a
    /// signature authored by an account other than the one the round-trip was
    /// begun for.
    DeliverSignerResponse(DeliverSignerResponse),
    Stop {
        correlation_id: String,
    },
}

/// Payload for [`WorkerRequest::BeginSign`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginSign {
    /// Lowercase-hex account the sign is pinned to (the host already has it from
    /// the NIP-07 `getPublicKey()` handshake).
    pub account_pubkey: String,
    /// The unsigned flat-NIP-01 (or nested-`UnsignedEvent`) JSON to sign.
    pub unsigned_json: String,
}

/// Payload for [`WorkerRequest::DeliverSignerResponse`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliverSignerResponse {
    /// The `correlation_id` from the [`WorkerEvent::SignRequest`] this fulfils.
    pub correlation_id: String,
    /// The signed flat-NIP-01 event JSON from `window.nostr.signEvent`. Present
    /// on success; `None` when `error` is set.
    #[serde(default)]
    pub signed_json: Option<String>,
    /// A failure reason (user rejected, no `window.nostr`, etc.). Present on
    /// failure; `None` on success.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub app_id: String,
    pub platform: String,
    pub protocol_version: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartConfig {
    pub app_id: String,
    /// Relay set the host wants the runtime to connect to. Relay policy is a
    /// host concern, not framework policy — the framework has no built-in
    /// default. A host (e.g. the Chirp web composition root) MUST supply this
    /// explicitly; a missing field fails serde deserialization loudly rather
    /// than silently falling back to one app's relays.
    pub relays: Vec<String>,
    /// Explicit relay bootstrap list (url + role). Like `relays`, this is host
    /// policy with no framework default; the host MUST supply it.
    pub relay_bootstrap: Vec<RelayBootstrapEntry>,
    pub database_name: String,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayBootstrapEntry {
    pub url: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveRef {
    pub namespace: u32,
    pub key: String,
    pub consumer_id: String,
    pub shape: u32,
    pub liveness: u32,
    /// Optional relay hints decoded by the app boundary from a NIP-19/NIP-21
    /// event reference. Missing defaults to the bare-key path.
    #[serde(default)]
    pub hints: Vec<String>,
    /// Optional event author decoded by the app boundary from a nevent author
    /// TLV. Ignored for profile refs and address-coordinate event keys, where
    /// the author is already part of the raw key.
    #[serde(default)]
    pub event_author: Option<String>,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRef {
    pub namespace: u32,
    pub key: String,
    pub consumer_id: String,
    pub correlation_id: String,
}

/// Payload for [`WorkerRequest::DispatchBytes`] — the ADR-0064 binary write
/// doorway. `bytes` are the raw bytes of a finished `DispatchEnvelope`
/// FlatBuffers root (file identifier `NMPD`); the runtime decodes them through
/// `nmp_core::dispatch_envelope::decode_dispatch_envelope` (the SAME path the
/// native FFI `nmp_app_dispatch_action_bytes` uses). The host posts them as a
/// transferable `Uint8Array`; serde renders them as a JSON number array on the
/// degraded in-process path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchBytes {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub capability: String,
    pub correlation_id: String,
    pub payload: Value,
}

/// Payload for [`WorkerRequest::SetIdentity`].
///
/// `kind` is the backend discriminator the host obtained the pubkey from.
/// `"nip07"` is the only kind wired; other kinds are honestly rejected rather
/// than silently dropped. The runtime seeds the kernel's active account from
/// the validated pubkey — it does NOT install a persistent signer (ADR-0064 §5:
/// signing is the [`WorkerRequest::BeginSign`] capability round-trip).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetIdentity {
    /// Backend kind. Currently must be `"nip07"`.
    pub kind: String,
    /// Hex-encoded public key the host already obtained from the backend.
    ///
    /// For NIP-07 this is the result of `await window.nostr.getPublicKey()`.
    /// Supplied by the host so the wasm runtime stays synchronous — the async
    /// getPublicKey() round-trip happens in JS, before the request is sent.
    pub pubkey_hex: String,
    /// Correlation id echoed back in [`WorkerEvent::ActionAccepted`] (or
    /// [`WorkerEvent::CapabilityFailure`] on failure) so the host can match
    /// the outcome to the request that triggered it.
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityFailure {
    pub capability: String,
    pub correlation_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Ready,
    Running,
    Degraded(DegradedMode),
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradedMode {
    BrowserActorDriverMissing,
    CapabilityRejected,
    ProtocolMismatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    HelloAccepted {
        protocol_version: u16,
        status: RuntimeStatus,
    },
    RuntimeStatus {
        status: RuntimeStatus,
        correlation_id: Option<String>,
    },
    ActionAccepted {
        action_type: String,
        correlation_id: String,
    },
    UpdateBytes {
        bytes: Vec<u8>,
    },
    CapabilityFailure(CapabilityFailure),
    /// #1753 S6 — a sign capability request the worker emits for the
    /// **main-thread JS bridge** (the broker) to fulfil. The bridge calls
    /// `window.nostr.signEvent(unsigned_json)` (ensuring `window.nostr` is on
    /// `account_pubkey` first) and posts the result back as a
    /// [`WorkerRequest::DeliverSignerResponse`] carrying this `correlation_id`.
    SignRequest {
        correlation_id: String,
        account_pubkey: String,
        unsigned_json: String,
    },
    /// #1753 S6 — a sign round-trip completed via message re-entry. Carries the
    /// signed flat-NIP-01 JSON. For publish-dispatch round-trips the wasm
    /// runtime also consumes the same completion to call `publish_pre_signed`
    /// and fan outbound frames; the JS host never publishes inline.
    SignCompleted {
        correlation_id: String,
        signed_json: String,
    },
    /// #1753 S6 — a sign round-trip failed (parse error, account-pin mismatch,
    /// user rejection, or an unknown/stale correlation id).
    SignFailed {
        correlation_id: String,
        reason: String,
    },
    Error {
        code: String,
        message: String,
        correlation_id: Option<String>,
    },
}

/// Normalise the relay bootstrap list from a [`StartConfig`]: if
/// `relay_bootstrap` is non-empty, it is the authoritative list; otherwise
/// synthesise one `RelayBootstrapEntry` per URL string in `relays` with
/// `role = "both"`.
pub(crate) fn relay_bootstrap_from_config(
    relays: Vec<String>,
    relay_bootstrap: Vec<RelayBootstrapEntry>,
) -> Vec<RelayBootstrapEntry> {
    if !relay_bootstrap.is_empty() {
        return relay_bootstrap;
    }
    relays
        .into_iter()
        .map(|url| RelayBootstrapEntry {
            url,
            role: "both".to_string(),
        })
        .collect()
}
