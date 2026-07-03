use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::start_test_browser_builder;

const RELAY: &str = "wss://relay.example";

fn browser_handle() -> crate::BrowserRuntimeHandle {
    start_test_browser_builder(
        BrowserAppBuilder::new()
            .in_memory()
            .consume_all_builtin_projections()
            .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
            .decide_providers(BrowserRunConfig::default()),
    )
}

fn tag(term: &str) -> nmp_feed::FeedScope {
    nmp_feed::FeedScope::Tag {
        term: nmp_feed::TagTerm(term.to_string()),
    }
}

fn params(
    key: &str,
    source: nmp_feed::FeedScope,
    admission: nmp_feed::FeedAdmission,
    order: nmp_feed::FeedOrder,
) -> nmp_feed::FeedParams {
    nmp_feed::FeedParams {
        primary_kinds: vec![nmp_kinds::KIND_SHORT_TEXT_NOTE],
        shape: nmp_feed::FeedShape::Flat,
        source,
        admission,
        order,
        window: nmp_feed::FeedWindowPolicy::bounded(16),
        key: nmp_feed::FeedKey::app(key).unwrap(),
        item_projection: nmp_feed::FeedItemProjection::feed_rows(),
    }
}

fn assert_open_fails(handle: &mut crate::BrowserRuntimeHandle, params: nmp_feed::FeedParams) {
    assert!(
        handle.feeds().open(params).is_none(),
        "browser custom policy open must fail closed"
    );
    assert_eq!(
        handle.feed_sessions.live_count(),
        0,
        "failed custom policy open must not leak a session"
    );
}

#[test]
fn browser_custom_feed_policy_registry_opens_registered_feed() {
    let mut handle = browser_handle();
    let source_id = nmp_feed::CustomSourceId("test.browser.source".into());
    let admission_id = nmp_feed::CustomAdmissionId("test.browser.admission".into());
    let order_id = nmp_feed::CustomOrderId("test.browser.order".into());

    assert!(handle.feeds().register_custom_source(
        source_id.clone(),
        nmp_feed::CustomSourceDef::new(tag("rust")),
    ));
    assert!(handle.feeds().register_custom_admission(
        admission_id.clone(),
        nmp_feed::CustomAdmissionDef::new(tag("nmp")),
    ));
    assert!(handle.feeds().register_custom_order(
        order_id.clone(),
        nmp_feed::CustomOrderDef::new(nmp_feed::FeedOrder::NewestByFeedPosition),
    ));

    assert!(handle.custom_source(&source_id).is_some());
    assert!(handle.custom_admission(&admission_id).is_some());
    assert!(handle.custom_order(&order_id).is_some());
    assert_eq!(handle.custom_feed_policy_count(), 3);

    let opened = handle
        .feeds()
        .open(params(
            "test.browser.custom.open",
            nmp_feed::FeedScope::CustomSource(source_id),
            nmp_feed::FeedAdmission::Custom(admission_id),
            nmp_feed::FeedOrder::Custom(order_id),
        ))
        .expect("registered custom source/admission/order opens");

    assert_eq!(opened.projection_key.as_str(), "test.browser.custom.open");
    assert_eq!(handle.feed_sessions.live_count(), 1);
    assert!(handle.feeds().close(&opened));
    assert_eq!(handle.feed_sessions.live_count(), 0);
}

#[test]
fn browser_unregistered_custom_policy_ids_fail_closed_by_phase() {
    let mut handle = browser_handle();

    assert_open_fails(
        &mut handle,
        params(
            "test.browser.custom.unregistered.source",
            nmp_feed::FeedScope::CustomSource(nmp_feed::CustomSourceId("missing.source".into())),
            nmp_feed::FeedAdmission::All,
            nmp_feed::FeedOrder::NewestByFeedPosition,
        ),
    );
    assert_open_fails(
        &mut handle,
        params(
            "test.browser.custom.unregistered.admission",
            tag("rust"),
            nmp_feed::FeedAdmission::Custom(nmp_feed::CustomAdmissionId(
                "missing.admission".into(),
            )),
            nmp_feed::FeedOrder::NewestByFeedPosition,
        ),
    );
    assert_open_fails(
        &mut handle,
        params(
            "test.browser.custom.unregistered.order",
            tag("rust"),
            nmp_feed::FeedAdmission::All,
            nmp_feed::FeedOrder::Custom(nmp_feed::CustomOrderId("missing.order".into())),
        ),
    );
}

#[test]
fn browser_nested_custom_policy_ids_fail_closed_by_phase() {
    let mut handle = browser_handle();
    let nested_source = nmp_feed::CustomSourceId("test.browser.nested.source".into());
    let nested_admission = nmp_feed::CustomAdmissionId("test.browser.nested.admission".into());
    let nested_order = nmp_feed::CustomOrderId("test.browser.nested.order".into());

    assert!(handle.register_custom_source(
        nested_source.clone(),
        nmp_feed::CustomSourceDef::new(nmp_feed::FeedScope::CustomSource(
            nmp_feed::CustomSourceId("test.browser.nested.source.inner".into()),
        )),
    ));
    assert!(handle.register_custom_admission(
        nested_admission.clone(),
        nmp_feed::CustomAdmissionDef::new(nmp_feed::FeedScope::CustomSource(
            nmp_feed::CustomSourceId("test.browser.nested.admission.inner".into()),
        )),
    ));
    assert!(handle.register_custom_order(
        nested_order.clone(),
        nmp_feed::CustomOrderDef::new(nmp_feed::FeedOrder::Custom(nmp_feed::CustomOrderId(
            "test.browser.nested.order.inner".into(),
        ))),
    ));

    assert_open_fails(
        &mut handle,
        params(
            "test.browser.custom.nested.source",
            nmp_feed::FeedScope::CustomSource(nested_source),
            nmp_feed::FeedAdmission::All,
            nmp_feed::FeedOrder::NewestByFeedPosition,
        ),
    );
    assert_open_fails(
        &mut handle,
        params(
            "test.browser.custom.nested.admission",
            tag("rust"),
            nmp_feed::FeedAdmission::Custom(nested_admission),
            nmp_feed::FeedOrder::NewestByFeedPosition,
        ),
    );
    assert_open_fails(
        &mut handle,
        params(
            "test.browser.custom.nested.order",
            tag("rust"),
            nmp_feed::FeedAdmission::All,
            nmp_feed::FeedOrder::Custom(nested_order),
        ),
    );
}
