//! Host-facing upload completion carrier — parse the `action_results` terminal.
//!
//! Blossom uploads are async-completing: `nmp_app_dispatch_action_bytes` returns
//! a `correlation_id` immediately; the blob descriptor (`url` + `sha256`, …)
//! surfaces on a **later** snapshot tick in the kernel-owned `action_results`
//! projection (ADR-0043 Decision 4). This is the canonical completion path —
//! **not** `register_action_result_observer`, which fires only when the action
//! is accepted/enqueued.
//!
//! # Host contract
//!
//! 1. Dispatch `nmp.blossom.upload` via `nmp_app_dispatch_action_bytes` and
//!    retain the returned `correlation_id`.
//! 2. On each update tick, drain `projections["action_results"]` (or the typed
//!    `action_results` / `KARS` sidecar).
//! 3. Find the row whose `correlation_id` matches; when `status == "published"`,
//!    parse `result` with [`parse_upload_completion`].
//!
//! No native waiter dictionary is required — correlation is keyed on the
//! dispatch-returned id alone.

use serde::{Deserialize, Serialize};

use crate::upload::http::BlobDescriptor;

/// One server's outcome in a multi-server upload terminal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerUploadOutcome {
    pub server: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Parsed upload completion — single-server flat descriptor or multi-server
/// aggregate (Decision 4 shapes in ADR-0043).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadCompletion {
    /// One server accepted — the flat BUD-02 descriptor is the `result` body.
    Single(BlobDescriptor),
    /// Multiple servers — aggregated shape with per-server itemisation.
    Multi {
        sha256: String,
        size: u64,
        mime_type: String,
        uploaded: u64,
        servers: Vec<ServerUploadOutcome>,
    },
}

/// Parse the opaque `result` JSON object from an `action_results` terminal row.
///
/// Accepts both Decision-4 shapes (flat descriptor vs multi-server aggregate).
/// Returns an error when required fields are missing or the JSON is malformed.
pub fn parse_upload_completion(result: &serde_json::Value) -> Result<UploadCompletion, String> {
    if result.get("servers").and_then(|v| v.as_array()).is_some() {
        let sha256 = required_str(result, "sha256")?;
        let size = required_u64(result, "size")?;
        let mime_type = required_str(result, "type")?;
        let uploaded = required_u64(result, "uploaded")?;
        let servers: Vec<ServerUploadOutcome> = result
            .get("servers")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "multi-server result requires servers[]".to_string())?
            .iter()
            .map(|entry| {
                serde_json::from_value(entry.clone()).map_err(|e| format!("servers[] entry: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(UploadCompletion::Multi {
            sha256,
            size,
            mime_type,
            uploaded,
            servers,
        });
    }

    let descriptor: BlobDescriptor = serde_json::from_value(result.clone())
        .map_err(|e| format!("parse flat BUD-02 descriptor: {e}"))?;
    Ok(UploadCompletion::Single(descriptor))
}

/// Convenience: `url` and `sha256` from any completion shape.
#[must_use]
pub fn completion_url_sha256(completion: &UploadCompletion) -> (String, String) {
    match completion {
        UploadCompletion::Single(d) => (d.url.clone(), d.sha256.clone()),
        UploadCompletion::Multi {
            sha256, servers, ..
        } => {
            let url = servers
                .iter()
                .find(|s| s.ok)
                .and_then(|s| s.url.clone())
                .unwrap_or_default();
            (url, sha256.clone())
        }
    }
}

fn required_str(value: &serde_json::Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("result missing required field `{key}`"))
}

fn required_u64(value: &serde_json::Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("result missing required field `{key}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_server_flat_descriptor() {
        let result = serde_json::json!({
            "url": "https://b.example/abc.png",
            "sha256": "deadbeef",
            "size": 5,
            "type": "image/png",
            "uploaded": 1733356800
        });
        let completion = parse_upload_completion(&result).expect("flat descriptor");
        let (url, sha256) = completion_url_sha256(&completion);
        assert_eq!(url, "https://b.example/abc.png");
        assert_eq!(sha256, "deadbeef");
        assert!(matches!(completion, UploadCompletion::Single(_)));
    }

    #[test]
    fn parse_multi_server_aggregate() {
        let result = serde_json::json!({
            "sha256": "abc",
            "size": 5,
            "type": "image/png",
            "uploaded": 1733356800,
            "servers": [
                { "server": "https://b1.example", "ok": true, "url": "https://b1.example/abc.png" },
                { "server": "https://b2.example", "ok": false, "error": "413" }
            ]
        });
        let completion = parse_upload_completion(&result).expect("aggregate");
        let (url, sha256) = completion_url_sha256(&completion);
        assert_eq!(url, "https://b1.example/abc.png");
        assert_eq!(sha256, "abc");
        assert!(matches!(completion, UploadCompletion::Multi { .. }));
    }
}
