use std::collections::HashMap;
use std::sync::Arc;

use nmp_core::testing::ConformanceHarness;
use nmp_router::Nip65RelayListPublishSupport;

#[test]
fn create_account_publishes_router_built_kind10002_to_cold_start_relays() {
    let mut h = ConformanceHarness::new();
    h.set_relay_list_publish_support(Arc::new(Nip65RelayListPublishSupport));

    let relays = vec![
        ("wss://nip65-write.test".to_string(), "write".to_string()),
        ("wss://nip65-read.test".to_string(), "read".to_string()),
    ];
    let mut profile = HashMap::new();
    profile.insert("display_name".to_string(), "Marcus Webb".to_string());
    h.create_account(profile, &relays, &[]);

    let event = h
        .published_event_of_kind(10002)
        .expect("router support must preserve cold-start kind:10002 publish");
    let tags = event["tags"].as_array().expect("tags array");
    assert!(tags.iter().any(|tag| tag.as_array().is_some_and(|parts| {
        parts.first().and_then(|v| v.as_str()) == Some("r")
            && parts.get(1).and_then(|v| v.as_str()) == Some("wss://nip65-write.test")
            && parts.get(2).and_then(|v| v.as_str()) == Some("write")
    })));
    assert!(tags.iter().any(|tag| tag.as_array().is_some_and(|parts| {
        parts.first().and_then(|v| v.as_str()) == Some("r")
            && parts.get(1).and_then(|v| v.as_str()) == Some("wss://nip65-read.test")
            && parts.get(2).and_then(|v| v.as_str()) == Some("read")
    })));
    assert_eq!(
        h.last_error_toast(),
        None,
        "router-owned cold-start target resolution must provide publish targets"
    );
}
