//! Tests for the `ActionRejection::InvalidCoded` wire shape through the FFI
//! action-dispatch entry point (issue #1734).
//!
//! Moved out of `action/tests.rs` to keep that file at its baselined LOC cap.

use super::super::{nmp_app_free, nmp_app_new};
use super::*;

/// Coded-rejection test module under `test.coded_reject` — `start()` returns
/// an `ActionRejection::InvalidCoded` with a stable `code` (issue #1734).
/// Used to verify that the FFI action-result JSON carries `{"error":…,"code":…}`.
struct TestCodedRejectModule; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
impl nmp_core::substrate::ActionModule for TestCodedRejectModule {
    const NAMESPACE: &'static str = "test.coded_reject"; // doctrine-allow: D9
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Err(ActionRejection::InvalidCoded {
            code: "test_coded_error",
            message: "coded rejection: field required".into(),
        })
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// A typed `ActionModule` whose `start()` returns `Err(ActionRejection::InvalidCoded)`
/// produces `{"error":"…","code":"…"}` — the structured rejection wire shape
/// introduced by issue #1734. The `code` field carries the stable machine token;
/// the `error` field carries the English fallback. Both must be present.
#[test]
fn coded_rejection_includes_code_field_in_ffi_json() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; pointer is valid until `nmp_app_free` below.
    let app_mut = unsafe { &mut *app };
    app_mut.register_action(TestCodedRejectModule);

    let out = dispatch_action_json(Some(&*app_mut), "test.coded_reject", "{}");
    let parsed: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|_| panic!("output was not valid JSON: {out}"));

    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected 'error' field in: {out}"));
    assert!(
        err.contains("coded rejection"),
        "English fallback must be in 'error'; got: {err}"
    );

    let code = parsed
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected 'code' field in: {out}"));
    assert_eq!(
        code, "test_coded_error",
        "stable machine code must be in 'code'; got: {code}"
    );

    // Sanity: no correlation_id on a start() rejection (action never started).
    assert!(
        parsed.get("correlation_id").is_none(),
        "a start()-phase rejection must not include a correlation_id; got: {out}"
    );

    nmp_app_free(app);
}
