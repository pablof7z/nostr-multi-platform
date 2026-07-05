//! Split out of `client.rs` (AGENTS.md file-size discipline) — the native
//! `ureq` transport's tests, including the 429-retry/backoff coverage for
//! #2968 and the redacted-`Debug`/log-content coverage for the transport
//! itself.

use super::*;
use std::io::Write;
use std::net::TcpListener;
use std::thread;

#[test]
fn retry_after_duration_honors_integer_seconds_header() {
    assert_eq!(retry_after_duration(Some("3")), Duration::from_secs(3));
}

#[test]
fn retry_after_duration_falls_back_to_default_when_absent() {
    assert_eq!(retry_after_duration(None), DEFAULT_RATE_LIMIT_BACKOFF);
}

#[test]
fn retry_after_duration_falls_back_to_default_when_unparsable() {
    // HTTP-date form ("Wed, 21 Oct 2015 07:28:00 GMT") is valid per
    // RFC 9110 but this crate doesn't parse it (see doc comment) — must
    // fall back rather than panic or hang forever.
    assert_eq!(
        retry_after_duration(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        DEFAULT_RATE_LIMIT_BACKOFF
    );
}

#[test]
fn retry_after_duration_caps_an_absurdly_large_header() {
    assert_eq!(retry_after_duration(Some("999999")), MAX_RATE_LIMIT_BACKOFF);
}

/// Minimal local HTTP/1.1 mock that serves a fixed sequence of responses
/// across successive connections. Every response sends `Connection:
/// close` so `roundtrip`'s retry opens a fresh connection per attempt,
/// mirroring a real mint's fresh accept per request rather than relying
/// on (or fighting) `ureq`'s connection pooling. Mirrors the
/// `nmp-blossom::upload::http` raw-socket mock pattern used for the same
/// kind of ureq-level test.
fn spawn_sequenced_mock(responses: Vec<(&'static str, &'static str, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    thread::spawn(move || {
        for (status_line, extra_header, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            // Drain the request (headers + any Content-Length body)
            // before writing a response.
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
                        let header_text = String::from_utf8_lossy(&buf[..pos]).to_string();
                        for line in header_text.lines() {
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

            let mut head = format!(
                "{status_line}\r\nConnection: close\r\nContent-Length: {}\r\n",
                body.len()
            );
            if !extra_header.is_empty() {
                head.push_str(extra_header);
                head.push_str("\r\n");
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
            let _ = stream.flush();
        }
    });
    url
}

#[test]
fn get_mint_quote_status_retries_after_429_then_succeeds() {
    // First connection: rate-limited, `Retry-After: 0` so the test
    // doesn't actually wait. Second connection: the mint quote finally
    // returns as PAID.
    let url = spawn_sequenced_mock(vec![
        (
            "HTTP/1.1 429 Too Many Requests",
            "Retry-After: 0",
            r#"{"detail":"Rate limit exceeded."}"#,
        ),
        (
            "HTTP/1.1 200 OK",
            "",
            r#"{"quote":"q1","request":"lnbc1","amount":10,"unit":"sat","state":"PAID"}"#,
        ),
    ]);
    let client = MintClient::new(&url);
    let resp = client
        .get_mint_quote_status("q1")
        .expect("a 429 must be retried, not surfaced as a terminal error");
    assert_eq!(resp.state, MintQuoteState::Paid);
}

#[test]
fn get_mint_quote_status_gives_up_after_max_retries_of_429() {
    // Every attempt (initial + MAX_RATE_LIMIT_RETRIES retries) is
    // rate-limited — proves the retry loop is bounded rather than
    // hanging forever against a mint that never stops rate-limiting.
    let responses: Vec<_> = (0..=MAX_RATE_LIMIT_RETRIES)
        .map(|_| {
            (
                "HTTP/1.1 429 Too Many Requests",
                "Retry-After: 0",
                r#"{"detail":"Rate limit exceeded."}"#,
            )
        })
        .collect();
    let url = spawn_sequenced_mock(responses);
    let client = MintClient::new(&url);
    let err = client
        .get_mint_quote_status("q1")
        .expect_err("exhausting the retry budget must still surface a terminal error");
    assert!(matches!(
        err,
        Nip60Error::MintProtocol(_) | Nip60Error::MintHttp(_)
    ));
}

/// `MintClient::get_keysets_with_fees` merges `input_fee_ppk` across EVERY
/// unit the mint advertises, not just `"sat"` — the generalization
/// `get_sat_keyset` now delegates to.
#[test]
fn get_keysets_with_fees_merges_fees_across_every_unit() {
    let url = spawn_sequenced_mock(vec![
        (
            "HTTP/1.1 200 OK",
            "",
            r#"{"keysets":[
                {"id":"00sat","unit":"sat","keys":{"1":"02aa"}},
                {"id":"00usd","unit":"usd","keys":{"1":"02bb"}}
            ]}"#,
        ),
        (
            "HTTP/1.1 200 OK",
            "",
            r#"{"keysets":[
                {"id":"00sat","unit":"sat","input_fee_ppk":100},
                {"id":"00usd","unit":"usd","input_fee_ppk":250}
            ]}"#,
        ),
    ]);
    let client = MintClient::new(&url);
    let keysets = client
        .get_keysets_with_fees()
        .expect("keysets with fees round-trip");
    assert_eq!(keysets.len(), 2);
    let sat = keysets.iter().find(|ks| ks.unit == "sat").unwrap();
    let usd = keysets.iter().find(|ks| ks.unit == "usd").unwrap();
    assert_eq!(sat.input_fee_ppk, 100);
    assert_eq!(usd.input_fee_ppk, 250);
}

/// `MintClient::get_mint_info` end-to-end against a local mock mint — proves
/// the whole roundtrip (request build -> `ureq` call -> response parse),
/// complementing `http::info`'s own builder-shape/decode-only unit tests.
#[test]
fn get_mint_info_parses_a_sample_v1_info_body() {
    let url = spawn_sequenced_mock(vec![(
        "HTTP/1.1 200 OK",
        "",
        r#"{"name":"Test Mint","pubkey":"02deadbeef","version":"Nutshell/0.15.0","icon_url":"https://mint.example/icon.png"}"#,
    )]);
    let client = MintClient::new(&url);
    let info = client.get_mint_info().expect("mint info round-trip");
    assert_eq!(info.name.as_deref(), Some("Test Mint"));
    assert_eq!(
        info.icon_url.as_deref(),
        Some("https://mint.example/icon.png")
    );
}

#[test]
fn split_amount_reexport_still_reachable() {
    // `split_amount` moved to `http.rs` (always-compiled, no `ureq`) so
    // a browser transport can reuse it; this guards the native-facing
    // re-export path (`crate::cashu::split_amount`) that
    // `nip60_wallet::nutzap_send`/`nutzap_receive` depend on.
    assert_eq!(crate::cashu::split_amount(3), vec![1, 2]);
}

/// Minimal capturing `tracing::Subscriber` — avoids pulling in
/// `tracing-subscriber` (not otherwise a dependency of this crate) just
/// to assert on log content.
struct CaptureSubscriber {
    buf: std::sync::Arc<std::sync::Mutex<String>>,
}

impl tracing::Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct LineVisitor<'a>(&'a mut String);
        impl tracing::field::Visit for LineVisitor<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!(" {}={value:?}", field.name()));
            }
        }
        let mut line = String::new();
        event.record(&mut LineVisitor(&mut line));
        let mut buf = self.buf.lock().unwrap();
        buf.push_str(&line);
        buf.push('\n');
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

/// D6/security — the only `debug!` in the native transport must name the
/// operation and method, never the mint URL, request path, or body (all
/// of which routinely carry a quote id or proof secret).
#[test]
fn mint_client_logs_operation_not_url_body_or_quote() {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let subscriber = CaptureSubscriber { buf: buf.clone() };

    // No live network round-trip — `log_request` is the exact call
    // `roundtrip` makes before touching the network, so this proves
    // what gets logged without depending on network access in CI.
    let req = http::build_get_mint_quote_bolt11_request("top-secret-quote-id").unwrap();
    tracing::subscriber::with_default(subscriber, || {
        log_request(&req);
    });

    let logged = buf.lock().unwrap().clone();
    assert!(logged.contains("mint http request"));
    assert!(logged.contains("GetMintQuoteBolt11"));
    assert!(!logged.contains("mint.example"));
    assert!(!logged.contains("super-secret-path"));
    assert!(!logged.contains("top-secret-quote-id"));
}
