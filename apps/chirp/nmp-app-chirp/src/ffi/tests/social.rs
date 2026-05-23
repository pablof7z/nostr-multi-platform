//! Social-verb migration proof: react / follow / unfollow are reachable
//! through the generic `dispatch_action` path after `nmp_app_chirp_register`.

use nmp_core::{nmp_app_free, nmp_app_new};

use super::super::{nmp_app_chirp_register, nmp_app_chirp_unregister};
use super::helpers::dispatch;

/// THE MIGRATION PROOF: after `nmp_app_chirp_register`, the three social
/// verbs are reachable through the generic `dispatch_action` path — each
/// returns a 32-hex `correlation_id`, proving BOTH the host-registered
/// module (consumed by `start()`) AND executor (consumed by `execute()`)
/// are wired. This replaces the deleted per-verb `nmp_app_react` /
/// `nmp_app_follow` / `nmp_app_unfollow` C symbols (D0).
#[test]
fn social_verbs_dispatch_through_action_registry() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    for (namespace, body) in [
        ("nmp.nip25.react", r#"{"target_event_id":"abc","reaction":"+"}"#),
        ("nmp.follow", r#"{"pubkey":"deadbeef"}"#),
        ("nmp.unfollow", r#"{"pubkey":"deadbeef"}"#),
    ] {
        let parsed = dispatch(app, namespace, body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "{namespace}: correlation id should be 32 hex");
    }

    // `nmp.nip25.react` defaults `reaction` to `"+"` when absent.
    let parsed = dispatch(app, "nmp.nip25.react", r#"{"target_event_id":"abc"}"#);
    assert!(
        parsed.get("correlation_id").is_some(),
        "nmp.nip25.react without reaction should default and succeed: {parsed}"
    );

    // Malformed JSON shape is rejected by the host module validator (D6).
    let parsed = dispatch(app, "nmp.follow", r#"{"not_pubkey":"x"}"#);
    assert!(
        parsed.get("error").is_some(),
        "wrong-shape nmp.follow must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
