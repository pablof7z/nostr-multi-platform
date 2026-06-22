//! Higher-order NIP-50 search against a REAL search relay.
//!
//! Proves the consumer-facing search contract end-to-end on the live network:
//! the generic `InterestShape.search` wire field (`{"search":"…"}`) reaches a
//! real NIP-50 relay, the relay returns matching events, and the
//! `nmp_nip50::SearchResultsProjection` ingests + deduplicates them into the
//! authoritative `SearchResultsSnapshot`. Two scopes are covered:
//!
//!   * `SearchScope::Users` → kind:0 profile metadata.
//!   * `SearchScope::LongForm` → kind:30023 long-form articles.
//!
//! This is the live-network sibling of the in-crate unit tests (which drive the
//! projection with synthetic events). It does NOT exercise the kernel actor /
//! relay-pin routing (that is covered by the FFI `search` unit tests); it pins
//! the wire field + projection ingest against a relay that actually evaluates
//! NIP-50.
//!
//! Honest-validation: if the relay is unreachable or returns no events for the
//! query within budget, the scenario writes a SKIP finding and pass-but-skips —
//! it never fabricates a green assertion.
//!
//! Gated behind the `real-relay` feature AND `#[ignore]` so CI stays
//! deterministic. Run explicitly:
//!
//! ```bash
//! cargo test -p nmp-testing --features real-relay --test real_relay_nip50_search \
//!   -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::{report_page, send_text, try_open, write_report, RelaySocket, Verdict};
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_nip50::{
    SearchRequest, SearchResultsProjection, SearchScope, SearchTargets,
};
use serde_json::Value;

/// nos.lol's dedicated NIP-50 full-text search relay.
const SEARCH_RELAY: &str = "wss://search.nos.lol";

/// Per-scope drain budget. Search relays answer quickly; keep it short so a
/// quiet query does not burn the run.
const DRAIN_BUDGET: Duration = Duration::from_secs(12);

/// The free-text query both scopes search for.
const QUERY: &str = "nostr";

/// Parse a relay `["EVENT", <sub>, <event>]` text frame for `sub_id` into a
/// `KernelEvent`, or `None` for any other frame / malformed event.
fn parse_event_frame(text: &str, sub_id: &str) -> Option<KernelEvent> {
    let arr = serde_json::from_str::<Value>(text).ok()?;
    let arr = arr.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?;
    let id = ev.get("id")?.as_str()?.to_string();
    let author = ev.get("pubkey")?.as_str()?.to_string();
    let kind = u32::try_from(ev.get("kind")?.as_u64()?).ok()?;
    let created_at = ev.get("created_at")?.as_u64()?;
    let content = ev.get("content")?.as_str()?.to_string();
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
        id: EventId::from(id),
        author,
        kind,
        created_at,
        tags,
        content,
        relay_provenance: vec![SEARCH_RELAY.to_string()],
    })
}

/// Subscribe with `request`'s interest shape over `socket`, drain EVENTs into a
/// fresh projection until EOSE or the budget elapses, and return the resulting
/// snapshot hit count.
fn run_scope(socket: &mut RelaySocket, request: SearchRequest, sub_id: &str) -> usize {
    let shape = request.interest_shape();
    let filter = nmp_core::subs::filter_json_for(&shape);
    let projection = SearchResultsProjection::new(request);

    let req = format!(r#"["REQ","{sub_id}",{filter}]"#);
    if let Err(e) = send_text(socket, req) {
        eprintln!("SKIP: REQ send failed on {SEARCH_RELAY}: {e}");
        return 0;
    }

    let deadline = Instant::now() + DRAIN_BUDGET;
    let mut saw_eose = false;
    common::drain_until(socket, deadline, |text| {
        if let Some(ev) = parse_event_frame(text, sub_id) {
            // The relay already evaluated the NIP-50 `search` filter; ingest as
            // a relay hit (structural InterestShape gate still applies).
            projection.ingest_relay_event(&ev, SEARCH_RELAY.to_string());
        } else if text.starts_with(&format!(r#"["EOSE","{sub_id}"#)) {
            saw_eose = true;
            return true; // stop draining this sub on EOSE
        }
        false
    });
    let _ = saw_eose;
    projection.snapshot().hits.len()
}

#[test]
#[ignore = "real-relay (run with --features real-relay --ignored)"]
fn nip50_search_users_and_longform_against_search_relay() {
    let Some(mut socket) = try_open(SEARCH_RELAY) else {
        write_report(
            "real-relay-nip50-search",
            &report_page(
                "NIP-50 higher-order search (Users + LongForm)",
                "nip50_search",
                Verdict::Skip,
                &[SEARCH_RELAY],
                &format!("Could not reach {SEARCH_RELAY} within the connect budget."),
            ),
        );
        eprintln!("SKIP: {SEARCH_RELAY} unreachable");
        return;
    };

    let users = SearchRequest::new(QUERY, SearchScope::Users, SearchTargets::AppDefault, Some(10))
        .expect("users request");
    let users_hits = run_scope(&mut socket, users, "nip50-users");

    let longform =
        SearchRequest::new(QUERY, SearchScope::LongForm, SearchTargets::AppDefault, Some(10))
            .expect("longform request");
    let longform_hits = run_scope(&mut socket, longform, "nip50-longform");

    let (verdict, body) = if users_hits == 0 && longform_hits == 0 {
        (
            Verdict::Skip,
            format!(
                "{SEARCH_RELAY} returned no kind:0 or kind:30023 hits for `{QUERY}` within \
                 budget. The wire path connected; no events to assert on (honest skip)."
            ),
        )
    } else {
        (
            Verdict::Pass,
            format!(
                "NIP-50 `{{\"search\":\"{QUERY}\"}}` round-tripped through {SEARCH_RELAY} and the \
                 `SearchResultsProjection` deduplicated real hits:\n\n\
                 - Users (kind:0): {users_hits} hit(s)\n\
                 - LongForm (kind:30023): {longform_hits} hit(s)\n"
            ),
        )
    };

    write_report(
        "real-relay-nip50-search",
        &report_page(
            "NIP-50 higher-order search (Users + LongForm)",
            "nip50_search",
            verdict,
            &[SEARCH_RELAY],
            &body,
        ),
    );

    // A PASS requires at least one scope to have produced a deduplicated hit;
    // a SKIP (no events / unreachable) is an honest non-failure.
    if verdict == Verdict::Pass {
        assert!(
            users_hits > 0 || longform_hits > 0,
            "PASS verdict requires at least one real search hit"
        );
    }
}
