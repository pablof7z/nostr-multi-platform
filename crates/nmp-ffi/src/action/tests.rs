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

#[test]
fn ack_action_stage_null_app_is_noop() {
    let cstr = std::ffi::CString::new("corr-1").unwrap();
    super::nmp_app_ack_action_stage(std::ptr::null_mut(), cstr.as_ptr());
}

#[test]
fn ack_action_stage_null_correlation_id_is_noop() {
    with_app(|app| {
        let app_ptr = app as *const _ as *mut super::NmpApp;
        let depth_before = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
        super::nmp_app_ack_action_stage(app_ptr, std::ptr::null());
        let depth_after = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            depth_before, depth_after,
            "null correlation_id must not enqueue any command"
        );
    });
}

#[test]
fn ack_action_stage_empty_string_is_noop() {
    with_app(|app| {
        let app_ptr = app as *const _ as *mut super::NmpApp;
        let depth_before = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
        let empty = std::ffi::CString::new("").unwrap();
        super::nmp_app_ack_action_stage(app_ptr, empty.as_ptr());
        let depth_after = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(depth_before, depth_after);
    });
}

#[test]
fn ack_action_stage_well_formed_enqueues_command() {
    with_app(|app| {
        let app_ptr = app as *const _ as *mut super::NmpApp;
        let cid = std::ffi::CString::new("corr-test").unwrap();
        super::nmp_app_ack_action_stage(app_ptr, cid.as_ptr());
        let _ = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
    });
}

use nmp_core::publish::{PublishAction, PublishTarget};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

fn fixture_signed_event() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "c".repeat(64),
            kind: 1,
            tags: vec![vec!["t".to_string(), "nmp".to_string()]],
            content: "hello from dispatch_action".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn dispatch_publish_action_returns_minted_correlation_id_not_event_id() {
    with_app(|app| {
        let event = fixture_signed_event();
        let event_id = event.id.clone();
        let action = PublishAction::Publish {
            handle: "h1".to_string(),
            event,
            target: PublishTarget::Auto,
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let out = dispatch_action_json(Some(app), "nmp.publish", &action_json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got: {out}"));
        assert_ne!(
            id, event_id,
            "the returned correlation_id must NOT be the event id"
        );
        assert_eq!(
            id.len(),
            32,
            "minted correlation_id is 32-hex, not the 64-hex event id"
        );
    });
}

#[test]
fn execute_action_publish_is_ok() {
    with_app(|app| {
        let action = PublishAction::Publish {
            handle: "h2".to_string(),
            event: fixture_signed_event(),
            target: PublishTarget::Explicit {
                relays: vec!["wss://relay.example".to_string()],
            },
        };
        let action_json = serde_json::to_string(&action).unwrap();
        let ctx = ActionContext::with_event_store_slot(app.event_store_handle());
        assert!(
            execute_action(app, &ctx, "nmp.publish", &action_json, "corr-id").is_ok(),
            "publish execution should not error"
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
