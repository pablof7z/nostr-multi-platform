use std::sync::Arc;

use nmp_core::planner::{InterestId, InterestLifecycle};
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, RelayRole};
use nmp_coverage_gate::CoverageGate;

use crate::{NegentropySyncRuntime, RelayNegentropyState};

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
fn opens_negentropy_for_author_kind_product_at_threshold() {
    let runtime = Arc::new(NegentropySyncRuntime::new(CoverageGate::default()));
    let mut kernel = Kernel::testing_new(50);
    let out = runtime
        .intercept_req(&mut kernel, &ctx(25, &[3, 10_000]))
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].text().starts_with(r#"["NEG-OPEN","sub-large","#));
}

#[test]
fn counts_three_kinds_times_twenty_authors() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    assert!(runtime
        .intercept_req(&mut kernel, &ctx(20, &[0, 3, 10_002]))
        .is_some());
}

#[test]
fn below_threshold_or_tailing_falls_back_to_raw_req() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    assert!(runtime
        .intercept_req(&mut kernel, &ctx(24, &[3, 10_000]))
        .is_none());
    let mut tailing = ctx(50, &[1]);
    tailing.lifecycle = InterestLifecycle::Tailing;
    assert!(runtime.intercept_req(&mut kernel, &tailing).is_none());
}

#[test]
fn neg_err_falls_back_to_original_req_and_marks_unsupported() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);
    let ctx = ctx(50, &[3]);
    assert!(runtime.intercept_req(&mut kernel, &ctx).is_some());
    let out = runtime.on_relay_text(
        &mut kernel,
        "wss://relay.example",
        r#"["NEG-ERR","sub-large","unsupported"]"#,
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].text().starts_with(r#"["REQ","sub-large","#));
    assert_eq!(
        runtime.relay_state("wss://relay.example"),
        RelayNegentropyState::Unsupported
    );
}
