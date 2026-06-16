use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use nmp_core::publish::{NoopOutboxResolver, OutboxResolver};
use nmp_core::store::VerifiedEvent;
use nmp_core::substrate::{
    empty_blocked_relay_lookup, empty_contacts_lookup, empty_dm_inbox_relay_lookup,
    empty_profile_lookup, EmptyOutboxRouter, IngestParser, MailboxCache, OutboxRouter,
    RelayConnectedHook, RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor,
    TestInMemoryMailboxCache,
};
use nmp_core::{Disposition, OutboundMessage, TypedProjectionData};

use crate::{nmp_app_consume_all_builtin_projections, nmp_app_free, nmp_app_new, nmp_app_start};

struct AppHandle(*mut crate::NmpApp);

impl AppHandle {
    fn new() -> Self {
        Self(nmp_app_new())
    }

    fn app(&self) -> &crate::NmpApp {
        // SAFETY: the handle is allocated by nmp_app_new and freed in Drop.
        unsafe { &*self.0 }
    }

    fn start(&self) {
        nmp_app_consume_all_builtin_projections(self.0);
        nmp_app_start(self.0, 0, 0, 0);
    }
}

impl Drop for AppHandle {
    fn drop(&mut self) {
        nmp_app_free(self.0);
    }
}

struct NoopParser;

impl IngestParser for NoopParser {
    fn parse(&self, _evt: &VerifiedEvent) {}
}

struct NoopReqInterceptor;

impl ReqFrameInterceptor for NoopReqInterceptor {
    fn intercept_req(
        &self,
        _kernel: &mut nmp_core::Kernel,
        _ctx: &ReqFrameContext,
    ) -> Option<Vec<OutboundMessage>> {
        None
    }
}

struct NoopRelayTextInterceptor;

impl RelayTextInterceptor for NoopRelayTextInterceptor {
    fn on_relay_text(
        &self,
        _kernel: &mut nmp_core::Kernel,
        _relay_url: &str,
        _text: &str,
    ) -> Vec<OutboundMessage> {
        Vec::new()
    }
}

struct NoopRelayConnectedHook;

impl RelayConnectedHook for NoopRelayConnectedHook {
    fn on_relay_connected(
        &self,
        _relay_url: &str,
        _is_reconnect: bool,
        _command_sender: nmp_core::CommandSender,
    ) {
    }
}

fn disposition_name(disposition: Disposition) -> &'static str {
    match disposition {
        Disposition::Installed => "Installed",
        Disposition::ReplacedPrevious => "ReplacedPrevious",
        Disposition::YieldedToExisting => "YieldedToExisting",
        Disposition::DroppedLateWiring => "DroppedLateWiring",
        Disposition::AppliedLive => "AppliedLive",
    }
}

fn disposition_for(app: &crate::NmpApp, seam: &str, key: &str) -> Option<String> {
    let report = app.composition_ledger().to_json();
    report
        .get("records")?
        .as_array()?
        .iter()
        .rev()
        .find(|record| {
            record.get("seam").and_then(|value| value.as_str()) == Some(seam)
                && record.get("key").and_then(|value| value.as_str()) == Some(key)
        })
        .and_then(|record| record.get("disposition"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn assert_disposition(app: &crate::NmpApp, seam: &str, key: &str, expected: Disposition) {
    assert_eq!(
        disposition_for(app, seam, key),
        Some(disposition_name(expected).to_string()),
        "missing or wrong disposition for {seam}/{key}"
    );
}

fn typed_projection(key: &str) -> TypedProjectionData {
    TypedProjectionData {
        key: key.to_string(),
        schema_id: "test.schema".to_string(),
        schema_version: 1,
        file_identifier: "TEST".to_string(),
        payload: vec![1, 2, 3, 4],
        ..Default::default()
    }
}

#[test]
fn post_start_snapshot_registrations_are_recorded_as_live_applied() {
    let handle = AppHandle::new();
    handle.start();
    let app = handle.app();

    app.register_snapshot_projection("late.generic", || serde_json::json!({"ok": true}));
    app.register_typed_snapshot_projection("late.typed", || Some(typed_projection("late.typed")));

    assert_disposition(
        app,
        "snapshot_projection",
        "late.generic",
        Disposition::AppliedLive,
    );
    assert_disposition(
        app,
        "typed_snapshot_projection",
        "late.typed",
        Disposition::AppliedLive,
    );
    assert!(
        app.run_snapshot_projections_for_test()
            .contains_key("late.generic"),
        "live generic projection must be visible to the shared registry"
    );
    assert!(
        app.run_typed_snapshot_projections_for_test()
            .iter()
            .any(|projection| projection.key == "late.typed"),
        "live typed projection must be visible to the shared registry"
    );
}

#[test]
fn post_start_ingest_parser_registration_is_a_true_drop() {
    let handle = AppHandle::new();
    handle.start();
    let app = handle.app();
    let before = app
        .ingest_dispatcher_slot
        .read()
        .expect("dispatcher lock")
        .registration_count();

    app.register_ingest_parser(424_242, Arc::new(NoopParser));

    let after = app
        .ingest_dispatcher_slot
        .read()
        .expect("dispatcher lock")
        .registration_count();
    assert_eq!(
        after, before,
        "late init-only parser registration must not mutate the live dispatcher"
    );
    assert_disposition(
        app,
        "ingest_parser",
        "kind:424242",
        Disposition::DroppedLateWiring,
    );
}

#[test]
fn post_start_next_reset_slots_are_true_drops() {
    let handle = AppHandle::new();
    handle.start();
    let app = handle.app();

    app.set_coverage_hook(Arc::new(|_| {}));
    app.set_req_frame_interceptor(Arc::new(NoopReqInterceptor));
    app.set_routing_substrate(|_observer| {
        let router: Arc<dyn OutboxRouter> = Arc::new(EmptyOutboxRouter::new());
        let cache: Arc<dyn MailboxCache> = Arc::new(TestInMemoryMailboxCache::new());
        (router, cache)
    });
    app.set_publish_resolver_factory(|_, _, _, _| {
        Arc::new(NoopOutboxResolver) as Arc<dyn OutboxResolver>
    });
    app.set_raw_event_forward_policy_factory(|_| Vec::new());
    app.set_bootstrap_self_kinds(Some(vec![0]));
    app.set_nostrconnect_bootstrap_relay("wss://late.example".to_string());

    assert!(
        app.coverage_hook
            .lock()
            .expect("coverage hook lock")
            .is_none(),
        "late coverage hook must not be staged for a future reset"
    );
    assert!(
        app.req_frame_interceptor
            .lock()
            .expect("req interceptor lock")
            .is_none(),
        "late req interceptor must not be staged for a future reset"
    );
    assert!(
        app.routing_substrate
            .lock()
            .expect("routing substrate lock")
            .is_none(),
        "late routing substrate must not be staged for a future reset"
    );
    assert!(
        app.publish_resolver
            .lock()
            .expect("publish resolver lock")
            .is_none(),
        "late publish resolver must not be staged for a future reset"
    );
    assert!(
        app.raw_event_forward_policy
            .lock()
            .expect("raw event policy lock")
            .is_none(),
        "late raw event forward policy must not be staged for a future reset"
    );
    assert!(
        app.bootstrap_self_kinds
            .lock()
            .expect("bootstrap self kinds lock")
            .is_none(),
        "late bootstrap self kinds must not be staged for a future reset"
    );
    assert!(
        app.nostrconnect_bootstrap_relay
            .lock()
            .expect("nostrconnect relay lock")
            .is_none(),
        "late Nostr Connect bootstrap relay must not be staged for a future reset"
    );
    assert_disposition(
        app,
        "coverage_hook",
        "coverage_hook",
        Disposition::DroppedLateWiring,
    );
    assert_disposition(
        app,
        "req_frame_interceptor",
        "req_frame_interceptor",
        Disposition::DroppedLateWiring,
    );
    assert_disposition(
        app,
        "routing_substrate",
        "routing_substrate",
        Disposition::DroppedLateWiring,
    );
    assert_disposition(
        app,
        "publish_resolver_factory",
        "publish_resolver_factory",
        Disposition::DroppedLateWiring,
    );
    assert_disposition(
        app,
        "raw_event_forward_policy_factory",
        "raw_event_forward_policy_factory",
        Disposition::DroppedLateWiring,
    );
    assert_disposition(
        app,
        "bootstrap_self_kinds",
        "bootstrap_self_kinds",
        Disposition::DroppedLateWiring,
    );
    assert_disposition(
        app,
        "nostrconnect_bootstrap_relay",
        "nostrconnect_bootstrap_relay",
        Disposition::DroppedLateWiring,
    );
}

#[test]
fn host_init_projection_and_substrate_seams_are_ledger_recorded() {
    let handle = AppHandle::new();
    let app = handle.app();

    let gate: Arc<dyn nmp_core::ChangeGate> = Arc::new(AtomicU64::new(1));
    app.register_snapshot_projection_gated("pre.gated", gate, || serde_json::json!({}));
    app.register_typed_snapshot_projection("pre.typed", || Some(typed_projection("pre.typed")));
    app.register_snapshot_tick_observer(|| {});
    app.set_req_frame_interceptor(Arc::new(NoopReqInterceptor));
    app.add_relay_text_interceptor(Arc::new(NoopRelayTextInterceptor));
    app.add_relay_connected_hook(Arc::new(NoopRelayConnectedHook));
    app.set_publish_resolver_factory(|_, _, _, _| {
        Arc::new(NoopOutboxResolver) as Arc<dyn OutboxResolver>
    });
    app.set_raw_event_forward_policy_factory(|_| Vec::new());
    app.set_dm_inbox_relay_lookup(empty_dm_inbox_relay_lookup());
    app.set_profile_lookup(empty_profile_lookup());
    app.set_contacts_lookup(empty_contacts_lookup());
    app.set_blocked_relay_lookup(empty_blocked_relay_lookup());
    app.set_mailbox_cache_reader(Arc::new(TestInMemoryMailboxCache::new()));
    app.set_routing_substrate(|_observer| {
        let router: Arc<dyn OutboxRouter> = Arc::new(EmptyOutboxRouter::new());
        let cache: Arc<dyn MailboxCache> = Arc::new(TestInMemoryMailboxCache::new());
        (router, cache)
    });
    app.set_bootstrap_self_kinds(Some(vec![0, 3]));
    app.set_nostrconnect_bootstrap_relay("wss://bootstrap.example".to_string());

    for (seam, key) in [
        ("snapshot_projection_gated", "pre.gated"),
        ("typed_snapshot_projection", "pre.typed"),
        ("snapshot_tick_observer", "snapshot_tick_observer"),
        ("req_frame_interceptor", "req_frame_interceptor"),
        ("relay_text_interceptor", "relay_text_interceptor"),
        ("relay_connected_hook", "relay_connected_hook"),
        ("publish_resolver_factory", "publish_resolver_factory"),
        (
            "raw_event_forward_policy_factory",
            "raw_event_forward_policy_factory",
        ),
        ("dm_inbox_relay_lookup", "dm_inbox_relay_lookup"),
        ("profile_lookup", "profile_lookup"),
        ("contacts_lookup", "contacts_lookup"),
        ("blocked_relay_lookup", "blocked_relay_lookup"),
        ("mailbox_cache_reader", "mailbox_cache_reader"),
        ("routing_substrate", "routing_substrate"),
        ("bootstrap_self_kinds", "bootstrap_self_kinds"),
        (
            "nostrconnect_bootstrap_relay",
            "nostrconnect_bootstrap_relay",
        ),
    ] {
        assert_disposition(app, seam, key, Disposition::Installed);
    }

    // Use the gate after registration so the test proves the value is a real
    // ChangeGate object, not a dead cast that only satisfied the type checker.
    let rev = app
        .snapshot_projections
        .lock()
        .expect("snapshot registry lock")
        .run()
        .contains_key("pre.gated");
    assert!(rev, "gated snapshot projection should be registered");
}
