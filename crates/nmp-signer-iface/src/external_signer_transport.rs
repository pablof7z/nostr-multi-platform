//! NIP-55 external-signer transport contract.
//!
//! [`ExternalSignerRequest`] / [`ExternalSignerResponse`] are the typed
//! request/response types the Rust `Nip55Signer` builds and parses. The host
//! (Kotlin) fires what Rust built and reports raw results — it decides nothing
//! (D7). Placed here in the leaf `nmp-signer-iface` crate so both `nmp-core`
//! (which holds `Arc<dyn ExternalSignerTransport>` as the capability) and
//! `nmp-signers` (which builds the requests) can import them without a D0
//! cycle.
//!
//! ## NIP-55 overview
//!
//! On Android, NIP-55 is the analogue of NIP-07 (web `window.nostr`): a
//! per-request Android Intent or background `ContentResolver` query to a
//! separate signer app (Amber / `nostrsigner:`) that holds the user's key.
//! The key never enters the NMP process.
//!
//! Unlike NIP-46 (relay round-trip, < 1s), an Intent round-trip requires the
//! user to foreground Amber and tap approve (5–30s). The per-op deadline
//! (`RemoteSignerHandle::op_timeout()` = 90s) exists for this reason.
//!
//! ## Transport choice (host-side, D7)
//!
//! - **Intent round-trip** — used when a method's permission has not been
//!   pre-granted. Rust sets `permissions` on the first `get_public_key` call.
//! - **`ContentResolver` fast-path** — used when the method is in
//!   `granted_permissions`. Rust sets `force_interactive: false`; host
//!   mechanically checks the grant list and picks the resolver path.
//! - A `ContentResolver` returning `null` (silently revoked) surfaces as
//!   [`ExternalSignerOutcome::Unavailable`]; Rust re-issues the same op with
//!   `force_interactive: true` so it falls to the Intent path — never the
//!   host retrying on its own (D7: native never retries/decides).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::SignerError;

/// Capability namespace for the NIP-55 external-signer bridge (ADR-0048 D2).
///
/// `ExternalSignerRequest` rides the existing `CapabilityRequest` carrier as
/// `payload_json` under this namespace; the host adapter recognises it and
/// dispatches the Intent / `ContentResolver` round-trip.
pub const EXTERNAL_SIGNER_NAMESPACE: &str = "external_signer";

/// Default per-op deadline budget for remote signer operations.
///
/// 5s — long enough for a fast / auto-approving NIP-46 bunker, short enough
/// that a crashed broker cannot strand the publish queue. This is the baseline;
/// individual signer kinds override it via `RemoteSignerHandle::op_timeout()`.
///
/// Defined here (leaf crate) so that `nmp-core/src/remote_signer.rs` can
/// reference it in the default `op_timeout()` impl without a feature gate —
/// `pending_sign.rs` is `#[cfg(feature = "native")]`-gated and therefore
/// unavailable in the always-compiled trait surface.
pub const PENDING_SIGN_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-op deadline budget for NIP-55 signer operations.
///
/// An Android Intent round-trip requires the user to foreground Amber and tap
/// approve — 5–30s in typical usage, occasionally more if the app is cold. 90s
/// gives ample headroom without stranding the publish queue indefinitely
/// (ADR-0048 D3). `Nip55Signer` returns this via `RemoteSignerHandle::op_timeout()`.
pub const EXTERNAL_SIGN_TIMEOUT: Duration = Duration::from_secs(90);

/// Methods the Rust layer can request from a NIP-55 external signer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalSignerMethod {
    /// Probe the signer's current active pubkey. Always the first call;
    /// carries the permission batch on first connect.
    GetPublicKey,
    /// Sign an unsigned event (NIP-01 JSON body).
    SignEvent,
    /// NIP-44 v2 encrypt `plaintext` to a counterparty pubkey.
    Nip44Encrypt,
    /// NIP-44 v2 decrypt a ciphertext from a counterparty pubkey.
    Nip44Decrypt,
}

impl ExternalSignerMethod {
    /// Permission-kind prefix used for ContentResolver fast-path admission.
    ///
    /// `get_public_key` is intentionally excluded: first-connect is always an
    /// interactive permission request, not a background resolver operation.
    #[must_use]
    pub fn granted_permission_prefix(&self) -> Option<&'static str> {
        match self {
            Self::GetPublicKey => None,
            Self::SignEvent => Some("sign_event:"),
            Self::Nip44Encrypt => Some("nip44_encrypt"),
            Self::Nip44Decrypt => Some("nip44_decrypt"),
        }
    }
}

/// A permission token for the NIP-55 first-connect batch request.
///
/// Amber grants the listed permissions permanently so subsequent calls can
/// use the `ContentResolver` fast-path without launching an Intent. Rust
/// decides which permissions to request (policy); the host fires them (D7).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip55Permission {
    /// Permission kind, e.g. `"sign_event:1"`, `"nip44_encrypt"`,
    /// `"nip44_decrypt"`.
    pub kind: String,
}

impl Nip55Permission {
    /// Construct a sign-event permission for a specific kind number.
    #[must_use]
    pub fn sign_event(kind: u16) -> Self {
        Self {
            kind: format!("sign_event:{kind}"),
        }
    }

    /// Construct an unconditional NIP-44 encrypt permission.
    #[must_use]
    pub fn nip44_encrypt() -> Self {
        Self {
            kind: "nip44_encrypt".to_string(),
        }
    }

    /// Construct an unconditional NIP-44 decrypt permission.
    #[must_use]
    pub fn nip44_decrypt() -> Self {
        Self {
            kind: "nip44_decrypt".to_string(),
        }
    }

    /// Construct an unconditional NIP-04 encrypt permission.
    #[must_use]
    pub fn nip04_encrypt() -> Self {
        Self {
            kind: "nip04_encrypt".to_string(),
        }
    }

    /// Construct an unconditional NIP-04 decrypt permission.
    #[must_use]
    pub fn nip04_decrypt() -> Self {
        Self {
            kind: "nip04_decrypt".to_string(),
        }
    }
}

/// Outbound request built by `Nip55Signer` and handed to the transport.
///
/// Serialized to `payload_json` inside `CapabilityRequest { namespace:
/// "external_signer", .. }` and sent across the FFI capability socket.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalSignerRequest {
    /// Echoed back in the response to correlate with the pending `Sender`.
    pub correlation_id: String,
    /// Operation to perform.
    pub method: ExternalSignerMethod,
    /// NIP-55 payload:
    /// - `sign_event`: unsigned event JSON string.
    /// - `nip44_encrypt`: plaintext.
    /// - `nip44_decrypt`: ciphertext/payload.
    /// - `get_public_key`: empty string (the signer supplies the pubkey).
    pub payload: String,
    /// Current user pubkey (hex). `None` only for the initial
    /// `get_public_key` request. Used by Amber to confirm which key to
    /// sign with, surfacing `Unavailable` if the active key changed.
    pub current_user: Option<String>,
    /// Counterparty pubkey for encrypt/decrypt ops (hex). `None` for sign.
    pub counterparty: Option<String>,
    /// Non-empty **only** on the first `get_public_key` request (the
    /// permission batch). Empty on all subsequent calls.
    pub permissions: Vec<Nip55Permission>,
    /// Persisted permissions the signer app has already granted.
    ///
    /// Distinct from [`Self::permissions`]: this field is a capability fact
    /// used to select the `ContentResolver` fast-path, while `permissions` is
    /// the requested permission batch sent to Amber only on interactive
    /// permission requests.
    #[serde(default)]
    pub granted_permissions: Vec<Nip55Permission>,
    /// Package name of the signer app (e.g. `"com.greenart7c3.nostrsigner"`).
    /// `None` on the very first `get_public_key` (host resolves which app);
    /// `Some` on all subsequent calls once the package is known.
    pub signer_package: Option<String>,
    /// When `true`, force an Intent round-trip even if the permission is in
    /// `granted_permissions`. Used by Rust after a `ContentResolver` returns
    /// `null` (silently-revoked permission) to re-issue the op interactively.
    /// The host checks this flag; it never retries on its own (D7).
    #[serde(default)]
    pub force_interactive: bool,
}

impl ExternalSignerRequest {
    /// Whether this request is eligible for the NIP-55 ContentResolver path.
    ///
    /// This is the Rust-owned mirror of the Android bridge's mechanical
    /// transport selection. Rust needs it to reject overlapping interactive
    /// approvals and to decide when an `Unavailable` result may be retried via
    /// Intent. Native still performs the OS dispatch and reports raw results.
    #[must_use]
    pub fn uses_content_resolver_fast_path(&self) -> bool {
        !self.force_interactive
            && self.signer_package.is_some()
            && self
                .method
                .granted_permission_prefix()
                .is_some_and(|prefix| {
                    self.granted_permissions
                        .iter()
                        .any(|p| p.kind.starts_with(prefix))
                })
    }

    /// Whether dispatching this request will require an interactive Intent.
    #[must_use]
    pub fn requires_interactive_intent(&self) -> bool {
        !self.uses_content_resolver_fast_path()
    }
}

/// Outcome of an external-signer request reported by the host.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalSignerOutcome {
    /// Operation succeeded. Carries the raw result string:
    /// - `sign_event`: signed event JSON.
    /// - `nip44_encrypt`: ciphertext string.
    /// - `nip44_decrypt`: plaintext string.
    /// - `get_public_key`: pubkey hex.
    Ok {
        /// Raw result string returned by the signer app.
        result: String,
    },
    /// The user explicitly rejected the request inside Amber.
    Rejected {
        /// Human-readable rejection reason from the signer app.
        reason: String,
    },
    /// Signer app is not installed or was uninstalled mid-session.
    Unavailable {
        /// Human-readable reason (e.g. "signer not installed").
        reason: String,
    },
    /// Signer app returned a recognisable error (wrong key, malformed
    /// response, etc.). Human-readable; displayed as a D6 toast.
    SignerError {
        /// Human-readable error from the signer app.
        reason: String,
    },
}

/// Inbound response reported by the host (D7 — raw results only).
///
/// Deserialized from `result_json` inside `CapabilityEnvelope` on the
/// `"external_signer"` namespace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalSignerResponse {
    /// Must equal the `correlation_id` in the original request.
    pub correlation_id: String,
    /// What happened.
    pub outcome: ExternalSignerOutcome,
    /// The signer app's package name as reported by the OS on the
    /// `get_public_key` reply. `None` for all other methods (the caller
    /// already knows the package by then).
    pub signer_package: Option<String>,
}

/// The outbound transport contract for NIP-55.
///
/// `Nip55Signer` calls `send_request` to hand a fully-built
/// [`ExternalSignerRequest`] to the host capability bridge. The production
/// bridge serializes it into a `CapabilityRequest` and dispatches it through
/// the registered FFI callback. Tests implement this with a `FakeExternalSignerTransport`
/// that captures requests and drives responses synchronously.
///
/// Per D7 the transport must NOT make decisions — it fires the request and
/// reports the raw result unchanged.
pub trait ExternalSignerTransport: Send + Sync + std::fmt::Debug {
    /// Fire a request. Returns immediately; the response arrives later via
    /// `Nip55Signer::deliver_response`.
    fn send_request(&self, request: ExternalSignerRequest) -> Result<(), SignerError>;
}
