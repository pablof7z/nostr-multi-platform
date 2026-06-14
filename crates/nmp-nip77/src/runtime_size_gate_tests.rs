//! NIP-77 frame-size gate tests.
//!
//! Split from `runtime_tests.rs` to keep that file under the 500-LOC hard cap
//! (AGENTS.md file-size rule). These exercise the codec-level `FRAME_SIZE_LIMIT`
//! protection on inbound `NEG-MSG` payloads: an oversize hex blob must be
//! rejected before allocation (falling back to a plain REQ), while a within-limit
//! payload must still decode normally.

use nmp_core::planner::{InterestId, InterestLifecycle};
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, RelayRole};
use nmp_coverage_gate::CoverageGate;

use crate::codec::hex_decode_size_limited;
use crate::{NegentropySyncRuntime, FRAME_SIZE_LIMIT};

fn author(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn ctx(authors: usize, kinds: &[u32]) -> ReqFrameContext {
    ReqFrameContext {
        role: RelayRole::Content,
        relay_url: "wss://relay.example".to_string(),
        sub_id: "sub-large".to_string(),
        filter_json: serde_json::json!({
            "authors": (0..authors).map(|i| author(i as u8)).collect::<Vec<_>>(),
            "kinds": kinds,
        })
        .to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::OneShot,
    }
}

#[test]
fn oversize_neg_msg_falls_back_without_giant_alloc() {
    // Build an oversized hex string: FRAME_SIZE_LIMIT*2 + 2 hex chars.
    let oversize_len = (FRAME_SIZE_LIMIT as usize) * 2 + 2;
    // A string of 'a' repeated is valid lowercase hex but far too large.
    let oversize_hex = "aa".repeat(oversize_len / 2);
    assert!(oversize_hex.len() > FRAME_SIZE_LIMIT as usize * 2);

    // Verify the codec-level gate rejects before allocating.
    assert!(
        hex_decode_size_limited(&oversize_hex).is_err(),
        "codec size gate must reject oversize hex before alloc"
    );

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);

    // Open a NIP-77 session so the runtime has state for "sub-large".
    let opened = runtime
        .intercept_req(&mut kernel, &ctx(50, &[3]))
        .expect("large filter must open NIP-77");
    assert_eq!(opened.len(), 1);

    // Deliver the oversize NEG-MSG from the relay.
    let relay_msg = format!(r#"["NEG-MSG","sub-large","{}"]"#, oversize_hex);
    let out = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);

    // Must fall back to a plain REQ, not return empty or panic.
    assert_eq!(out.len(), 1, "oversize NEG-MSG must produce a fallback REQ");
    assert!(
        out[0].text().starts_with(r#"["REQ","sub-large","#),
        "fallback must be a REQ, got: {}",
        out[0].text()
    );
}

/// Within-limit NEG-MSG must still decode and reconcile normally (the size
/// gate must not block legitimate messages).
#[test]
fn normal_size_neg_msg_is_not_rejected_by_size_gate() {
    // A small valid hex payload (16 bytes = 32 hex chars) is well within limit.
    let small_hex = "aa".repeat(16);
    assert!(small_hex.len() <= FRAME_SIZE_LIMIT as usize * 2);
    assert!(
        hex_decode_size_limited(&small_hex).is_ok(),
        "size gate must not block within-limit hex"
    );
}
