use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Regression guard: `sync_wallet_status` must mark the kernel dirty.
///
/// Wallet status is NOT a kernel field (D0 — NWC is an app noun), so the
/// slot write alone does not flip `changed_since_emit`. The actor's regular
/// tick (`tick::flush_due`) only emits when that flag is set; without the
/// explicit `mark_changed_since_emit`, a kind:23195 balance response — which
/// the kernel itself drops as an unknown kind — would never drive a
/// projection refresh until some unrelated kernel mutation happened to set
/// the flag.
#[test]
fn sync_wallet_status_marks_kernel_dirty_so_the_projection_emits() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Clear the flag a fresh kernel starts with so the assertion below
    // genuinely observes `sync_wallet_status`'s effect.
    let _ = kernel.make_update(true);
    assert!(
        !kernel.changed_since_emit(),
        "precondition: a just-emitted kernel must be clean",
    );

    let wallet = WalletRuntime::new(new_wallet_status_slot());
    sync_wallet_status(&wallet, &mut kernel);

    assert!(
        kernel.changed_since_emit(),
        "sync_wallet_status must mark the kernel dirty so the next due \
         tick emits the refreshed wallet projection",
    );
}

// ── pay_invoice response round-trip ─────────────────────────────────────
//
// These tests cover the original bug: a kind:23195 `pay_invoice` response
// was silently dropped. Every test below threads a dispatched
// correlation_id all the way from `wallet_pay_invoice` through a
// synthetic relay frame back into `action_results`, proving the round-trip
// closes the host spinner that used to hang forever.

/// The two endpoint keys for tests — a deterministic NWC connection.
/// `CLIENT_SECRET` is what the host's NWC URI carries; `WALLET_SECRET` is
/// the wallet service's side. Mirrors `decode.rs`'s test constants so the
/// `nmp-nwc` crate's own round-trip tests stay readable alongside this
/// integration layer.
const TEST_CLIENT_SECRET: &str =
    "0101010101010101010101010101010101010101010101010101010101010101";
const TEST_WALLET_SECRET: &str =
    "0202020202020202020202020202020202020202020202020202020202020202";

fn wallet_pubkey_hex() -> String {
    nmp_nwc::crypto::client_pubkey_hex(TEST_WALLET_SECRET).unwrap()
}

/// Build a `nostr+walletconnect://` URI for the deterministic test keys.
fn test_nwc_uri() -> String {
    format!(
        "nostr+walletconnect://{}?relay=wss%3A%2F%2Frelay.test&secret={}",
        wallet_pubkey_hex(),
        TEST_CLIENT_SECRET,
    )
}

/// Build a realistic `["EVENT", <sub>, {<event>}]` kind:23195 frame whose
/// `content` is the NIP-04-encrypted `response_payload`, encrypted
/// wallet→client.
///
/// `request_event_id` is the id of the original kind:23194 request — it
/// goes into the response's `e` tag, which is how `handle_nwc_text`
/// correlates the reply to its inflight payment (NIP-47 §3.2).
fn build_response_frame(
    response_event_id: &str,
    request_event_id: &str,
    response_payload: serde_json::Value,
) -> String {
    let wallet_pk = wallet_pubkey_hex();
    let client_pk = nmp_nwc::crypto::client_pubkey_hex(TEST_CLIENT_SECRET).unwrap();
    let plaintext = serde_json::to_string(&response_payload).unwrap();
    // Wallet encrypts to the client's pubkey using the wallet secret —
    // the same direction the real wallet service does.
    let content =
        nmp_nwc::crypto::encrypt(TEST_WALLET_SECRET, &client_pk, &plaintext).unwrap();
    let frame = json!([
        "EVENT",
        "sub-test",
        {
            "id": response_event_id,
            "kind": 23195u32,
            "pubkey": wallet_pk,
            "content": content,
            "tags": [["e", request_event_id]],
        }
    ]);
    serde_json::to_string(&frame).unwrap()
}

/// Drive `wallet_connect` then send a `get_info` response so the
/// connection reaches `status = "ready"`. Returns the populated wallet
/// runtime ready for a `wallet_pay_invoice` call.
///
/// `get_info` responses don't carry the same correlation-id contract as
/// `pay_invoice` (the request id is the connect-time bootstrap, not a
/// dispatched action), so an arbitrary placeholder request id is enough
/// — the handler matches on `result_type == "get_info"`, not the id.
fn ready_wallet_for_payment(kernel: &mut Kernel) -> WalletRuntime {
    let mut wallet = WalletRuntime::new(new_wallet_status_slot());
    let _ = wallet_connect(&mut wallet, kernel, &test_nwc_uri());
    // Bring status to "ready". The actual `get_info` request id the
    // wallet would echo back via `e` is internal to wallet_connect's
    // outbound EVENT; for a single get_info we don't need to match it —
    // the response handler keys "ready" off `result_type`, not the id.
    let frame = build_response_frame(
        "ff".repeat(32).as_str(),
        "00".repeat(32).as_str(),
        json!({ "result_type": "get_info", "error": null, "result": {
            "alias": "test-wallet",
            "color": null, "pubkey": null, "network": null, "methods": ["pay_invoice"]
        } }),
    );
    let _ = handle_nwc_text(&mut wallet, &frame, kernel);
    wallet
}

/// Extract the kind:23194 event id from the first outbound EVENT frame
/// `wallet_pay_invoice` produced — that is the request id the kind:23195
/// response's `e` tag must carry so the response handler can correlate.
fn first_pay_invoice_request_id(outbound: &[OutboundMessage]) -> String {
    let frame = outbound
        .iter()
        .find(|m| m.text.starts_with("[\"EVENT\""))
        .expect("wallet_pay_invoice must emit a kind:23194 EVENT frame");
    let parsed: serde_json::Value = serde_json::from_str(&frame.text).unwrap();
    parsed
        .as_array()
        .and_then(|arr| arr.get(1))
        .and_then(|ev| ev.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .expect("outbound EVENT must have an id")
}

/// Read `projections.action_results` from a fresh wire snapshot. Returns
/// `Null` when the projection key is absent (nothing settled this tick).
fn action_results_snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update(true);
    let parsed: serde_json::Value = serde_json::from_str(&snapshot_json).unwrap();
    parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// A successful `pay_invoice` response carrying a dispatched correlation_id
/// surfaces a terminal `"ok"` entry in `action_results` — the host's
/// payment spinner clears on the next tick. This is the round-trip the
/// original bug broke: before the fix, the response was decoded but the
/// `pay_invoice` branch did nothing, so the spinner hung forever.
#[test]
fn pay_invoice_response_success_surfaces_ok_terminal_in_action_results() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = ready_wallet_for_payment(&mut kernel);
    // Drain any action_results that may have been incidentally produced
    // by the bootstrap.
    let _ = action_results_snapshot(&mut kernel);

    let correlation_id = "corr-pay-ok".to_string();
    let outbound = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc100n1p3xnhl2pp5",
        Some(10_000),
        Some(correlation_id.clone()),
    );
    let request_id = first_pay_invoice_request_id(&outbound);

    // Synthesize the wallet's successful pay_invoice response. The `e`
    // tag MUST carry `request_id` — that's how `handle_nwc_text` finds
    // the inflight payment.
    let frame = build_response_frame(
        "ee".repeat(32).as_str(),
        &request_id,
        json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "deadbeef".repeat(8) }
        }),
    );
    let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

    let results = action_results_snapshot(&mut kernel);
    let arr = results
        .as_array()
        .expect("a settled pay_invoice must surface a terminal in action_results");
    let entry = arr
        .iter()
        .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(&correlation_id))
        .expect("the dispatch correlation_id must appear in action_results");
    // The wire-side serializer (`take_action_results_projection`)
    // translates engine status `"ok"` → host-visible `"published"`. The
    // iOS shell keys its spinner cleanup on this exact string.
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("published"),
        "successful pay_invoice must report the wire status `published`",
    );
    assert!(
        entry.get("error").map(|v| v.is_null()).unwrap_or(true),
        "success entry must carry null/absent error",
    );
}

/// A `pay_invoice` response carrying an `error` object closes the dispatched
/// correlation_id with a `"failed"` terminal — the host sees the actual
/// NWC error code instead of a generic timeout, and the spinner clears.
#[test]
fn pay_invoice_response_error_surfaces_failed_terminal_in_action_results() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = ready_wallet_for_payment(&mut kernel);
    let _ = action_results_snapshot(&mut kernel);

    let correlation_id = "corr-pay-err".to_string();
    let outbound = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc200n1xxx",
        Some(20_000),
        Some(correlation_id.clone()),
    );
    let request_id = first_pay_invoice_request_id(&outbound);

    let frame = build_response_frame(
        "11".repeat(32).as_str(),
        &request_id,
        json!({
            "result_type": "pay_invoice",
            "error": { "code": "PAYMENT_FAILED", "message": "no route" },
            "result": null
        }),
    );
    let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

    let results = action_results_snapshot(&mut kernel);
    let arr = results
        .as_array()
        .expect("a failed pay_invoice must surface a terminal in action_results");
    let entry = arr
        .iter()
        .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(&correlation_id))
        .expect("the dispatch correlation_id must appear in action_results");
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "an error response reports the terminal `failed` status",
    );
    let err = entry
        .get("error")
        .and_then(|v| v.as_str())
        .expect("a failed entry carries a non-null error string");
    assert!(
        err.contains("PAYMENT_FAILED") && err.contains("no route"),
        "the failure carries the NWC code + message verbatim: {err}",
    );
}

/// A C-ABI direct caller passes `correlation_id == None`. The response
/// handler must NOT panic, MUST still drain the per-payment entry, and
/// MUST NOT push any spurious entry into `action_results` (nothing is
/// waiting on an id). Without this path the no-correlation case would
/// hit the same bug that motivated the fix.
#[test]
fn pay_invoice_response_without_correlation_id_drains_silently() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = ready_wallet_for_payment(&mut kernel);
    let _ = action_results_snapshot(&mut kernel);

    let outbound = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc300n1xxx",
        None, // No amount override.
        None, // No dispatched correlation_id — C-ABI direct path.
    );
    let request_id = first_pay_invoice_request_id(&outbound);

    let frame = build_response_frame(
        "22".repeat(32).as_str(),
        &request_id,
        json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "cafe".repeat(16) }
        }),
    );
    let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

    let results = action_results_snapshot(&mut kernel);
    assert!(
        results.is_null() || results.as_array().map(Vec::is_empty).unwrap_or(false),
        "C-ABI direct pay_invoice must NOT push an action_results entry (got {results})",
    );
    // The pending_payments slot must have been removed — verify via the
    // private field through the connection accessor used by other tests.
    assert!(
        wallet
            .connection
            .as_ref()
            .map(|c| !c.pending_payments.contains_key(&request_id))
            .unwrap_or(true),
        "the response handler must drain pending_payments even when correlation_id is None",
    );
}

/// A dispatched payment whose wallet response carries an unmatched `e`
/// tag (a stale or duplicate frame the connection didn't initiate) MUST
/// NOT push an `action_results` entry under any other inflight
/// correlation_id — the dispatched action's spinner remains waiting for
/// its own response. D6: silent on unknown, never panic.
#[test]
fn pay_invoice_response_with_unknown_request_id_does_not_misroute() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = ready_wallet_for_payment(&mut kernel);
    let _ = action_results_snapshot(&mut kernel);

    // Issue a real payment so an inflight entry exists.
    let correlation_id = "corr-still-waiting".to_string();
    let _outbound = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc400n1xxx",
        Some(40_000),
        Some(correlation_id.clone()),
    );

    // Send a response whose `e` tag points to a request id we never
    // sent.
    let bogus_request_id = "ab".repeat(32);
    let frame = build_response_frame(
        "33".repeat(32).as_str(),
        &bogus_request_id,
        json!({
            "result_type": "pay_invoice",
            "error": null,
            "result": { "preimage": "00".repeat(32) }
        }),
    );
    let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

    let results = action_results_snapshot(&mut kernel);
    if let Some(arr) = results.as_array() {
        assert!(
            arr.iter().all(|e| {
                e.get("correlation_id").and_then(|v| v.as_str()) != Some(&correlation_id)
            }),
            "an unmatched response must not falsely close the unrelated inflight payment",
        );
    }
}

/// Build an `["EVENT", <sub>, {event}]` kind:23195 frame WITHOUT any
/// `tags` field — exercises the bootstrap-compatibility path where a
/// real-world wallet (some Alby / Mutiny builds) returns a `get_info`
/// reply that doesn't carry the NIP-47 §3.2 `e` tag. The lenient decoder
/// must accept it; only `pay_invoice` correlation needs the tag.
fn build_response_frame_no_tags(
    response_event_id: &str,
    response_payload: serde_json::Value,
) -> String {
    let wallet_pk = wallet_pubkey_hex();
    let client_pk = nmp_nwc::crypto::client_pubkey_hex(TEST_CLIENT_SECRET).unwrap();
    let plaintext = serde_json::to_string(&response_payload).unwrap();
    let content =
        nmp_nwc::crypto::encrypt(TEST_WALLET_SECRET, &client_pk, &plaintext).unwrap();
    let frame = json!([
        "EVENT",
        "sub-test",
        {
            "id": response_event_id,
            "kind": 23195u32,
            "pubkey": wallet_pk,
            "content": content,
            // No `tags` field — some wallets omit it on get_info.
        }
    ]);
    serde_json::to_string(&frame).unwrap()
}

/// Bootstrap-compatibility regression guard: a `get_info` response WITHOUT
/// an `e` tag must still drive the connection to `status = "ready"`.
/// Tightening the response handler to the strict NIP-47 §3.2 decoder for
/// bootstrap would break real shipped wallets that omit the tag — the
/// strict matcher is only applied to `pay_invoice` (where the
/// correlation IS protocol-mandatory).
#[test]
fn get_info_response_without_e_tag_still_drives_status_ready() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = WalletRuntime::new(new_wallet_status_slot());
    let _ = wallet_connect(&mut wallet, &mut kernel, &test_nwc_uri());
    let frame = build_response_frame_no_tags(
        "aa".repeat(32).as_str(),
        json!({ "result_type": "get_info", "error": null, "result": {
            "alias": "lenient-wallet",
            "color": null, "pubkey": null, "network": null, "methods": ["pay_invoice"]
        } }),
    );
    let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);
    assert_eq!(
        wallet.connection.as_ref().map(|c| c.status.as_str()),
        Some("ready"),
        "a get_info response without the `e` tag must still bring the wallet to `ready`",
    );
}

/// `WalletDisconnect` mid-payment must close every inflight dispatched
/// correlation_id as `Failed` — without this fix a user who cancels a
/// payment (or whose iOS shell tears down the connection on backgrounding)
/// leaks the host spinner exactly the same way the response-not-handled
/// bug did. Same broken-promise class, different lifecycle entry point.
#[test]
fn wallet_disconnect_closes_inflight_pay_invoice_correlations() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = ready_wallet_for_payment(&mut kernel);
    let _ = action_results_snapshot(&mut kernel);

    let cid_a = "corr-inflight-a".to_string();
    let cid_b = "corr-inflight-b".to_string();
    let _ = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc100n1aaa",
        Some(10_000),
        Some(cid_a.clone()),
    );
    let _ = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc200n1bbb",
        Some(20_000),
        Some(cid_b.clone()),
    );

    // User backgrounds the app / cancels — the iOS shell calls
    // `nmp_app_wallet_disconnect`, which routes through here.
    let _ = wallet_disconnect(&mut wallet, &mut kernel);

    let results = action_results_snapshot(&mut kernel);
    let arr = results
        .as_array()
        .expect("disconnect must produce action_results terminals for inflight payments");
    let ids: std::collections::HashSet<&str> = arr
        .iter()
        .filter_map(|e| e.get("correlation_id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(cid_a.as_str()) && ids.contains(cid_b.as_str()),
        "both inflight correlation_ids must close as Failed on disconnect (got {ids:?})",
    );
    for entry in arr {
        if let Some(cid) = entry.get("correlation_id").and_then(|v| v.as_str()) {
            if cid == cid_a || cid == cid_b {
                assert_eq!(
                    entry.get("status").and_then(|v| v.as_str()),
                    Some("failed"),
                    "disconnect-induced termination reports `failed`",
                );
            }
        }
    }
}

/// A dispatched `pay_invoice` called against a wallet that never
/// connected (or whose status is still "connecting") fails the action
/// CLOSED — the host spinner clears immediately rather than waiting on a
/// kind:23195 response that will never come (because no kind:23194 went
/// out). Mirrors the sign-step early-exit precedent in `publish.rs`.
#[test]
fn pay_invoice_with_no_connected_wallet_records_immediate_failure() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut wallet = WalletRuntime::new(new_wallet_status_slot());
    let _ = action_results_snapshot(&mut kernel);

    let correlation_id = "corr-no-wallet".to_string();
    let outbound = wallet_pay_invoice(
        &mut wallet,
        &mut kernel,
        "lnbc500n1xxx",
        Some(50_000),
        Some(correlation_id.clone()),
    );
    assert!(
        outbound.is_empty(),
        "no wallet means no outbound — request never goes on the wire",
    );

    let results = action_results_snapshot(&mut kernel);
    let arr = results
        .as_array()
        .expect("the early-exit failure must surface an action_results terminal");
    let entry = arr
        .iter()
        .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(&correlation_id))
        .expect("dispatched correlation_id must be closed even on the early-exit path");
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "no-wallet early exit reports the terminal `failed` status",
    );
}
