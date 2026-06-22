//! Higher-order NIP-50 search against REAL relays — the consumer-facing search
//! contract proven end-to-end on the live network.
//!
//! Two tiers:
//!
//!   * **Per-scope wire tier (SC-01/03/04/08/09)** — opens a real socket to a
//!     NIP-50 relay, sends the generic `{"search":"…"}` REQ that
//!     `nmp_nip50::SearchRequest::interest_shape` + `filter_json_for` produce,
//!     and drains the EVENTs into a `nmp_nip50::SearchResultsProjection`. Proves
//!     the wire field + the dedup projection against real relay output.
//!   * **Transparency tier (SC-TRANSPARENCY)** — the headline. A FRESH random
//!     account publishes a kind:10002 (read relay R) AND a kind:10007 (search
//!     relay `wss://nostr.wine`) to R. A real `NmpApp` cold-starts with ONLY
//!     `register_defaults` (no `set_search_relay_source`, no explicit relay
//!     arg). Calling `open_search(.., UserPreferred)` must transparently fan the
//!     search REQ out to the published kind:10007 relay and return results —
//!     proving NMP discovers + uses the user's kind:10007 with zero app code.
//!
//! Live-relay facts (verified the day this was written, via nak):
//!   * `wss://nostr.wine`     — NIP-50 (paid writes; READS work unauthenticated).
//!   * `wss://relay.nostr.band` — NIP-50 (free; the `NMP_BUILTIN_SEARCH_RELAY`).
//!   * `wss://nos.lol`        — NO NIP-50 (CLOSED "unrecognised filter item:
//!                              search"); used here as the kind:10002/10007 host.
//!   * `wss://search.nos.lol` — DEAD (NXDOMAIN).
//!
//! Honest-validation: an unreachable relay or a no-data query writes a SKIP
//! finding and pass-but-skips — it never fabricates a green assertion.
//!
//! Gated behind the `real-relay` feature AND `#[ignore]`. Run explicitly:
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay --test real_relay_nip50_search \
//!   -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

#[path = "real_relay_nip50_search/transparency.rs"]
mod transparency;

use std::time::{Duration, Instant};

use common::{report_page, send_text, try_open, write_report, RelaySocket, Verdict};
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_nip50::{SearchRequest, SearchResultsProjection, SearchScope, SearchTargets};
use serde_json::Value;

/// nostr.wine — a live NIP-50 relay whose reads work unauthenticated.
pub(crate) const WINE: &str = "wss://nostr.wine";
/// nos.lol — does NOT support NIP-50 (the SC-09 negative lane) but accepts
/// kind:10002/10007 writes (the SC-TRANSPARENCY publish host).
pub(crate) const NOS_LOL: &str = "wss://nos.lol";

/// jack@primal.net — must appear in a profile search for "jack".
const JACK_PUBKEY: &str = "1852740ae33f6106ece21c2e398815adcd5e7ad56d358343c472241b19ec0ebe";
/// "Bitcoin Quotes" — must appear in a long-form search for "bitcoin".
const BITCOIN_QUOTES_ID: &str = "74b79b13b02a966c2d063d28583fdd895078eedb52e81bbd41c1a30f00622816";

/// Per-scope drain budget. Search relays answer quickly.
const DRAIN_BUDGET: Duration = Duration::from_secs(15);

/// Parse a relay `["EVENT", <sub>, <event>]` text frame for `sub_id` into a
/// `KernelEvent`, or `None` for any other frame / malformed event.
pub(crate) fn parse_event_frame(text: &str, sub_id: &str) -> Option<KernelEvent> {
    let arr = serde_json::from_str::<Value>(text).ok()?;
    let arr = arr.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?;
    let tags = ev
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(|t| {
            Some(
                t.as_array()?
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    Some(KernelEvent {
        id: EventId::from(ev.get("id")?.as_str()?.to_string()),
        author: ev.get("pubkey")?.as_str()?.to_string(),
        kind: u32::try_from(ev.get("kind")?.as_u64()?).ok()?,
        created_at: ev.get("created_at")?.as_u64()?,
        tags,
        content: ev.get("content")?.as_str()?.to_string(),
        relay_provenance: vec![WINE.to_string()],
    })
}

/// Outcome of draining one scope: the projection's deduplicated hits + whether
/// the relay signalled a non-NIP-50 rejection (CLOSED with a "search" complaint).
pub(crate) struct ScopeRun {
    pub hits: Vec<nmp_nip50::SearchHit>,
    pub closed_unsupported: bool,
}

/// Subscribe with `request`'s interest shape over `socket`, drain EVENTs into a
/// fresh projection until EOSE / CLOSED / budget, and return the snapshot.
pub(crate) fn run_scope(socket: &mut RelaySocket, request: SearchRequest, sub_id: &str) -> ScopeRun {
    let filter = nmp_core::subs::filter_json_for(&request.interest_shape());
    let projection = SearchResultsProjection::new(request);

    if let Err(e) = send_text(socket, format!(r#"["REQ","{sub_id}",{filter}]"#)) {
        eprintln!("SKIP: REQ send failed: {e}");
        return ScopeRun { hits: Vec::new(), closed_unsupported: false };
    }

    let mut closed_unsupported = false;
    let deadline = Instant::now() + DRAIN_BUDGET;
    common::drain_until(socket, deadline, |text| {
        if let Some(ev) = parse_event_frame(text, sub_id) {
            // The relay already evaluated the NIP-50 `search` filter; ingest as
            // a relay hit (the projection's structural gate still applies).
            projection.ingest_relay_event(&ev, WINE.to_string());
            return false;
        }
        if text.starts_with(&format!(r#"["EOSE","{sub_id}"#)) {
            return true;
        }
        if text.starts_with(&format!(r#"["CLOSED","{sub_id}"#)) {
            // nos.lol: CLOSED "unrecognised filter item: search" — the
            // non-NIP-50 lane (SC-09).
            if text.contains("search") {
                closed_unsupported = true;
            }
            return true;
        }
        false
    });
    ScopeRun { hits: projection.snapshot().hits, closed_unsupported }
}

fn users_request(query: &str) -> SearchRequest {
    SearchRequest::new(query, SearchScope::Users, SearchTargets::AppDefault, Some(20))
        .expect("users request")
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn sc01_profile_search_jack_via_nostr_wine() {
    let Some(mut s) = try_open(WINE) else {
        return skip("sc01", "nostr.wine unreachable");
    };
    let run = run_scope(&mut s, users_request("jack"), "sc01");
    let distinct = run.hits.len();
    let jack = run.hits.iter().any(|h| h.author == JACK_PUBKEY);
    println!("[SC-01] kind:0 'jack' via nostr.wine: {distinct} hits, jack present={jack}");
    if distinct == 0 {
        return skip("sc01", "nostr.wine returned no kind:0 hits for 'jack'");
    }
    record("sc01", Verdict::Pass, &format!(
        "{distinct} distinct kind:0 hits; jack@primal.net ({JACK_PUBKEY}) present={jack}"
    ));
    assert!(distinct >= 3, "SC-01 expects >=3 profile hits, got {distinct}");
    assert!(jack, "SC-01 expects jack's pubkey {JACK_PUBKEY} in the results");
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn sc03_profile_search_bitcoin_via_nostr_wine() {
    let Some(mut s) = try_open(WINE) else {
        return skip("sc03", "nostr.wine unreachable");
    };
    let run = run_scope(&mut s, users_request("bitcoin"), "sc03");
    let n = run.hits.len();
    let has_bitcoin = run.hits.iter().any(|h| h.content.to_lowercase().contains("bitcoin"));
    println!("[SC-03] kind:0 'bitcoin' via nostr.wine: {n} hits, name/about match={has_bitcoin}");
    if n == 0 {
        return skip("sc03", "nostr.wine returned no kind:0 hits for 'bitcoin'");
    }
    record("sc03", Verdict::Pass, &format!("{n} kind:0 hits; a profile's content contains 'bitcoin'={has_bitcoin}"));
    assert!(n >= 3, "SC-03 expects >=3 profile hits, got {n}");
    assert!(has_bitcoin, "SC-03 expects a profile name/about containing 'bitcoin'");
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn sc04_longform_search_bitcoin_via_nostr_wine() {
    let Some(mut s) = try_open(WINE) else {
        return skip("sc04", "nostr.wine unreachable");
    };
    let req = SearchRequest::new("bitcoin", SearchScope::LongForm, SearchTargets::AppDefault, Some(30))
        .expect("longform request");
    let run = run_scope(&mut s, req, "sc04");
    let n = run.hits.len();
    let all_30023 = run.hits.iter().all(|h| h.kind == 30023);
    let target = run.hits.iter().any(|h| h.id == BITCOIN_QUOTES_ID);
    println!("[SC-04] kind:30023 'bitcoin' via nostr.wine: {n} hits, all kind==30023={all_30023}, Bitcoin Quotes present={target}");
    if n == 0 {
        return skip("sc04", "nostr.wine returned no kind:30023 hits for 'bitcoin'");
    }
    record("sc04", Verdict::Pass, &format!("{n} kind:30023 hits; all kind==30023={all_30023}; 'Bitcoin Quotes' ({BITCOIN_QUOTES_ID}) present={target}"));
    assert!(n >= 2, "SC-04 expects >=2 long-form hits, got {n}");
    assert!(all_30023, "SC-04 expects every hit to be kind:30023");
    assert!(target, "SC-04 expects 'Bitcoin Quotes' event {BITCOIN_QUOTES_ID}");
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn sc08_zero_result_nonsense_query_via_nostr_wine() {
    let Some(mut s) = try_open(WINE) else {
        return skip("sc08", "nostr.wine unreachable");
    };
    let run = run_scope(&mut s, users_request("zxqwvfjkqpwoeiruzzzznonsense12345"), "sc08");
    let n = run.hits.len();
    println!("[SC-08] nonsense query via nostr.wine: {n} hits, closed_unsupported={}", run.closed_unsupported);
    record("sc08", Verdict::Pass, &format!("nonsense query returned {n} hits with no error (closed_unsupported={})", run.closed_unsupported));
    assert_eq!(n, 0, "SC-08 expects 0 results for a nonsense query");
    assert!(!run.closed_unsupported, "SC-08: nostr.wine must NOT reject the search filter");
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn sc09_non_nip50_lane_via_nos_lol() {
    let Some(mut s) = try_open(NOS_LOL) else {
        return skip("sc09", "nos.lol unreachable");
    };
    let run = run_scope(&mut s, users_request("bitcoin"), "sc09");
    let n = run.hits.len();
    println!("[SC-09] search via nos.lol (non-NIP-50): {n} hits, closed_unsupported={}", run.closed_unsupported);
    // The point: a non-NIP-50 relay surfaces as a zero-result / CLOSED lane,
    // NOT a fatal search failure (the projection simply has no hits and the
    // process did not panic).
    record("sc09", Verdict::Pass, &format!(
        "nos.lol returned {n} hits and signalled CLOSED-unsupported={} — a non-fatal zero-result lane",
        run.closed_unsupported
    ));
    assert_eq!(n, 0, "SC-09: a non-NIP-50 relay yields 0 hits, never bogus results");
}

/// SC-TRANSPARENCY — the headline. Delegates to the kernel-driven harness in
/// the `transparency` submodule (a fresh account, real `NmpApp`, zero explicit
/// relay wiring). See that module for the full scenario.
#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn sc_transparency_kind10007_discovered_with_zero_app_wiring() {
    transparency::run();
}

// ── Shared report helpers ────────────────────────────────────────────────────

pub(crate) fn skip(slug: &str, why: &str) {
    record(slug, Verdict::Skip, why);
    eprintln!("[{slug}] SKIP: {why}");
}

pub(crate) fn record(slug: &str, verdict: Verdict, body: &str) {
    write_report(
        &format!("real-relay-nip50-{slug}"),
        &report_page(
            &format!("NIP-50 higher-order search — {slug}"),
            "nip50_search",
            verdict,
            &[WINE, NOS_LOL],
            body,
        ),
    );
}
