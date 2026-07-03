//! `OutboundHttpCapability` — a protocol-neutral, host-executed outbound HTTP
//! round-trip, routed through the generic capability-callback socket
//! ([`super::capability`] / [`crate::capability_socket`]).
//!
//! # Why this exists
//!
//! Several protocol crates (NIP-57 LNURL, NIP-60 Cashu mint HTTP, and future
//! consumers) need to make an outbound HTTP request whose *construction* and
//! *response validation* must be Rust-owned (D0/D7 — policy never leaks to
//! the host), while the *transport* differs by runtime:
//!
//! - **Native** (iOS/Android/desktop): Rust makes the call directly (`ureq`
//!   on a spawned worker thread — see `nmp-nip57::lnurl::spawn_lnurl_worker`
//!   for the established pattern). No capability round-trip is needed here;
//!   native Rust already has raw sockets.
//! - **Browser** (wasm32): Rust cannot open a raw socket. The host JS
//!   environment must execute `fetch()` and hand the raw bytes back across
//!   the FFI boundary. This module is that seam for the browser case.
//!
//! `OutboundHttpCapability` carries ONLY transport-shaped data (method, URL,
//! headers, body, bounds) — no protocol noun ever crosses into `nmp-core`
//! (D0). A NIP crate builds the typed request its own protocol logic
//! requires, converts it to an [`OutboundHttpRequest`], dispatches it through
//! [`crate::capability_socket::dispatch_capability`] (or, on native, skips
//! this module entirely and calls its own HTTP client on a worker thread),
//! and parses the returned [`OutboundHttpResult`] with its own
//! protocol-specific validation. This module never parses or validates a
//! response body — that would require naming what the body means.
//!
//! # D6 — failures are data
//!
//! [`OutboundHttpResult`] has no "success" bias: `Response` carries whatever
//! status code the server returned (including 4xx/5xx — the caller's
//! protocol-specific validation decides what to do with it), and
//! `TransportError` / `Timeout` / `Canceled` cover the cases where no
//! response was ever received. There is no bare exception path.
//!
//! # No secret material in Debug/logs
//!
//! A request/response pair routinely carries mint quote ids, bearer tokens,
//! or Cashu proof secrets in its URL, headers, or body. Both types implement
//! a redacted `Debug` that never prints the URL, header values, or body
//! bytes — only shape (method, header count, body length, status code) —
//! so a stray `{:?}` in a log line cannot leak them.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::capability::CapabilityModule;

/// Typed marker for the outbound-HTTP capability. See the module docs.
pub struct OutboundHttpCapability;

impl CapabilityModule for OutboundHttpCapability {
    const NAMESPACE: &'static str = "nmp.outbound_http.capability";

    type Request = OutboundHttpRequest;
    type Result = OutboundHttpResult;

    fn callback_interface_name() -> &'static str {
        "OutboundHttpCapabilityCallback"
    }
}

/// HTTP method for an [`OutboundHttpRequest`]. Deliberately narrow — the
/// mint/LNURL/etc. HTTP surfaces this seam serves only ever need GET/POST.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OutboundHttpMethod {
    Get,
    Post,
}

/// A single request/response header. A `Vec` (not a map) because HTTP
/// permits repeated header names and the wire order is sometimes meaningful.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboundHttpHeader {
    pub name: String,
    pub value: String,
}

impl fmt::Debug for OutboundHttpHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Header VALUES can carry bearer tokens / cookies; only the name is
        // safe to print unconditionally.
        f.debug_struct("OutboundHttpHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// A Rust-constructed outbound HTTP request, handed to the host for
/// execution (browser `fetch()`) or serialized as a [`super::CapabilityRequest`]
/// payload.
#[derive(Clone, Deserialize, Serialize)]
pub struct OutboundHttpRequest {
    pub method: OutboundHttpMethod,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<OutboundHttpHeader>,
    #[serde(default)]
    pub body: Vec<u8>,
    /// Host-side deadline for the whole round-trip (connect + transfer).
    pub timeout_ms: u64,
    /// Upper bound on the response body the host should read before giving
    /// up — defends against a misbehaving/hostile server streaming an
    /// unbounded body at a caller that must buffer it in memory.
    pub max_response_bytes: u64,
}

impl fmt::Debug for OutboundHttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The URL and body routinely carry protocol-specific secrets (a
        // mint quote id in a path segment, a signed event in a POST body).
        // Only shape is safe to print.
        f.debug_struct("OutboundHttpRequest")
            .field("method", &self.method)
            .field("url", &"<redacted>")
            .field("header_count", &self.headers.len())
            .field("body_len", &self.body.len())
            .field("timeout_ms", &self.timeout_ms)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

/// The host's report of what happened to an [`OutboundHttpRequest`]. D6 —
/// every outcome is data; there is no panic/exception path out of the host
/// callback.
#[derive(Clone, Deserialize, Serialize)]
pub enum OutboundHttpResult {
    /// The host received a response. `status_code` may be any HTTP status,
    /// including 4xx/5xx — the caller's protocol-specific validation decides
    /// what a given status means.
    Response {
        status_code: u16,
        #[serde(default)]
        headers: Vec<OutboundHttpHeader>,
        #[serde(default)]
        body: Vec<u8>,
    },
    /// The request never reached a server, or the connection failed before a
    /// status line arrived (DNS failure, TLS failure, connection reset, …).
    ///
    /// `reason` is a short, host-constructed diagnostic string (e.g. "DNS
    /// resolution failed"). **Seam contract**: the host implementing this
    /// capability MUST NOT embed the request URL, headers, or body in
    /// `reason` — this type's `Debug` deliberately prints `reason` verbatim
    /// (unlike `Response.body`, which is always redacted) because a fixed
    /// diagnostic class string is not, on its own, a URL/quote-id/secret.
    /// The host, not this type, is responsible for keeping it that way.
    TransportError { reason: String },
    /// `timeout_ms` elapsed before a response arrived. Same `reason`
    /// contract as `TransportError`.
    Timeout { reason: String },
    /// The caller (or the host) abandoned the request before it completed
    /// (e.g. the owning operation was superseded). Same `reason` contract
    /// as `TransportError`.
    Canceled { reason: String },
}

impl fmt::Debug for OutboundHttpResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response {
                status_code,
                headers,
                body,
            } => f
                .debug_struct("OutboundHttpResult::Response")
                .field("status_code", status_code)
                .field("header_count", &headers.len())
                .field("body_len", &body.len())
                .finish(),
            Self::TransportError { reason } => f
                .debug_struct("OutboundHttpResult::TransportError")
                .field("reason", reason)
                .finish(),
            Self::Timeout { reason } => f
                .debug_struct("OutboundHttpResult::Timeout")
                .field("reason", reason)
                .finish(),
            Self::Canceled { reason } => f
                .debug_struct("OutboundHttpResult::Canceled")
                .field("reason", reason)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> OutboundHttpRequest {
        OutboundHttpRequest {
            method: OutboundHttpMethod::Post,
            url: "https://mint.example/v1/mint/quote/bolt11/super-secret-quote-id".into(),
            headers: vec![OutboundHttpHeader {
                name: "Authorization".into(),
                value: "Bearer top-secret-token".into(),
            }],
            body: br#"{"quote_secret":"do-not-leak"}"#.to_vec(),
            timeout_ms: 5_000,
            max_response_bytes: 1 << 20,
        }
    }

    #[test]
    fn request_serde_round_trips() {
        let req = sample_request();
        let json = serde_json::to_string(&req).expect("serialize");
        let back: OutboundHttpRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.method, OutboundHttpMethod::Post);
        assert_eq!(back.url, req.url);
        assert_eq!(back.headers.len(), 1);
        assert_eq!(back.body, req.body);
        assert_eq!(back.timeout_ms, req.timeout_ms);
        assert_eq!(back.max_response_bytes, req.max_response_bytes);
    }

    #[test]
    fn result_serde_round_trips_every_variant() {
        let variants = vec![
            OutboundHttpResult::Response {
                status_code: 200,
                headers: vec![],
                body: b"secret-mint-response-body".to_vec(),
            },
            OutboundHttpResult::TransportError {
                reason: "dns failure".into(),
            },
            OutboundHttpResult::Timeout {
                reason: "5000ms elapsed".into(),
            },
            OutboundHttpResult::Canceled {
                reason: "operation superseded".into(),
            },
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).expect("serialize");
            let _back: OutboundHttpResult = serde_json::from_str(&json).expect("deserialize");
        }
    }

    /// D6/security — a stray `{:?}` on the request must never leak the URL,
    /// header values, or body bytes (mint quote ids, bearer tokens, signed
    /// event payloads all pass through these fields for real callers).
    #[test]
    fn request_debug_redacts_url_headers_and_body() {
        let req = sample_request();
        let debug = format!("{req:?}");
        assert!(!debug.contains("mint.example"));
        assert!(!debug.contains("super-secret-quote-id"));
        assert!(!debug.contains("top-secret-token"));
        assert!(!debug.contains("do-not-leak"));
        assert!(debug.contains("body_len"));
    }

    #[test]
    fn result_debug_redacts_response_body() {
        let result = OutboundHttpResult::Response {
            status_code: 200,
            headers: vec![OutboundHttpHeader {
                name: "Set-Cookie".into(),
                value: "session=super-secret".into(),
            }],
            body: b"secret-mint-response-body".to_vec(),
        };
        let debug = format!("{result:?}");
        assert!(!debug.contains("secret-mint-response-body"));
        assert!(!debug.contains("session=super-secret"));
        assert!(debug.contains("200"));
    }
}
