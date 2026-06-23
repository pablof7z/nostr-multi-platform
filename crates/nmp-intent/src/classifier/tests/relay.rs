//! Rung 3 — relay-URL normalization.

use nmp_core::substrate::{InputIntentTarget, InputScopeId};

use super::{classify_bare, expect_single, profiles_scope, req};

#[test]
fn wss_url_routes_to_relayurl_normalized() {
    let r = req("wss://Relay.Example.COM", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.scope, InputScopeId::nostr_ref());
    match cand.target {
        InputIntentTarget::RelayUrl { url } => {
            // Canonical form is whatever the single authority produces; assert it
            // is idempotent under that authority and lowercased the host.
            assert_eq!(
                Some(url.clone()),
                nmp_core::substrate::canonicalize_relay_url(&url)
            );
            assert!(url.contains("relay.example.com"));
        }
        other => panic!("expected RelayUrl, got {other:?}"),
    }
}

#[test]
fn ws_url_routes_to_relayurl() {
    let r = req("ws://localhost:7777", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert!(matches!(cand.target, InputIntentTarget::RelayUrl { .. }));
}
