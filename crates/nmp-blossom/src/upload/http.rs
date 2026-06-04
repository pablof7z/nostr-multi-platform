//! Blossom BUD-02 PUT transport + descriptor parse. Blocking I/O — runs ONLY on
//! the spawned worker thread (never the actor thread, D8).
//!
//! One server, one blob: stream the file body to `PUT {server}/upload` with the
//! `Authorization: Nostr <…>` header, then parse the BUD-02 blob descriptor
//! (`url`, `sha256`, `size`, `type`, `uploaded`) from the JSON response.
//!
//! Bounds (mirroring `nmp-nip57`'s LNURL caps): a per-upload timeout and a
//! max-response-bytes cap so a hostile / runaway server is a bounded error, not
//! an OOM or a wedged worker.

use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Per-upload HTTP budget. Generous (uploads can be multi-MB over slow links)
/// but bounded so a stuck server cannot accumulate worker threads forever.
const UPLOAD_HTTP_TIMEOUT_SECS: u64 = 60;

/// Maximum descriptor-response body the worker accepts. BUD-02 descriptors are
/// tiny JSON objects (a few hundred bytes); 64 KiB is orders of magnitude over
/// the spec. The cap makes a hostile / runaway response a bounded error.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// The BUD-02 blob descriptor a server returns on a successful upload.
///
/// `url`, `sha256`, `size`, and `uploaded` are required by BUD-02; `mime_type`
/// (wire key `type`) is optional. Unknown fields are ignored so a server that
/// returns extra metadata does not break the parse.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BlobDescriptor {
    /// Canonical URL to fetch the blob.
    pub url: String,
    /// SHA-256 of the blob, lowercase hex.
    pub sha256: String,
    /// Blob size in bytes.
    pub size: u64,
    /// MIME type (BUD-02 wire key `type`). Optional.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Upload Unix timestamp (seconds).
    pub uploaded: u64,
}

/// Parse a BUD-02 descriptor from a server's JSON response body. Separated from
/// the network call so it is unit-testable without a socket.
///
/// Returns an error string when the body is not valid JSON or is missing a
/// required field.
pub fn parse_descriptor(body: &[u8]) -> Result<BlobDescriptor, String> {
    serde_json::from_slice::<BlobDescriptor>(body)
        .map_err(|e| format!("parse BUD-02 descriptor: {e}"))
}

/// Build the BUD-02 upload endpoint from a server base URL. BUD-02 uploads PUT
/// to `{server}/upload`. Trailing slashes on the base are tolerated.
#[must_use]
pub fn upload_endpoint(server: &str) -> String {
    format!("{}/upload", server.trim_end_matches('/'))
}

/// Stream `body` (the blob bytes) to one Blossom server via BUD-02 PUT and parse
/// the returned descriptor.
///
/// * `server` — the blob-server base URL (e.g. `https://blossom.example`).
/// * `auth_header` — the full `Authorization` header value
///   (`Nostr <base64(signed kind:24242)>`).
/// * `content_type` — the blob MIME type (sent as `Content-Type`).
/// * `body` — the blob bytes (owned; streamed as the request body).
///
/// Non-2xx responses map to an error string carrying the status (and reason
/// phrase) so multi-server aggregation can itemise per-server failures.
pub fn put_blob(
    server: &str,
    auth_header: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<BlobDescriptor, String> {
    let endpoint = upload_endpoint(server);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(UPLOAD_HTTP_TIMEOUT_SECS))
        .build();
    let response = match agent
        .put(&endpoint)
        .set("Authorization", auth_header)
        .set("Content-Type", content_type)
        .send_bytes(&body)
    {
        Ok(resp) => resp,
        // `ureq` returns `Err(Status(code, resp))` for non-2xx — surface the
        // server's status + body so the per-server error is actionable.
        Err(ureq::Error::Status(code, resp)) => {
            let reason = resp.status_text().to_string();
            let detail = resp
                .into_string()
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(|s| format!(": {}", s.trim()))
                .unwrap_or_default();
            return Err(format!("{code} {reason}{detail}"));
        }
        Err(ureq::Error::Transport(t)) => {
            return Err(format!("PUT {endpoint} transport error: {t}"));
        }
    };

    // Bound the response so a runaway / hostile server cannot OOM the worker.
    let mut buf = Vec::with_capacity(1024);
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read descriptor body from {endpoint}: {e}"))?;
    parse_descriptor(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn upload_endpoint_appends_upload_path() {
        assert_eq!(
            upload_endpoint("https://b.example"),
            "https://b.example/upload"
        );
        assert_eq!(
            upload_endpoint("https://b.example/"),
            "https://b.example/upload"
        );
    }

    #[test]
    fn parse_descriptor_reads_all_bud02_fields() {
        let body = br#"{"url":"https://b.example/abc.png","sha256":"abc","size":2048,"type":"image/png","uploaded":1733356800}"#;
        let d = parse_descriptor(body).expect("valid descriptor");
        assert_eq!(d.url, "https://b.example/abc.png");
        assert_eq!(d.sha256, "abc");
        assert_eq!(d.size, 2048);
        assert_eq!(d.mime_type.as_deref(), Some("image/png"));
        assert_eq!(d.uploaded, 1733356800);
    }

    #[test]
    fn parse_descriptor_tolerates_missing_optional_type() {
        let body = br#"{"url":"u","sha256":"s","size":1,"uploaded":2}"#;
        let d = parse_descriptor(body).expect("type is optional");
        assert!(d.mime_type.is_none());
    }

    #[test]
    fn parse_descriptor_errors_on_missing_required_field() {
        // Missing `sha256` — a required field.
        let body = br#"{"url":"u","size":1,"uploaded":2}"#;
        assert!(parse_descriptor(body).is_err());
    }

    /// Minimal local HTTP/1.1 mock that reads the request, returns a fixed
    /// status + body, and records the request line + Authorization header it
    /// saw. Mirrors the standalone-socket approach a protocol-crate HTTP test
    /// needs (nmp-nip57 documents HTTP needs a live provider; Blossom PUT is
    /// simple enough to mock with a one-shot listener).
    fn spawn_mock(
        status_line: &'static str,
        response_body: &'static str,
    ) -> (
        String,
        std::sync::mpsc::Receiver<(String, Option<String>, Vec<u8>)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}");
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Read headers up to the blank line, then the body by
                // Content-Length.
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let mut header_end = None;
                let mut content_length = 0usize;
                loop {
                    let n = stream.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let headers = String::from_utf8_lossy(&buf[..pos]).to_string();
                            for line in headers.lines() {
                                if let Some(v) =
                                    line.to_ascii_lowercase().strip_prefix("content-length:")
                                {
                                    content_length = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                    if let Some(he) = header_end {
                        if buf.len() >= he + content_length {
                            break;
                        }
                    }
                }
                let he = header_end.unwrap_or(buf.len());
                let header_text = String::from_utf8_lossy(&buf[..he]).to_string();
                let request_line = header_text.lines().next().unwrap_or("").to_string();
                let auth = header_text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                    .map(|l| l["authorization:".len()..].trim().to_string());
                let body = buf[he..].to_vec();
                let _ = tx.send((request_line, auth, body));

                let resp = format!(
                    "{status_line}\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{response_body}",
                    response_body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        (url, rx)
    }

    #[test]
    fn put_blob_sends_auth_header_and_parses_descriptor() {
        let (url, rx) = spawn_mock(
            "HTTP/1.1 200 OK",
            r#"{"url":"https://b.example/x.bin","sha256":"deadbeef","size":5,"type":"application/octet-stream","uploaded":1733356800}"#,
        );
        let descriptor = put_blob(
            &url,
            "Nostr dGVzdA==",
            "application/octet-stream",
            b"hello".to_vec(),
        )
        .expect("2xx → descriptor");
        assert_eq!(descriptor.sha256, "deadbeef");
        assert_eq!(descriptor.size, 5);

        let (request_line, auth, body) = rx.recv().unwrap();
        assert!(
            request_line.starts_with("PUT "),
            "method is PUT: {request_line}"
        );
        assert!(
            request_line.contains("/upload"),
            "path is /upload: {request_line}"
        );
        assert_eq!(
            auth.as_deref(),
            Some("Nostr dGVzdA=="),
            "auth header forwarded"
        );
        assert_eq!(body, b"hello", "blob body streamed as the request body");
    }

    #[test]
    fn put_blob_maps_non_2xx_to_error_with_status() {
        let (url, _rx) = spawn_mock("HTTP/1.1 413 Payload Too Large", r#"{"message":"too big"}"#);
        let err = put_blob(&url, "Nostr x", "image/png", vec![0u8; 8])
            .expect_err("413 must map to an error");
        assert!(err.contains("413"), "error must carry the status: {err}");
    }
}
