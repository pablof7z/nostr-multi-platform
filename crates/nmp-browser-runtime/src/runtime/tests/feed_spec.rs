use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::start_test_browser_builder;

const RELAY: &str = "wss://relay.example";

#[test]
fn browser_feed_spec_requires_source_before_opening_session() {
    let mut handle = start_test_browser_builder(
        BrowserAppBuilder::new()
            .in_memory()
            .consume_all_builtin_projections()
            .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
            .decide_providers(BrowserRunConfig::default()),
    );

    let opened = handle.feeds().open_spec(
        nmp_feed::FeedKey::app("test.browser.feed.invalid").unwrap(),
        nmp_feed::feed::events().primary_kinds([nmp_kinds::KIND_SHORT_TEXT_NOTE]),
    );

    assert!(opened.is_none());
    assert_eq!(handle.feed_sessions.live_count(), 0);
}
