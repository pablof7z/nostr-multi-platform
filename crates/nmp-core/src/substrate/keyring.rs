//! Kernel-side `KeyringCapability` contract.
//!
//! Rust-side `KeyringCapability` contract — the store/retrieve/delete-by-`account_id`
//! vocabulary the host keychain implementation speaks. Shell implementations
//! were already built against the *generic* `CapabilityRequest`/`CapabilityEnvelope`
//! before this typed contract existed; the JSON shapes here are
//! byte-compatible with those `KeyringRequest`/`KeyringResult` types.
//!
//! Doctrine (`docs/product-spec/doctrine.md`):
//! * **D0** — this is generic key/secret storage keyed by an opaque
//!   `account_id`. There are no app nouns: the kernel never learns what a
//!   secret *is*, only that some bytes must be persisted/recalled.
//! * **D6** — no error ever leaves this module as an exception. Every failure
//!   is `KeyringResult { status: "error", .. }` data inside the envelope.
//! * **D7** — the capability reports and executes; it never decides policy.
//!   *Which* account is active and *when* a secret is forgotten are
//!   identity-layer decisions (see [`KeyringIdentityWiring`]).

use serde::{Deserialize, Serialize};

use super::capability::{CapabilityEnvelope, CapabilityModule, CapabilityRequest};

/// Typed marker for the keyring capability. Carries the namespace + the
/// request/result vocabulary; the platform supplies the actual secret store
/// (iOS Keychain, Android Keystore, …) behind the FFI capability socket.
pub struct KeyringCapability;

impl CapabilityModule for KeyringCapability {
    const NAMESPACE: &'static str = "nmp.keyring.capability";

    type Request = KeyringRequest;
    type Result = KeyringResult;

    fn callback_interface_name() -> &'static str {
        "KeyringCapabilityCallback"
    }
}

/// Capability-private request payload — the decoded `payload_json`.
///
/// Wire shape (matches Swift `KeyringRequest`):
/// * `{"op":"store","account_id":"<id>","secret":"<secret>"}`
/// * `{"op":"retrieve","account_id":"<id>"}`
/// * `{"op":"delete","account_id":"<id>"}`
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum KeyringRequest {
    /// Persist `secret` under `account_id`. Overwrites any existing value.
    Store { account_id: String, secret: String },
    /// Read the secret stored under `account_id`.
    Retrieve { account_id: String },
    /// Remove the secret stored under `account_id` (no-op if absent).
    Delete { account_id: String },
}

impl KeyringRequest {
    /// The `account_id` this request is keyed by, regardless of variant.
    #[must_use]
    pub fn account_id(&self) -> &str {
        match self {
            Self::Store { account_id, .. }
            | Self::Retrieve { account_id }
            | Self::Delete { account_id } => account_id,
        }
    }
}

/// Capability-private result payload — the encoded `result_json`.
///
/// Note there is no error *exception*: a failure is data (`status == "error"`
/// with an optional `os_status` code), satisfying D6. Wire shape matches Swift
/// `KeyringResult` — `secret`/`os_status` are omitted when absent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyringResult {
    /// `"ok"` | `"not_found"` | `"error"`.
    pub status: KeyringStatus,
    /// Populated only for a successful `retrieve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    /// Raw platform status code for diagnostics; absent on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_status: Option<i32>,
}

impl KeyringResult {
    /// Successful operation; `secret` is `Some` only for a `retrieve` hit.
    #[must_use]
    pub fn ok(secret: Option<String>) -> Self {
        Self {
            status: KeyringStatus::Ok,
            secret,
            os_status: None,
        }
    }

    /// `retrieve` of an absent key — distinct from an error (D6 data).
    #[must_use]
    pub fn not_found() -> Self {
        Self {
            status: KeyringStatus::NotFound,
            secret: None,
            os_status: None,
        }
    }

    /// Platform-level failure carrying the native status code.
    #[must_use]
    pub fn error(os_status: i32) -> Self {
        Self {
            status: KeyringStatus::Error,
            secret: None,
            os_status: Some(os_status),
        }
    }
}

/// Tri-state outcome. Serialises to the lowercase strings the Swift side
/// emits (`"ok"`, `"not_found"`, `"error"`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyringStatus {
    Ok,
    NotFound,
    Error,
}

/// Identity-layer wiring: builds the keyring `CapabilityRequest`
/// envelopes the identity layer issues to persist/recall/forget the active
/// account secret. The identity layer *decides* (policy, D7) which account is
/// active and when to forget it; the capability merely *executes* (D7) the
/// resulting store/retrieve/delete and *reports* the result.
///
/// `correlation_id`s are caller-supplied so the issuing module can match the
/// returned [`CapabilityEnvelope`] to its in-flight request.
pub struct KeyringIdentityWiring;

impl KeyringIdentityWiring {
    /// Persist the active account secret under `account_id`.
    pub fn persist_secret(
        correlation_id: impl Into<String>,
        account_id: impl Into<String>,
        secret: impl Into<String>,
    ) -> CapabilityRequest {
        Self::request(
            correlation_id,
            KeyringRequest::Store {
                account_id: account_id.into(),
                secret: secret.into(),
            },
        )
    }

    /// Recall the account secret previously persisted under `account_id`.
    pub fn recall_secret(
        correlation_id: impl Into<String>,
        account_id: impl Into<String>,
    ) -> CapabilityRequest {
        Self::request(
            correlation_id,
            KeyringRequest::Retrieve {
                account_id: account_id.into(),
            },
        )
    }

    /// Forget the account secret stored under `account_id` (sign-out).
    pub fn forget_secret(
        correlation_id: impl Into<String>,
        account_id: impl Into<String>,
    ) -> CapabilityRequest {
        Self::request(
            correlation_id,
            KeyringRequest::Delete {
                account_id: account_id.into(),
            },
        )
    }

    /// Decode the [`CapabilityEnvelope`] the capability handed back into a
    /// typed [`KeyringResult`]. A malformed envelope is itself reported as a
    /// `KeyringResult::error` (D6: never an exception across the boundary).
    #[must_use]
    pub fn decode_result(envelope: &CapabilityEnvelope) -> KeyringResult {
        serde_json::from_str(&envelope.result_json)
            .unwrap_or_else(|_| KeyringResult::error(MALFORMED_RESULT))
    }

    fn request(correlation_id: impl Into<String>, request: KeyringRequest) -> CapabilityRequest {
        CapabilityRequest {
            namespace: KeyringCapability::NAMESPACE.to_string(),
            correlation_id: correlation_id.into(),
            // serde_json::to_string on a closed enum cannot fail; the
            // `unwrap_or` keeps this panic-free without a TODO/unwrap (D6).
            payload_json: serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

/// Synthetic status code reported when a capability result cannot be parsed.
/// Mirrors the Swift side's use of `errSecParam` (-50) for malformed data.
pub const MALFORMED_RESULT: i32 = -50;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_payload_matches_swift_wire_shape() {
        let store = KeyringRequest::Store {
            account_id: "acct-1".into(),
            secret: "nsec1abc".into(),
        };
        let json = serde_json::to_string(&store).unwrap();
        assert_eq!(
            json,
            r#"{"op":"store","account_id":"acct-1","secret":"nsec1abc"}"#
        );

        // The Swift side emits exactly this shape; round-trip it back.
        let decoded: KeyringRequest =
            serde_json::from_str(r#"{"op":"retrieve","account_id":"acct-1"}"#).unwrap();
        assert_eq!(
            decoded,
            KeyringRequest::Retrieve {
                account_id: "acct-1".into()
            }
        );

        let decoded: KeyringRequest =
            serde_json::from_str(r#"{"op":"delete","account_id":"acct-1"}"#).unwrap();
        assert_eq!(decoded.account_id(), "acct-1");
    }

    #[test]
    fn result_payload_matches_swift_wire_shape() {
        // success retrieve: secret present, os_status omitted
        let ok = KeyringResult::ok(Some("nsec1abc".into()));
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"status":"ok","secret":"nsec1abc"}"#
        );

        // success store: nothing but status
        assert_eq!(
            serde_json::to_string(&KeyringResult::ok(None)).unwrap(),
            r#"{"status":"ok"}"#
        );

        // not_found and error
        assert_eq!(
            serde_json::to_string(&KeyringResult::not_found()).unwrap(),
            r#"{"status":"not_found"}"#
        );
        assert_eq!(
            serde_json::to_string(&KeyringResult::error(-25300)).unwrap(),
            r#"{"status":"error","os_status":-25300}"#
        );

        // Swift-emitted error result decodes back.
        let decoded: KeyringResult =
            serde_json::from_str(r#"{"status":"error","os_status":-50}"#).unwrap();
        assert_eq!(decoded, KeyringResult::error(-50));
    }

    #[test]
    fn identity_wiring_builds_namespaced_envelopes() {
        let req = KeyringIdentityWiring::persist_secret("corr-1", "acct-1", "nsec1abc");
        assert_eq!(req.namespace, KeyringCapability::NAMESPACE);
        assert_eq!(req.correlation_id, "corr-1");
        let payload: KeyringRequest = serde_json::from_str(&req.payload_json).unwrap();
        assert_eq!(
            payload,
            KeyringRequest::Store {
                account_id: "acct-1".into(),
                secret: "nsec1abc".into(),
            }
        );

        let recall = KeyringIdentityWiring::recall_secret("c", "acct-1");
        assert_eq!(
            serde_json::from_str::<KeyringRequest>(&recall.payload_json).unwrap(),
            KeyringRequest::Retrieve {
                account_id: "acct-1".into()
            }
        );

        let forget = KeyringIdentityWiring::forget_secret("c", "acct-1");
        assert_eq!(
            serde_json::from_str::<KeyringRequest>(&forget.payload_json).unwrap(),
            KeyringRequest::Delete {
                account_id: "acct-1".into()
            }
        );
    }

    #[test]
    fn decode_result_reports_malformed_envelope_as_data() {
        let bad = CapabilityEnvelope {
            namespace: KeyringCapability::NAMESPACE.to_string(),
            correlation_id: "c".to_string(),
            result_json: "not json".to_string(),
        };
        assert_eq!(
            KeyringIdentityWiring::decode_result(&bad),
            KeyringResult::error(MALFORMED_RESULT)
        );
    }
}
