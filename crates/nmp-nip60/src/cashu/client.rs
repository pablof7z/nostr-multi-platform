//! Native HTTP transport for the Cashu mint API (NUT-01 through NUT-12).
//!
//! `native`-feature-only (uses `ureq`), matching the pattern used by
//! `nmp-nip57`'s LNURL fetcher. All calls are synchronous — the caller is
//! responsible for spawning these on a worker thread (D8 — never block the
//! actor loop); see `nmp_nip57::lnurl::spawn_lnurl_worker` for the
//! established pattern a future `nmp-wallet` mint-HTTP worker should mirror.
//!
//! # This is the transport, not the protocol
//!
//! Every method here follows the same three-step shape: build a
//! [`super::http::MintHttpRequest`] (pure, no I/O), `roundtrip` it over
//! `ureq`, then hand the raw bytes to a pure `parse_*`/`finalize_*`
//! validator in [`super::http`]. Request construction and response
//! validation live entirely in `http.rs` so a browser transport (which
//! cannot use `ureq` — it must route through the host's `fetch()` via
//! `nmp_core::substrate::OutboundHttpCapability`) can reuse the exact same
//! validation this crate's tests exercise. `MintClient` itself owns nothing
//! but the mint URL and a `secp256k1` context.

use std::io::Read as _;
use std::time::Duration;

use nostr::secp256k1::{All, Secp256k1};
use tracing::debug;

use super::http::{self, DleqPolicy, MintHttpMethod, MintHttpRequest, MintRawResponse};
use super::types::*;
use crate::error::Nip60Error;

/// Upper bound on a mint response body we'll buffer in memory. A misbehaving
/// or hostile mint streaming an unbounded body must not be able to exhaust
/// the caller's memory.
const MAX_MINT_RESPONSE_BYTES: u64 = 1 << 20; // 1 MiB

/// How many times [`MintClient::roundtrip`] retries a request after an HTTP
/// 429 ("Rate limit exceeded") before giving up and surfacing the mint's
/// rate-limit response as a terminal error (#2968 — a real
/// `mint.minibits.cash` deposit run hit 429 on `GET
/// /v1/mint/quote/bolt11/{id}` status polling and, before this fix, never
/// settled because the 429 surfaced as a hard failure a caller gave up on
/// instead of a retry/backoff). Bounded so a mint that rate-limits forever
/// cannot wedge the calling worker thread indefinitely.
const MAX_RATE_LIMIT_RETRIES: u32 = 5;

/// Backoff used on a 429 response that carries no (or an unparsable)
/// `Retry-After` header.
const DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(2);

/// Upper bound on a single 429 backoff, even if the mint sends an absurdly
/// large `Retry-After` value — bounds a hostile/misconfigured mint's ability
/// to wedge the calling worker thread on one retry.
const MAX_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(30);

/// Parse a `Retry-After` header value into a backoff duration.
///
/// Only the integer-seconds form (RFC 9110 §10.2.3) is honored — every mint
/// we've observed in the wild sends this form, and the alternative
/// HTTP-date form needs a date parser this crate has no other use for.
/// Falls back to [`DEFAULT_RATE_LIMIT_BACKOFF`] when the header is absent or
/// not a plain integer, and always caps at [`MAX_RATE_LIMIT_BACKOFF`]. Pure
/// (no I/O, no sleep) so a test can exercise every branch without actually
/// waiting on a clock.
fn retry_after_duration(header: Option<&str>) -> Duration {
    header
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_RATE_LIMIT_BACKOFF)
        .min(MAX_RATE_LIMIT_BACKOFF)
}

/// Cashu mint HTTP client.
///
/// Holds the mint URL and a `secp256k1` context. Constructed once per mint
/// URL and reused across operations.
pub struct MintClient {
    mint_url: String,
    secp: Secp256k1<All>,
}

impl MintClient {
    /// Create a new client for the given mint URL.
    pub fn new(mint_url: impl Into<String>) -> Self {
        Self {
            mint_url: mint_url.into().trim_end_matches('/').to_string(),
            secp: Secp256k1::new(),
        }
    }

    /// Execute one [`MintHttpRequest`] against this client's mint and return
    /// the raw, unvalidated response. Only `operation`/`method` are ever
    /// logged — never the URL, path, or body, both of which routinely carry
    /// a mint quote id or proof secret (see `http::MintHttpRequest`'s
    /// redacted `Debug`).
    ///
    /// A `429` ("Rate limit exceeded") is retried with backoff (honoring the
    /// mint's `Retry-After` header when present — see
    /// [`retry_after_duration`]) up to [`MAX_RATE_LIMIT_RETRIES`] times
    /// rather than surfaced immediately as a terminal error: every mint
    /// operation this crate makes (including quote-status polling, #2968's
    /// real-world trigger) shares this one transport chokepoint, so handling
    /// it here covers all of them without duplicating the retry loop per
    /// call site. This sleep happens on the caller's worker thread, never
    /// the actor thread (see module docs / D8), so it does not violate the
    /// no-polling-in-library-code rule the same way a sleep+recheck loop in
    /// `nip60_wallet::deposit::complete_deposit` would.
    fn roundtrip(&self, req: &MintHttpRequest) -> Result<MintRawResponse, Nip60Error> {
        let url = format!("{}{}", self.mint_url, req.path);
        log_request(req);

        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let response = match req.method {
                MintHttpMethod::Get => ureq::get(&url).call(),
                MintHttpMethod::Post => ureq::post(&url)
                    .set("Content-Type", "application/json")
                    .send_bytes(&req.body),
            };

            match response {
                Ok(resp) => {
                    let status_code = resp.status();
                    let body = read_bounded(resp.into_reader())?;
                    return Ok(MintRawResponse { status_code, body });
                }
                Err(ureq::Error::Status(429, resp)) if attempt < MAX_RATE_LIMIT_RETRIES => {
                    let backoff = retry_after_duration(resp.header("Retry-After"));
                    debug!(
                        operation = ?req.operation,
                        attempt,
                        backoff_ms = backoff.as_millis() as u64,
                        "mint rate-limited (429); backing off and retrying"
                    );
                    // A bounded (<= MAX_RATE_LIMIT_RETRIES), single-shot HTTP
                    // retry backoff on the caller-owned worker thread this
                    // whole client is documented to require (module docs —
                    // never the actor thread), not a sleep+check polling loop
                    // against actor/kernel state.
                    std::thread::sleep(backoff); // doctrine-allow: D8 — bounded mint-HTTP 429 retry backoff on the caller's worker thread, not an actor-loop poll
                    continue;
                }
                Err(ureq::Error::Status(status_code, resp)) => {
                    // A non-2xx response still carries a body the caller's
                    // `parse_*`/`finalize_*` validator wants to inspect
                    // (Cashu mints return `{"code":N,"detail":"..."}` on
                    // protocol errors) — read it the same bounded way as a
                    // success. This is also where a 429 lands once
                    // `MAX_RATE_LIMIT_RETRIES` is exhausted: still not
                    // silently swallowed, just no longer retried.
                    let body = read_bounded(resp.into_reader())?;
                    return Ok(MintRawResponse { status_code, body });
                }
                Err(ureq::Error::Transport(t)) => {
                    // `Transport`'s own `Display` embeds the failed URL
                    // (which routinely carries a quote id) — use `.kind()`
                    // instead, whose `Display` is a fixed, url-free string
                    // ("Dns Failed", "Connection Failed", ...).
                    return Err(Nip60Error::MintHttp(format!(
                        "transport error for {:?} {:?}: {}",
                        req.operation,
                        req.method,
                        t.kind()
                    )));
                }
            }
        }
        unreachable!(
            "the loop always returns on its last iteration (attempt == MAX_RATE_LIMIT_RETRIES \
             never satisfies the retry guard, so every remaining match arm returns)"
        )
    }

    // ─── Keyset ────────────────────────────────────────────────────────────

    /// Fetch the mint's active keysets.
    pub fn get_keys(&self) -> Result<KeysResponse, Nip60Error> {
        let raw = self.roundtrip(&http::build_get_keys_request())?;
        http::parse_keys_response(&raw)
    }

    /// Fetch the active sat-unit keyset (with fee info from `/v1/keysets` merged in).
    pub fn get_sat_keyset(&self) -> Result<KeySet, Nip60Error> {
        // /v1/keys has the denomination→pubkey map; /v1/keysets has input_fee_ppk.
        let keys_resp = self.get_keys()?;
        let mut keyset = keys_resp
            .keysets
            .into_iter()
            .find(|ks| ks.unit == "sat")
            .ok_or_else(|| Nip60Error::MintProtocol("no sat keyset found".into()))?;

        // Merge in fee info from /v1/keysets (best-effort; ignore errors).
        let keysets_raw = self.roundtrip(&http::build_get_keysets_request());
        if let Ok(keysets_resp) = keysets_raw.and_then(|raw| http::parse_keys_response(&raw)) {
            if let Some(ks_meta) = keysets_resp
                .keysets
                .into_iter()
                .find(|ks| ks.id == keyset.id)
            {
                keyset.input_fee_ppk = ks_meta.input_fee_ppk;
            }
        }
        Ok(keyset)
    }

    /// Compute the swap fee for `n` inputs given `input_fee_ppk` (parts per thousand).
    ///
    /// Per NUT-02: `fee = ceil(n * input_fee_ppk / 1000)`. `input_fee_ppk`
    /// comes from the mint's `/v1/keysets` response — saturate rather than
    /// panic/wrap if a mint ever advertises a value large enough to
    /// overflow `u64` when multiplied by the input count.
    pub fn compute_fee(n_inputs: u64, input_fee_ppk: u64) -> u64 {
        n_inputs.saturating_mul(input_fee_ppk).div_ceil(1000)
    }

    // ─── Mint quote (NUT-04) ──────────────────────────────────────────────

    /// Request a bolt11 mint quote for the given amount in sats.
    pub fn create_mint_quote(&self, amount_sats: u64) -> Result<MintQuoteResponse, Nip60Error> {
        let req = http::build_mint_quote_bolt11_request(amount_sats, "sat")?;
        let raw = self.roundtrip(&req)?;
        http::parse_mint_quote_bolt11_response(
            &raw,
            http::MintQuoteExpectation {
                amount: Some(amount_sats),
                unit: Some("sat"),
                quote_id: None,
            },
        )
    }

    /// Poll the status of an existing mint quote.
    pub fn get_mint_quote_status(&self, quote_id: &str) -> Result<MintQuoteResponse, Nip60Error> {
        let req = http::build_get_mint_quote_bolt11_request(quote_id)?;
        let raw = self.roundtrip(&req)?;
        http::parse_mint_quote_bolt11_response(
            &raw,
            http::MintQuoteExpectation {
                quote_id: Some(quote_id),
                ..Default::default()
            },
        )
    }

    // ─── Mint tokens (NUT-04) ─────────────────────────────────────────────

    /// Mint tokens for a paid quote.
    ///
    /// Returns a list of `Proof`s whose amounts sum to `total_amount` using
    /// the standard 2^n denomination split.
    ///
    /// Each proof is verified with its DLEQ proof if the mint provides one
    /// (NUT-12). Returns an error if any DLEQ verification fails.
    pub fn mint_tokens(
        &self,
        quote_id: &str,
        total_amount: u64,
        keyset: &KeySet,
    ) -> Result<Vec<Proof>, Nip60Error> {
        let prepared =
            http::prepare_mint_bolt11_request(quote_id, total_amount, keyset, &self.secp)?;
        let raw = self.roundtrip(&prepared.http)?;
        http::finalize_mint_bolt11_response(
            &prepared,
            &raw,
            DleqPolicy::VerifyIfPresent,
            &self.secp,
        )
    }

    // ─── Swap (NUT-03) ────────────────────────────────────────────────────

    /// Swap proofs for new proofs, optionally with P2PK spending conditions.
    ///
    /// `new_secrets` — if `Some`, these are the secrets for the output proofs
    /// (used for P2PK where the secret is a spending condition JSON).
    /// If `None`, random secrets are generated.
    pub fn swap(
        &self,
        inputs: Vec<Proof>,
        output_amounts: Vec<u64>,
        output_secrets: Option<Vec<String>>,
        keyset: &KeySet,
    ) -> Result<Vec<Proof>, Nip60Error> {
        let prepared =
            http::prepare_swap_request(inputs, output_amounts, output_secrets, keyset, &self.secp)?;
        let raw = self.roundtrip(&prepared.http)?;
        http::finalize_swap_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &self.secp)
    }

    // ─── Proof state check (NUT-07) ───────────────────────────────────────

    /// Check which of the given proof secrets are still unspent.
    pub fn check_state(&self, secrets: &[String]) -> Result<Vec<ProofState>, Nip60Error> {
        let (req, expected_ys) = http::build_check_state_request(secrets)?;
        let raw = self.roundtrip(&req)?;
        http::parse_check_state_response(&raw, &expected_ys)
    }
}

/// Log an outgoing mint HTTP request. A dedicated function (rather than an
/// inline `debug!` in `roundtrip`) so a unit test can exercise the log
/// content without needing a live network round-trip: `operation`/`method`
/// only, never the URL, path, or body (see `MintHttpRequest`'s redacted
/// `Debug`, which this call deliberately does not use — logging the whole
/// struct even redacted would still print its `Debug` impl's field names,
/// this keeps the log line minimal by construction).
fn log_request(req: &MintHttpRequest) {
    debug!(operation = ?req.operation, method = ?req.method, "mint http request");
}

/// Read `reader` into memory, refusing to buffer more than
/// [`MAX_MINT_RESPONSE_BYTES`]. Reads one byte past the limit to detect
/// truncation and errors instead of silently handing a truncated (and
/// therefore likely still-"valid-looking" JSON prefix of a) body to a
/// `parse_*`/`finalize_*` validator downstream.
fn read_bounded(reader: impl std::io::Read) -> Result<Vec<u8>, Nip60Error> {
    let mut body = Vec::new();
    reader
        .take(MAX_MINT_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|e| Nip60Error::MintHttp(format!("read response body: {e}")))?;
    if body.len() as u64 > MAX_MINT_RESPONSE_BYTES {
        return Err(Nip60Error::MintHttp(format!(
            "mint response body exceeded the {MAX_MINT_RESPONSE_BYTES}-byte limit"
        )));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
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
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
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
}
