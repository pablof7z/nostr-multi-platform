use super::super::{nmp_app_free, nmp_app_new};
use super::*;

fn with_app(body: impl FnOnce(&NmpApp)) {
    let app = nmp_app_new();
    body(unsafe { &*app });
    nmp_app_free(app);
}

#[test]
fn dispatch_publish_raw_action_returns_correlation_id() {
    with_app(|app| {
        let out = dispatch_action_json(
            Some(app),
            "nmp.publish",
            r#"{"PublishRaw":{"kind":1,"tags":[],"content":"smoke-test","target":"Auto"}}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .expect("expected a correlation_id field");
        assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    });
}

#[test]
fn dispatch_unknown_namespace_returns_error_json() {
    with_app(|app| {
        let out = dispatch_action_json(Some(app), "nmp.unknown", "{}");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
        assert!(err.contains("unknown action namespace"), "got: {err}");
    });
}

#[test]
fn dispatch_malformed_json_returns_error_json() {
    with_app(|app| {
        let out = dispatch_action_json(Some(app), "nmp.publish", "{bad json");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            parsed.get("error").is_some(),
            "expected error object: {out}"
        );
    });
}

#[test]
fn dispatch_null_app_returns_error_json() {
    let out = dispatch_action_json(None, "nmp.publish", "{}");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed.get("error").and_then(|v| v.as_str()),
        Some("null app")
    );
}

use nmp_core::publish::{PublishAction, PublishRouteClass, PublishTarget};

#[test]
fn dispatch_presigned_publish_action_returns_error_json() {
    with_app(|app| {
        let action = PublishAction::Publish {
            handle: "h1".to_string(),
            event: nmp_signer_iface::SignedEvent {
                id: "a".repeat(64),
                sig: "b".repeat(128),
                unsigned: nmp_signer_iface::UnsignedEvent {
                    pubkey: "c".repeat(64),
                    kind: 1,
                    tags: vec![vec!["t".to_string(), "nmp".to_string()]],
                    content: "hello from dispatch_action".to_string(),
                    created_at: 1_700_000_000,
                },
            },
            target: PublishTarget::explicit(
                vec!["wss://relay.example".to_string()],
                PublishRouteClass::ImportedOrPresigned,
            ),
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let out = dispatch_action_json(Some(app), "nmp.publish", &action_json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
        assert!(
            err.contains("internal/protocol-only"),
            "pre-signed Publish must be rejected; got: {out}"
        );
    });
}

#[test]
fn execute_action_presigned_publish_is_rejected() {
    with_app(|app| {
        let action = PublishAction::Publish {
            handle: "h2".to_string(),
            event: nmp_signer_iface::SignedEvent {
                id: "a".repeat(64),
                sig: "b".repeat(128),
                unsigned: nmp_signer_iface::UnsignedEvent {
                    pubkey: "c".repeat(64),
                    kind: 1,
                    tags: vec![vec!["t".to_string(), "nmp".to_string()]],
                    content: "hello from dispatch_action".to_string(),
                    created_at: 1_700_000_000,
                },
            },
            target: PublishTarget::Explicit {
                relays: vec!["wss://relay.example".to_string()],
                route_class: PublishRouteClass::ImportedOrPresigned,
            },
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let ctx = ActionContext::with_event_store_slot(app.event_store_handle());
        let err = execute_action(app, &ctx, "nmp.publish", &action_json, "corr-id")
            .expect_err("pre-signed Publish execute must reject");
        assert!(
            err.message.contains("internal/protocol-only"),
            "pre-signed Publish must be rejected; got: {err:?}"
        );
    });
}

#[test]
fn execute_action_publish_raw_is_ok_without_actor() {
    with_app(|app| {
        let json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"h3","target":"Auto"}}"#;
        let ctx = ActionContext::with_event_store_slot(app.event_store_handle());
        assert!(execute_action(app, &ctx, "nmp.publish", json, "corr-id").is_ok());
    });
}

#[test]
fn execute_action_unknown_namespace_returns_err() {
    with_app(|app| {
        let ctx = ActionContext::with_event_store_slot(app.event_store_handle());
        let err = execute_action(app, &ctx, "nmp.future", "{}", "corr-id")
            .expect_err("unwired namespace must surface an error");
        assert!(
            err.message.contains("no executor registered") && err.message.contains("nmp.future"),
            "error should name the unwired namespace, got: {err:?}"
        );
    });
}
