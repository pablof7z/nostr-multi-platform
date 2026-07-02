//! Feed Worker control tests.

use super::*;

fn active_follows_feed_params_json(feed_key: &str) -> String {
    serde_json::json!({
        "primary_kinds": [1],
        "shape": "RootIndexed",
        "source": "ActiveUserFollows",
        "admission": "All",
        "order": "NewestByFeedPosition",
        "window": {
            "initial_limit": 20,
            "page_size": 20,
            "source_page_size": 20
        },
        "key": feed_key,
        "item_projection": "FeedRows"
    })
    .to_string()
}

#[test]
fn feed_open_json_returns_handle_and_close_tears_down_session() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());

    let resp = core.handle_json_request(
        &serde_json::json!({
            "type": "feed_open_json",
            "params_json": active_follows_feed_params_json("app.web.feed.test"),
            "correlation_id": "feed-open-1"
        })
        .to_string(),
    );
    let events: serde_json::Value = serde_json::from_str(&resp).expect("valid feed events");
    assert_eq!(events[0]["type"], "feed_opened", "resp={resp}");
    assert_eq!(events[0]["correlation_id"], "feed-open-1", "resp={resp}");
    let feed_handle: nmp_feed::FeedHandle =
        serde_json::from_value(events[0]["handle"].clone()).expect("FeedHandle JSON");
    assert_eq!(
        core.handle.as_ref().unwrap().feed_sessions.live_count(),
        1,
        "feed_open_json must register one live feed session"
    );

    let load_resp = core.handle_json_request(
        &serde_json::json!({
            "type": "feed_load_older",
            "handle": &feed_handle,
            "correlation_id": "feed-load-1"
        })
        .to_string(),
    );
    let load_events: serde_json::Value =
        serde_json::from_str(&load_resp).expect("valid load events");
    assert_eq!(
        load_events[0]["type"], "feed_load_status",
        "resp={load_resp}"
    );
    assert_eq!(load_events[0]["correlation_id"], "feed-load-1");
    assert_eq!(load_events[0]["status"]["changed"], false);
    assert_eq!(
        load_events[0]["status"]["reason"], "source_unavailable",
        "active-follows feed without an active account fails closed"
    );

    let close_resp = core.handle_json_request(
        &serde_json::json!({
            "type": "feed_close",
            "handle": &feed_handle,
            "correlation_id": "feed-close-1"
        })
        .to_string(),
    );
    assert!(close_resp.contains("nmp.feed.close"), "resp={close_resp}");
    assert_eq!(
        core.handle.as_ref().unwrap().feed_sessions.live_count(),
        0,
        "feed_close must tear down the live feed session"
    );
}

#[test]
fn feed_load_older_unknown_handle_returns_typed_status() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());

    let load_resp = core.handle_json_request(
        &serde_json::json!({
            "type": "feed_load_older",
            "handle": {
                "projection_key": "app.web.feed.missing",
                "session_id": 9999
            },
            "correlation_id": "feed-load-missing"
        })
        .to_string(),
    );
    let events: serde_json::Value = serde_json::from_str(&load_resp).expect("valid load events");
    assert_eq!(events[0]["type"], "feed_load_status", "resp={load_resp}");
    assert_eq!(events[0]["status"]["changed"], false);
    assert_eq!(events[0]["status"]["reason"], "session_unavailable");
}

#[test]
fn feed_open_json_rejects_invalid_params_without_registering_session() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());

    let resp = core.handle_json_request(
        &serde_json::json!({
            "type": "feed_open_json",
            "params_json": "{\"primary_kinds\":",
            "correlation_id": "feed-open-bad"
        })
        .to_string(),
    );
    assert!(resp.contains("feed_params_rejected"), "resp={resp}");
    assert_eq!(
        core.handle.as_ref().unwrap().feed_sessions.live_count(),
        0,
        "invalid FeedParams JSON must not register a session"
    );
}
