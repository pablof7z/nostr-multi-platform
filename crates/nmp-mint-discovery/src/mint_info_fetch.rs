//! Optional NUT-06 `/v1/info` pull fetch (`mint-info` feature).
//!
//! `nmp-mint-discovery` surfaces NIP-87-discovered mints (kind:38172
//! announcements), but for a **discovered, not-yet-held** mint the
//! announcement carries only a `name` — a mint's canonical identity
//! (name/icon/units/nuts) lives in its own NUT-06 `/v1/info` endpoint, which
//! the announcement never echoes. `nmp-nip60`/`nmp-wallet` already fetch
//! `/v1/info` for mints the account actually holds (#3030), but this crate
//! must not depend on the wallet crate (see `docs/architecture/`'s
//! crate-boundary rule: discovery is reusable Nostr infrastructure, the
//! wallet is app/product-domain). Per product-owner decision, this crate
//! grows its OWN thin NUT-06 retrieval instead — a pull API, not a
//! dependency on `nmp-nip60`. Some duplication with `nmp-nip60`'s own
//! `/v1/info` fetch (`crates/nmp-nip60/src/cashu/http/info.rs`) is expected
//! and explicitly accepted.
//!
//! # The hot-path boundary (doctrine, not a suggestion)
//!
//! [`fetch_mint_info`] performs a real HTTP GET (via `reqwest`). Per the
//! projections-and-emission doctrine, a registered snapshot-projection
//! closure runs on the actor thread inside `make_update` and MUST be
//! non-blocking — no I/O, no await (D8). **`fetch_mint_info` must never be
//! called from inside the closure passed to
//! `SnapshotProjectionRegistrar::register_typed_snapshot_projection`** (see
//! `register.rs` / `runtime.rs`), exactly like `audit::enrich_with_audit`.
//! It is a composition-root-owned pull API: the caller says "give me NUT-06
//! info for mint URL x" on its own schedule (e.g. when the user opens a
//! discovered mint's detail view), and feeds the result into whatever
//! `DiscoveredMint`-shaped view it owns off the reactive path. This crate's
//! own `MintDiscoveryStore`/`MintDiscoveryRuntime` never call it, and the
//! reactive discovery projection/aggregate are unchanged by this module.

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;

/// Network timeout for a single `/v1/info` request. Discovery enrichment is
/// best-effort — a slow or hanging mint must not stall the caller
/// indefinitely.
const MINT_INFO_TIMEOUT: Duration = Duration::from_secs(8);

/// Identifies this crate to mints it queries.
const USER_AGENT: &str = concat!("nmp-mint-discovery/", env!("CARGO_PKG_VERSION"));

/// Upper bound on a `/v1/info` response body buffered in memory. A hostile
/// or misbehaving mint streaming an unbounded body must not be able to
/// exhaust the caller's memory (mirrors `nmp-nip60`'s
/// `MAX_MINT_RESPONSE_BYTES` posture for the same reason).
const MAX_INFO_RESPONSE_BYTES: usize = 1 << 20; // 1 MiB

/// Thin, best-effort NUT-06 mint metadata — enough to backfill a discovered
/// mint's display identity (name/icon/description) plus a coarse capability
/// summary (units/nuts). Deliberately a small local type rather than
/// `nmp-nip60`'s wallet-side `MintInfoResponse` or `cashu::MintInfo`'s full
/// typed `Nuts` breakdown — this crate only needs a best-effort read, not
/// the wallet's fee-bearing keyset/method detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MintNut06Info {
    /// The mint URL this info was fetched for (as passed to
    /// [`fetch_mint_info`], not re-normalized beyond trailing-slash
    /// trimming for the request itself).
    pub url: String,
    /// The mint's advertised name, when present.
    pub name: Option<String>,
    /// The mint's advertised icon URL, when present — the field
    /// `DiscoveredMint::icon_url` exists to hold (NIP-87 announcements
    /// never carry one).
    pub icon_url: Option<String>,
    /// The mint's short description, when present.
    pub description: Option<String>,
    /// Currency units mentioned anywhere inside the mint's `nuts` object
    /// (e.g. `"sat"`, `"msat"`). Best-effort and order-stable (sorted,
    /// deduplicated) — NUT-06 does not bound the unit string to a fixed
    /// enum, and nests it at varying depths across NUT method arrays.
    pub units: Vec<String>,
    /// NUT numbers the mint's `nuts` object has a top-level entry for,
    /// sorted ascending and deduplicated. Presence is a coarse capability
    /// signal, not an enabled/disabled verdict — some NUTs additionally
    /// nest a `disabled` flag this type does not interpret.
    pub nuts: Vec<u16>,
}

/// Errors from [`fetch_mint_info`]. Every variant is caller-facing —
/// discovery enrichment is best-effort, so a typical caller logs and skips
/// the mint rather than propagating.
#[derive(Debug)]
pub enum MintInfoError {
    /// `mint_url` was empty (or empty after trimming a trailing slash).
    InvalidUrl(String),
    /// The HTTP request itself failed (DNS, connect, TLS, timeout, ...).
    Request(String),
    /// The mint answered with a non-2xx status.
    HttpStatus { url: String, status: u16 },
    /// The response body exceeded [`MAX_INFO_RESPONSE_BYTES`].
    ResponseTooLarge { url: String, limit: usize },
    /// The response body was not valid JSON, or not a JSON object.
    Parse { url: String, message: String },
}

impl std::fmt::Display for MintInfoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(url) => write!(f, "invalid mint url: {url:?}"),
            Self::Request(message) => write!(f, "mint-info request failed: {message}"),
            Self::HttpStatus { url, status } => {
                write!(f, "mint {url:?} returned HTTP {status} for /v1/info")
            }
            Self::ResponseTooLarge { url, limit } => {
                write!(f, "mint {url:?} /v1/info response exceeded {limit} bytes")
            }
            Self::Parse { url, message } => {
                write!(f, "failed to parse /v1/info body from {url:?}: {message}")
            }
        }
    }
}

impl std::error::Error for MintInfoError {}

/// Fetch a mint's NUT-06 `/v1/info` document and parse it into
/// [`MintNut06Info`]. Pure pull API — the caller (composition root) invokes
/// this on demand per discovered mint URL, on its own schedule, exactly
/// like `audit::enrich_with_audit`. Never call this from inside a
/// registered snapshot-projection closure (D8 — see the module-level
/// "hot-path boundary" doc).
///
/// Graceful by construction: an unreachable mint, non-2xx status, oversized
/// body, or malformed JSON all yield `Err`, never a panic.
pub async fn fetch_mint_info(mint_url: &str) -> Result<MintNut06Info, MintInfoError> {
    let trimmed = mint_url.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(MintInfoError::InvalidUrl(mint_url.to_string()));
    }
    let info_url = format!("{trimmed}/v1/info");

    let client = reqwest::Client::builder()
        .timeout(MINT_INFO_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| MintInfoError::Request(e.to_string()))?;

    let mut response = client
        .get(&info_url)
        .send()
        .await
        .map_err(|e| MintInfoError::Request(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(MintInfoError::HttpStatus {
            url: info_url,
            status: status.as_u16(),
        });
    }

    // Stream the body chunk-by-chunk and abort as soon as it would exceed
    // MAX_INFO_RESPONSE_BYTES — never `response.bytes().await`, which would
    // buffer the ENTIRE body first and only bound it afterward (a hostile
    // mint could stream GBs within the timeout and exhaust memory before any
    // check fired). This mirrors `nmp-nip60`'s `read_bounded` posture: the
    // buffer is capped regardless of what the mint sends.
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| MintInfoError::Request(e.to_string()))?
    {
        push_bounded(&info_url, &mut body, &chunk, MAX_INFO_RESPONSE_BYTES)?;
    }

    parse_mint_info(mint_url, &body)
}

/// Append `chunk` to `body`, but fail with [`MintInfoError::ResponseTooLarge`]
/// if doing so would push the buffer past `limit` — the bounded-read guard
/// [`fetch_mint_info`] applies per streamed chunk. Pure (no I/O) so the
/// oversized-body path is unit-testable without a network mint, mirroring
/// `nmp-nip60`'s `read_bounded`.
fn push_bounded(
    url: &str,
    body: &mut Vec<u8>,
    chunk: &[u8],
    limit: usize,
) -> Result<(), MintInfoError> {
    if body.len() + chunk.len() > limit {
        return Err(MintInfoError::ResponseTooLarge {
            url: url.to_string(),
            limit,
        });
    }
    body.extend_from_slice(chunk);
    Ok(())
}

/// Pure parse: raw `/v1/info` JSON bytes → [`MintNut06Info`]. Split out from
/// [`fetch_mint_info`] so tests can exercise parsing (and URL building)
/// without real network I/O.
fn parse_mint_info(mint_url: &str, body: &[u8]) -> Result<MintNut06Info, MintInfoError> {
    let value: Value = serde_json::from_slice(body).map_err(|e| MintInfoError::Parse {
        url: mint_url.to_string(),
        message: e.to_string(),
    })?;

    if !value.is_object() {
        return Err(MintInfoError::Parse {
            url: mint_url.to_string(),
            message: "top-level /v1/info body is not a JSON object".to_string(),
        });
    }

    let name = value.get("name").and_then(Value::as_str).map(String::from);
    let icon_url = value
        .get("icon_url")
        .and_then(Value::as_str)
        .map(String::from);
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(String::from);

    let mut nuts: Vec<u16> = Vec::new();
    let mut units = BTreeSet::new();
    if let Some(nuts_obj) = value.get("nuts").and_then(Value::as_object) {
        for (key, nut_value) in nuts_obj {
            if let Ok(n) = key.parse::<u16>() {
                nuts.push(n);
            }
            collect_units(nut_value, &mut units);
        }
    }
    nuts.sort_unstable();
    nuts.dedup();

    Ok(MintNut06Info {
        url: mint_url.to_string(),
        name,
        icon_url,
        description,
        units: units.into_iter().collect(),
        nuts,
    })
}

/// Recursively collect every string value found under a `"unit"` key,
/// anywhere inside `value`. NUT-06 nests unit strings inside per-NUT method
/// arrays at varying depths across NUTs (e.g. `nuts."4".methods[].unit`),
/// so this walks the whole sub-tree under a `nuts."<n>"` entry rather than
/// assuming one fixed shape.
fn collect_units(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if key == "unit" {
                    if let Some(unit) = v.as_str() {
                        out.insert(unit.to_string());
                    }
                } else {
                    collect_units(v, out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_units(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REALISTIC_INFO_JSON: &str = r#"{
        "name": "Test Mint",
        "pubkey": "02deadbeef",
        "version": "Nutshell/0.16.0",
        "description": "A friendly test mint",
        "icon_url": "https://mint.example/icon.png",
        "nuts": {
            "4": {
                "methods": [
                    {"method": "bolt11", "unit": "sat", "min_amount": 1, "max_amount": 1000000}
                ],
                "disabled": false
            },
            "5": {
                "methods": [
                    {"method": "bolt11", "unit": "sat"}
                ],
                "disabled": false
            },
            "7": {"supported": true},
            "17": {
                "supported": [
                    {"method": "bolt11", "unit": "sat", "commands": ["bolt11_mint_quote"]}
                ]
            }
        }
    }"#;

    #[test]
    fn parses_a_realistic_v1_info_body() {
        let info = parse_mint_info("https://mint.example", REALISTIC_INFO_JSON.as_bytes())
            .expect("parses");
        assert_eq!(info.url, "https://mint.example");
        assert_eq!(info.name.as_deref(), Some("Test Mint"));
        assert_eq!(
            info.icon_url.as_deref(),
            Some("https://mint.example/icon.png")
        );
        assert_eq!(info.description.as_deref(), Some("A friendly test mint"));
        assert_eq!(info.units, vec!["sat".to_string()]);
        assert_eq!(info.nuts, vec![4, 5, 7, 17]);
    }

    #[test]
    fn missing_optional_fields_default_to_none_and_empty() {
        let info = parse_mint_info(
            "https://mint.example",
            br#"{"pubkey":"02deadbeef","nuts":{}}"#,
        )
        .expect("parses");
        assert_eq!(info.name, None);
        assert_eq!(info.icon_url, None);
        assert_eq!(info.description, None);
        assert!(info.units.is_empty());
        assert!(info.nuts.is_empty());
    }

    #[test]
    fn malformed_json_yields_err_not_panic() {
        let err = parse_mint_info("https://mint.example", b"not json").unwrap_err();
        assert!(matches!(err, MintInfoError::Parse { .. }));
    }

    #[test]
    fn empty_body_yields_err_not_panic() {
        let err = parse_mint_info("https://mint.example", b"").unwrap_err();
        assert!(matches!(err, MintInfoError::Parse { .. }));
    }

    #[test]
    fn non_object_json_yields_err_not_panic() {
        let err = parse_mint_info("https://mint.example", b"[1,2,3]").unwrap_err();
        assert!(matches!(err, MintInfoError::Parse { .. }));
    }

    #[test]
    fn push_bounded_accepts_chunks_up_to_the_limit() {
        let mut body = Vec::new();
        push_bounded("https://mint.example", &mut body, b"1234", 8).expect("within limit");
        push_bounded("https://mint.example", &mut body, b"5678", 8).expect("exactly at limit");
        assert_eq!(body, b"12345678");
    }

    #[test]
    fn push_bounded_rejects_a_chunk_that_would_exceed_the_limit() {
        // Mirrors the streamed-body guard in `fetch_mint_info`: the buffer is
        // capped regardless of what the mint sends, so a body over the cap
        // yields `ResponseTooLarge` (and never over-buffers).
        let mut body = Vec::new();
        push_bounded("https://mint.example", &mut body, b"1234", 8).expect("first chunk fits");
        let err = push_bounded("https://mint.example", &mut body, b"56789", 8)
            .expect_err("second chunk overflows the cap");
        assert!(matches!(
            err,
            MintInfoError::ResponseTooLarge { limit: 8, .. }
        ));
        // The overflowing chunk was NOT appended — the buffer stays bounded.
        assert_eq!(body, b"1234");
    }

    #[test]
    fn info_url_join_trims_a_trailing_slash_mint_url() {
        // Exercises the same trim-then-append logic `fetch_mint_info` uses,
        // without needing network I/O to prove the URL shape.
        let trimmed = "https://mint.example/".trim_end_matches('/');
        assert_eq!(format!("{trimmed}/v1/info"), "https://mint.example/v1/info");
    }

    #[test]
    fn info_url_join_handles_a_bare_mint_url_with_no_trailing_slash() {
        let trimmed = "https://mint.example".trim_end_matches('/');
        assert_eq!(format!("{trimmed}/v1/info"), "https://mint.example/v1/info");
    }

    /// Live network example — not run in CI. Run manually with:
    /// `cargo test -p nmp-mint-discovery --features mint-info -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn fetch_mint_info_against_a_real_mint() {
        let info = fetch_mint_info("https://testnut.cashu.space")
            .await
            .expect("real mint should answer /v1/info");
        assert!(info.name.is_some());
    }
}
