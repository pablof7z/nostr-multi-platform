//! `HttpCapability` — host-injected HTTP transport for kernel protocol modules.
//!
//! The kernel never makes HTTP calls directly; it routes them through this
//! capability so platform code (iOS URLSession, desktop reqwest, etc.) supplies
//! the transport. The current socket is synchronous (the actor thread blocks
//! for the duration of each HTTP call). For rare user-triggered actions like
//! NIP-57 zaps this is acceptable; a non-blocking async variant is future work.
//!
//! It is the second [`CapabilityModule`] after `KeyringCapability`, and it
//! follows that module's proven shape exactly: a zero-sized typed marker
//! carrying the namespace + the request/result vocabulary, plus a `*Wiring`
//! helper that builds the generic [`CapabilityRequest`] envelopes the issuing
//! protocol module hands to the FFI capability socket.
//!
//! **D0**: no app nouns — request/result carry only generic HTTP primitives.
//! **D6**: failures are `HttpResult::Error { message }` data, never panics.
//! **D7**: capability reports and executes; it never decides which URL to call.
//!   *Which* LNURL endpoint to hit and *what* to do with the bolt11 invoice
//!   that comes back are NIP-57 policy decisions (see `nmp-nip57`); this
//!   capability only performs the GET/POST it is handed and reports the result.

use serde::{Deserialize, Serialize};

use super::capability::CapabilityModule;

/// Typed marker for the HTTP capability. Carries the namespace + the
/// request/result vocabulary; the platform supplies the actual transport
/// (iOS `URLSession`, desktop `reqwest`, …) behind the FFI capability socket.
pub struct HttpCapability;

impl CapabilityModule for HttpCapability {
    const NAMESPACE: &'static str = "nmp.http.capability";

    type Request = HttpRequest;
    type Result = HttpResult;

    fn callback_interface_name() -> &'static str {
        "HttpCapabilityCallback"
    }
}

/// Capability-private request payload — the decoded `payload_json`.
///
/// Wire shape (matches Swift `HttpRequest`):
/// * `{"method":"GET","url":"https://…"}`
/// * `{"method":"POST","url":"https://…","headers":[["Content-Type","…"]],
///    "body":"…"}`
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    /// Header name/value pairs. Omitted from the wire when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<[String; 2]>,
    /// Request body — present for POST, absent for GET.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// HTTP verb. Only the two LNURL-pay needs (GET the lnurl-pay metadata, POST
/// the signed kind:9734 to the callback) — kept deliberately minimal (D-anti-
/// abstraction): more verbs are added when a caller needs them, not before.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
}

/// Capability-private result payload — the encoded `result_json`.
///
/// Note there is no error *exception*: a failure is data (`status == "error"`
/// with a human-readable `message`), satisfying D6. An `Ok` carries the raw
/// HTTP `status_code` so the caller can distinguish a transport success from
/// an application-level non-2xx (e.g. a 404 from the lnurl endpoint).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HttpResult {
    Ok { status_code: u16, body: String },
    Error { message: String },
}

impl HttpResult {
    /// Successful transport — `status_code` is the raw HTTP status, `body` the
    /// response body. Note a `200` and a `404` are both `Ok`: the transport
    /// succeeded; interpreting the status is the caller's policy (D7).
    pub fn ok(status_code: u16, body: impl Into<String>) -> Self {
        HttpResult::Ok {
            status_code,
            body: body.into(),
        }
    }

    /// Transport-level failure (DNS, TLS, timeout, malformed request, …).
    pub fn error(message: impl Into<String>) -> Self {
        HttpResult::Error {
            message: message.into(),
        }
    }

    /// The response body, or `None` for an `Error`.
    pub fn body(&self) -> Option<&str> {
        match self {
            HttpResult::Ok { body, .. } => Some(body),
            HttpResult::Error { .. } => None,
        }
    }

    /// `true` when the transport succeeded (regardless of HTTP status code).
    pub fn is_ok(&self) -> bool {
        matches!(self, HttpResult::Ok { .. })
    }
}

/// Builds the HTTP `CapabilityRequest` envelopes a protocol module issues to
/// perform an LNURL-pay round-trip (or any other host HTTP call). The protocol
/// module *decides* (policy, D7) which URL to call and how to react to the
/// result; the capability merely *executes* the GET/POST and *reports* it.
///
/// `correlation_id`s are caller-supplied so the issuing module can match the
/// returned [`CapabilityEnvelope`](super::capability::CapabilityEnvelope) to
/// its in-flight request.
pub struct HttpCapabilityWiring;

impl HttpCapabilityWiring {
    /// Build a GET `CapabilityRequest` for `url` (e.g. the lnurl-pay metadata
    /// endpoint).
    pub fn get(
        correlation_id: impl Into<String>,
        url: impl Into<String>,
    ) -> crate::substrate::capability::CapabilityRequest {
        Self::request(
            correlation_id,
            HttpRequest {
                method: HttpMethod::Get,
                url: url.into(),
                headers: vec![],
                body: None,
            },
        )
    }

    /// Build a POST `CapabilityRequest` for `url` carrying `body` with the
    /// given `content_type` header (e.g. POSTing a signed kind:9734 to the
    /// lnurl-pay callback).
    pub fn post(
        correlation_id: impl Into<String>,
        url: impl Into<String>,
        body: impl Into<String>,
        content_type: impl Into<String>,
    ) -> crate::substrate::capability::CapabilityRequest {
        Self::request(
            correlation_id,
            HttpRequest {
                method: HttpMethod::Post,
                url: url.into(),
                headers: vec![["Content-Type".into(), content_type.into()]],
                body: Some(body.into()),
            },
        )
    }

    /// Decode the [`CapabilityEnvelope`](super::capability::CapabilityEnvelope)
    /// the capability handed back into a typed [`HttpResult`]. A malformed
    /// envelope is itself reported as an `HttpResult::error` (D6: never an
    /// exception across the boundary).
    pub fn decode_result(
        envelope: &crate::substrate::capability::CapabilityEnvelope,
    ) -> HttpResult {
        serde_json::from_str(&envelope.result_json)
            .unwrap_or_else(|e| HttpResult::error(format!("malformed-result: {e}")))
    }

    fn request(
        correlation_id: impl Into<String>,
        request: HttpRequest,
    ) -> crate::substrate::capability::CapabilityRequest {
        use crate::substrate::capability::CapabilityRequest;
        CapabilityRequest {
            namespace: HttpCapability::NAMESPACE.to_string(),
            correlation_id: correlation_id.into(),
            // `serde_json::to_string` on this closed shape cannot realistically
            // fail; the `unwrap_or` keeps the path panic-free (D6).
            payload_json: serde_json::to_string(&request)
                .unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::capability::CapabilityEnvelope;

    #[test]
    fn namespace_is_nmp_http_capability() {
        assert_eq!(HttpCapability::NAMESPACE, "nmp.http.capability");
        assert_eq!(
            HttpCapability::callback_interface_name(),
            "HttpCapabilityCallback"
        );
    }

    #[test]
    fn get_builds_correct_request_json() {
        let req = HttpCapabilityWiring::get(
            "corr-1",
            "https://ln.example/.well-known/lnurlp/alice",
        );
        assert_eq!(req.namespace, HttpCapability::NAMESPACE);
        assert_eq!(req.correlation_id, "corr-1");

        let payload: HttpRequest =
            serde_json::from_str(&req.payload_json).unwrap();
        assert_eq!(payload.method, HttpMethod::Get);
        assert_eq!(
            payload.url,
            "https://ln.example/.well-known/lnurlp/alice"
        );
        // GET carries no headers and no body — both elided from the wire.
        assert!(payload.headers.is_empty());
        assert_eq!(payload.body, None);
        assert!(!req.payload_json.contains("headers"));
        assert!(!req.payload_json.contains("body"));
    }

    #[test]
    fn post_includes_content_type_header() {
        let req = HttpCapabilityWiring::post(
            "corr-2",
            "https://ln.example/callback?amount=21000",
            r#"{"nostr":"…"}"#,
            "application/json",
        );
        assert_eq!(req.namespace, HttpCapability::NAMESPACE);
        assert_eq!(req.correlation_id, "corr-2");

        let payload: HttpRequest =
            serde_json::from_str(&req.payload_json).unwrap();
        assert_eq!(payload.method, HttpMethod::Post);
        assert_eq!(
            payload.headers,
            vec![["Content-Type".to_string(), "application/json".to_string()]]
        );
        assert_eq!(payload.body.as_deref(), Some(r#"{"nostr":"…"}"#));
    }

    #[test]
    fn method_serialises_to_uppercase() {
        // The Swift side emits exactly these strings.
        assert_eq!(
            serde_json::to_string(&HttpMethod::Get).unwrap(),
            r#""GET""#
        );
        assert_eq!(
            serde_json::to_string(&HttpMethod::Post).unwrap(),
            r#""POST""#
        );
    }

    #[test]
    fn decode_ok_result() {
        let envelope = CapabilityEnvelope {
            namespace: HttpCapability::NAMESPACE.to_string(),
            correlation_id: "c".to_string(),
            result_json:
                r#"{"status":"ok","status_code":200,"body":"{\"pr\":\"lnbc1\"}"}"#
                    .to_string(),
        };
        let result = HttpCapabilityWiring::decode_result(&envelope);
        assert!(result.is_ok());
        assert_eq!(result.body(), Some(r#"{"pr":"lnbc1"}"#));
        match result {
            HttpResult::Ok { status_code, .. } => assert_eq!(status_code, 200),
            HttpResult::Error { .. } => panic!("expected Ok"),
        }
    }

    #[test]
    fn decode_error_result() {
        let envelope = CapabilityEnvelope {
            namespace: HttpCapability::NAMESPACE.to_string(),
            correlation_id: "c".to_string(),
            result_json: r#"{"status":"error","message":"dns-failure"}"#
                .to_string(),
        };
        let result = HttpCapabilityWiring::decode_result(&envelope);
        assert!(!result.is_ok());
        assert_eq!(result.body(), None);
        match result {
            HttpResult::Error { message } => assert_eq!(message, "dns-failure"),
            HttpResult::Ok { .. } => panic!("expected Error"),
        }
    }

    #[test]
    fn ok_and_error_round_trip_through_serde() {
        let ok = HttpResult::ok(201, "created");
        let back: HttpResult =
            serde_json::from_str(&serde_json::to_string(&ok).unwrap())
                .unwrap();
        assert!(back.is_ok());
        assert_eq!(back.body(), Some("created"));

        let err = HttpResult::error("timeout");
        let back: HttpResult =
            serde_json::from_str(&serde_json::to_string(&err).unwrap())
                .unwrap();
        assert!(!back.is_ok());
    }

    #[test]
    fn decode_malformed_envelope_reports_error() {
        // D6: a non-JSON `result_json` must surface as an `HttpResult::Error`,
        // never an exception across the boundary.
        let envelope = CapabilityEnvelope {
            namespace: HttpCapability::NAMESPACE.to_string(),
            correlation_id: "c".to_string(),
            result_json: "not json at all".to_string(),
        };
        let result = HttpCapabilityWiring::decode_result(&envelope);
        match result {
            HttpResult::Error { message } => {
                assert!(message.starts_with("malformed-result:"));
            }
            HttpResult::Ok { .. } => panic!("expected Error for malformed JSON"),
        }
    }
}
