//! Broken-promise fix — `Kernel::record_action_failure` → `action_results`.
//!
//! A host that dispatches a `PublishRaw` / `PublishProfile` through
//! `nmp_app_dispatch_action` receives a registry-minted `correlation_id` and
//! waits to see its outcome in the `action_results` snapshot projection. Every
//! terminal verdict for a *queued* publish reaches `action_results` via the
//! publish engine. But a failure in the *sign* step (no active account, a
//! malformed reply id, a local-key sign error, a remote-signer timeout /
//! rejection) aborts the publish *before* it ever reaches the engine — there
//! is no `PublishHandle`, no in-flight row.
//!
//! Before this fix those sign-step failures only set a toast; the host's
//! spinner keyed on the returned `correlation_id` would hang forever — a
//! broken promise (a correlation_id was returned but its outcome is never
//! observable). `Kernel::record_action_failure` closes that gap by pushing a
//! terminal `"failed"` verdict into the same per-tick `action_results` drain.
//!
//! These tests pin the *kernel-layer* contract — that `record_action_failure`
//! lands a `{correlation_id, status:"failed", error}` entry in the wire
//! snapshot. The engine-side push (`record_action_terminal_failure`) is
//! covered by `publish/engine/tests.rs`; the actor-loop wiring
//! (parked-remote-sign timeout / error) is covered there in lockstep.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{SignedEvent, UnsignedEvent};

/// Read `projections.action_results` from a fresh wire snapshot. The key is
/// conditionally inserted (only when a terminal settled this tick), so absence
/// is reported here as `Null`.
fn action_results(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn record_action_failure_surfaces_failed_terminal_in_action_results() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // No action recorded yet — the projection key is absent.
    assert!(
        action_results(&mut kernel).is_null(),
        "a kernel with no settled action has no action_results key"
    );

    kernel.record_action_failure(
        "corr-no-account".to_string(),
        "no active account".to_string(),
    );

    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array after a recorded failure");
    assert_eq!(arr.len(), 1, "exactly one terminal verdict this tick");
    let entry = &arr[0];
    assert_eq!(
        entry.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-no-account"),
        "the dispatch correlation_id is carried through so the host can match its spinner"
    );
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "a sign-step failure reports the terminal `failed` status"
    );
    assert_eq!(
        entry.get("error").and_then(|v| v.as_str()),
        Some("no active account"),
        "the failure reason is carried verbatim for the host to display"
    );
}

#[test]
fn record_action_failure_is_drained_per_tick() {
    // `action_results` is a per-tick drain: the failure verdict appears once
    // and is consumed — a second snapshot tick (nothing new) omits the key.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_action_failure("corr-once".to_string(), "sign failed: rejected".to_string());

    assert!(
        action_results(&mut kernel).as_array().is_some(),
        "the first tick after a recorded failure carries the verdict"
    );
    assert!(
        action_results(&mut kernel).is_null(),
        "the verdict is drained — a second tick omits the action_results key"
    );
}

#[test]
fn multiple_action_failures_in_one_tick_all_survive() {
    // Two dispatched actions whose sign step fails between snapshot emits both
    // reach `action_results` — neither host spinner is stranded.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_action_failure("corr-a".to_string(), "no active account".to_string());
    kernel.record_action_failure(
        "corr-b".to_string(),
        "reply: malformed target event id".to_string(),
    );

    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array when failures were recorded");
    assert_eq!(arr.len(), 2, "both failures settle in the same tick");
    let mut ids: Vec<&str> = arr
        .iter()
        .filter_map(|item| item.get("correlation_id").and_then(|v| v.as_str()))
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec!["corr-a", "corr-b"],
        "both correlation_ids appear — the per-tick Vec accumulates before the drain"
    );
    for item in arr {
        assert_eq!(
            item.get("status").and_then(|v| v.as_str()),
            Some("failed"),
            "each recorded sign-step failure reports `failed`"
        );
    }
}

#[test]
fn record_action_success_surfaces_published_terminal_in_action_results() {
    // The success leg of the same per-tick drain — `record_action_success`
    // with no result body lands a `{correlation_id, status:"published"}` row.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_action_success("corr-ok".to_string(), None);

    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be an array after a recorded success");
    assert_eq!(arr.len(), 1);
    let entry = &arr[0];
    assert_eq!(
        entry.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-ok")
    );
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("published"),
        "the kernel maps the engine's `ok` to the wire `published` status"
    );
    assert!(
        entry.get("result").is_none(),
        "no result_json → no `result` field (existing rows are byte-unchanged)"
    );
}

#[test]
fn record_action_success_with_result_json_carries_structured_result() {
    // ADR-0043 Decision 4 — a success terminal that attaches an opaque
    // structured result body surfaces it under the row's `result` field, as a
    // parsed JSON OBJECT (not a JSON-encoded string). `nmp-core` forwards it
    // verbatim and never interprets it — this is the linchpin the Blossom host
    // reads from `action_results[cid].result`.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let descriptor = r#"{"url":"https://b.example/abc.png","sha256":"abc","size":5,"type":"image/png","uploaded":1733356800}"#;
    kernel.record_action_success("corr-blob".to_string(), Some(descriptor.to_string()));

    let results = action_results(&mut kernel);
    let arr = results.as_array().expect("array after success");
    let entry = arr
        .iter()
        .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some("corr-blob"))
        .expect("the correlation_id must appear");
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("published")
    );
    let result = entry
        .get("result")
        .expect("result_json must surface under `result`");
    assert!(
        result.is_object(),
        "result must be a JSON object, not a JSON-encoded string: {result}"
    );
    assert_eq!(
        result.get("sha256").and_then(|v| v.as_str()),
        Some("abc"),
        "the descriptor's sha256 is readable as a nested field"
    );
    assert_eq!(result.get("size").and_then(|v| v.as_u64()), Some(5));
    assert_eq!(
        result.get("url").and_then(|v| v.as_str()),
        Some("https://b.example/abc.png")
    );
}

#[test]
fn record_action_success_with_non_json_result_forwards_as_string() {
    // A non-JSON result body is forwarded as a raw string rather than dropped —
    // the kernel never parses semantically, so it cannot reject a malformed
    // body; it round-trips it so a host can still inspect it.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.record_action_success("corr-raw".to_string(), Some("not json".to_string()));
    let results = action_results(&mut kernel);
    let entry = &results.as_array().expect("array")[0];
    assert_eq!(
        entry.get("result").and_then(|v| v.as_str()),
        Some("not json"),
        "a non-JSON body forwards as a raw string under `result`"
    );
}

fn signed(id: &str, author: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: format!("content-{id}"),
            created_at: 1_700_000_000,
        },
    }
}

/// Seed a kind:10002 so the `Nip65OutboxResolver` resolves a write relay —
/// without it the engine returns `NoTargets` before the in-flight handle is
/// ever registered. Mirrors `publish_terminal_status_tests::seed_kind10002`.
fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_url: &str) {
    let raw = RawEvent {
        // Use the author pubkey as the event id — guaranteed valid hex (64
        // hex chars) and unique per author.  The old two-char prefix approach
        // embedded a literal 'k' which is not a valid hex character; V-70
        // strengthened `is_structurally_valid()` to check hex chars, so those
        // synthetic events were rejected as Malformed.
        id: author_pubkey.to_string(),
        pubkey: author_pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags: vec![vec![
            "r".to_string(),
            write_url.to_string(),
            "write".to_string(),
        ]],
        content: String::new(),
        sig: "0".repeat(128),
    };
    kernel
        .store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://seed".to_string(),
            1_700_000_000_000,
        )
        .expect("seed_kind10002 insert");
}

#[test]
fn engine_error_for_dispatched_action_reaches_action_results() {
    // A dispatched action can also fail at the *engine* layer (after a
    // successful local sign): `DuplicateHandle`, `Store`, `UnsupportedAction`.
    // `record_engine_error` writes a `RecentFailure` row but no terminal
    // action verdict — so a dispatched action carrying a `correlation_id`
    // would leak (broken promise). `run_publish_engine_at`'s `Err` arm must
    // also push the correlation_id into `action_results`.
    //
    // `DuplicateHandle` is the deterministic trigger: a kind:10002 is seeded
    // so the FIRST publish resolves a relay and registers the in-flight
    // handle; the SECOND publish of the same event then hits the
    // `in_flight.contains_key` guard and returns `DuplicateHandle`.
    let author = "ab".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, "wss://action-failure-r1.test");
    let event = signed(&"cd".repeat(32), &author);
    let correlation_id = "corr-engine-dup".to_string();

    // First publish — resolves the seeded relay, registers the in-flight
    // handle (stays in flight: the relay never acks).
    let _ = kernel.run_publish_engine_at(
        &event,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        0,
    );
    // Drain whatever the first publish produced so the next snapshot read is
    // about the second (duplicate) publish only.
    let _ = action_results(&mut kernel);

    // Second publish of the SAME event → `DuplicateHandle`.
    let _ = kernel.run_publish_engine_at(
        &event,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        10,
    );

    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("a duplicate dispatched publish must surface a terminal in action_results");
    let entry = arr
        .iter()
        .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(correlation_id.as_str()))
        .expect("the dispatch correlation_id must appear in action_results");
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "an engine-level error reports the terminal `failed` status"
    );
    let error = entry
        .get("error")
        .and_then(|v| v.as_str())
        .expect("a failed engine-error result carries a non-null error");
    assert!(
        error.contains("already in flight"),
        "the error carries the DuplicateHandle reason: {error}"
    );
}
