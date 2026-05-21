//! Tests for [`crate::lnurl`].
//!
//! Lives in a `_tests.rs` file so the D6 doctrine linter exempts the
//! `.expect()/.unwrap()/panic!` calls a hand-rolled HTTP test fixture
//! relies on — see `crates/nmp-testing/bin/doctrine-lint/rules/d6.rs`.

use crate::lnurl::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

/// A hand-rolled HTTP/1.1 server that returns scripted responses. Lets
/// the LNURL tests assert the exact wire shape (NIP-57 metadata,
/// `amount`/`nostr` query params, bolt11 in `pr`) without pulling in an
/// HTTP mock crate (none in the workspace).
///
/// The server accepts a fixed sequence of `(path_prefix, response_body)`
/// pairs in `script` order and shuts down after serving them all.
struct CannedServer {
    addr: String,
    captured: Arc<Mutex<Vec<String>>>,
}

impl CannedServer {
    fn start(script: Vec<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr").to_string();
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        thread::spawn(move || {
            for body in script {
                let (mut stream, _) = match listener.accept() {
                    Ok(p) => p,
                    Err(_) => return,
                };
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                captured_clone.lock().unwrap().push(req);
                let response = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self { addr, captured }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn captured_request(&self, idx: usize) -> Option<String> {
        self.captured.lock().unwrap().get(idx).cloned()
    }
}

fn well_formed_metadata(callback: &str) -> String {
    format!(
        r#"{{
            "callback":"{callback}",
            "minSendable":1000,
            "maxSendable":1000000000,
            "allowsNostr":true,
            "nostrPubkey":"abcd0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab",
            "metadata":"[[\"text/plain\",\"zap alice\"]]"
        }}"#
    )
}

#[test]
fn empty_lnurl_rejected() {
    let err = fetch_invoice("", "{}", 1000).unwrap_err();
    assert!(matches!(err, LnurlError::Invalid(_)), "got: {err:?}");
}

#[test]
fn bech32_lnurl_rejected_in_this_build() {
    let err =
        fetch_invoice("lnurl1dp68gurn8ghj7etcv9khqcm9", "{}", 1000).unwrap_err();
    assert!(matches!(err, LnurlError::Invalid(_)), "got: {err:?}");
}

#[test]
fn fetches_invoice_end_to_end() {
    // Start a server that serves the metadata first, then the callback
    // response (we'll point the metadata's `callback` at the SAME server,
    // so both hits go to one socket; the script feeds them in order).
    // We bind once but the addr is the same for both requests; the
    // metadata's `callback` field points back to the same host:port with
    // a distinct path so the server returns the invoice on the second hit.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr").to_string();
    let callback_url = format!("http://{addr}/cb");
    let metadata_body = well_formed_metadata(&callback_url);
    let invoice_body =
        r#"{"pr":"lnbc100n1pj…fake_invoice_payload","routes":[]}"#.to_string();

    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);
    let server_handle = thread::spawn(move || {
        for body in [metadata_body, invoice_body].iter() {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = vec![0u8; 16 * 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            captured_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let signed_event = r#"{"id":"e1","pubkey":"a1","sig":"deadbeef","kind":9734,"created_at":1,"tags":[["amount","21000"]],"content":""}"#;
    let result =
        fetch_invoice(&format!("http://{addr}/.well-known/lnurlp/alice"), signed_event, 21_000)
            .expect("LNURL fetch should succeed");

    assert!(result.invoice.starts_with("lnbc"), "got invoice: {}", result.invoice);
    assert_eq!(
        result.nostr_pubkey,
        "abcd0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab"
    );

    server_handle.join().expect("server thread");
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 2, "expected exactly 2 requests");

    // Second request must carry the amount AND nostr query params on the
    // callback URL — that's the NIP-57 contract.
    let callback_req = &captured[1];
    assert!(
        callback_req.contains("amount=21000"),
        "callback missing amount=21000: {callback_req}"
    );
    // `nostr` is URL-encoded by ureq; the leading `{` becomes %7B.
    assert!(
        callback_req.contains("nostr=%7B"),
        "callback missing url-encoded `nostr` query: {callback_req}"
    );
}

#[test]
fn rejects_endpoint_without_allows_nostr() {
    let server = CannedServer::start(vec![
        r#"{"callback":"http://ignored","minSendable":1,"maxSendable":1000,"allowsNostr":false,"nostrPubkey":"a"}"#,
    ]);
    let err = fetch_invoice(&server.url("/lnurlp/bob"), "{}", 100).unwrap_err();
    assert!(matches!(err, LnurlError::InvalidMetadata(_)), "got: {err:?}");
    assert!(server.captured_request(0).is_some());
}

#[test]
fn rejects_endpoint_without_nostr_pubkey() {
    let server = CannedServer::start(vec![
        r#"{"callback":"http://ignored","minSendable":1,"maxSendable":1000,"allowsNostr":true}"#,
    ]);
    let err = fetch_invoice(&server.url("/lnurlp/bob"), "{}", 100).unwrap_err();
    assert!(matches!(err, LnurlError::InvalidMetadata(_)), "got: {err:?}");
}

#[test]
fn rejects_amount_out_of_range() {
    // minSendable: 1000 msats; maxSendable: 5000 msats. Request 6000 →
    // AmountOutOfRange.
    let server = CannedServer::start(vec![
        r#"{"callback":"http://ignored","minSendable":1000,"maxSendable":5000,"allowsNostr":true,"nostrPubkey":"a"}"#,
    ]);
    let err = fetch_invoice(&server.url("/lnurlp/bob"), "{}", 6_000).unwrap_err();
    match err {
        LnurlError::AmountOutOfRange { min_msats, max_msats, requested_msats } => {
            assert_eq!(min_msats, 1000);
            assert_eq!(max_msats, 5000);
            assert_eq!(requested_msats, 6000);
        }
        other => panic!("expected AmountOutOfRange, got: {other:?}"),
    }
}

#[test]
fn surfaces_endpoint_error_envelope() {
    let server = CannedServer::start(vec![
        r#"{"status":"ERROR","reason":"recipient not found"}"#,
    ]);
    let err = fetch_invoice(&server.url("/lnurlp/missing"), "{}", 100).unwrap_err();
    assert!(matches!(err, LnurlError::InvalidMetadata(_)), "got: {err:?}");
    // The surface contains the upstream reason.
    let msg = err.to_string();
    assert!(msg.contains("recipient not found"), "got: {msg}");
}

#[test]
fn surfaces_callback_invoice_missing() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr").to_string();
    let callback_url = format!("http://{addr}/cb");
    let metadata_body = well_formed_metadata(&callback_url);
    let invoice_body = r#"{"routes":[]}"#.to_string();

    thread::spawn(move || {
        for body in [metadata_body, invoice_body].iter() {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = vec![0u8; 16 * 1024];
            let _ = stream.read(&mut buf).unwrap_or(0);
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let err = fetch_invoice(&format!("http://{addr}/lnurlp/x"), "{}", 1000).unwrap_err();
    assert!(matches!(err, LnurlError::InvoiceMissing(_)), "got: {err:?}");
}
