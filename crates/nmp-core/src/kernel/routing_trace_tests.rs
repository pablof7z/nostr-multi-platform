use super::*;
use crate::substrate::{ClassRoutingPath, EventClass, RoutingSource};

fn pubtrace(kind: u32) -> PublishTrace {
    PublishTrace {
        kind,
        author: "alice".into(),
        event_id_short: None,
        attempts: vec![],
    }
}

fn subtrace(id: u64) -> SubscriptionTrace {
    SubscriptionTrace {
        interest_id: id,
        kinds: vec![1],
        authors_count: 1,
        attempts: vec![],
    }
}

fn routed_one(url: &str) -> RoutedRelaySet {
    let mut r = RoutedRelaySet::new();
    r.add(
        url.into(),
        RoutingSource::ClassRouted {
            class: EventClass::Wiki,
            via: ClassRoutingPath::Nip51,
        },
    );
    r
}

#[test]
fn default_capacity_is_sixty_four() {
    let p = RoutingTraceProjection::new();
    assert_eq!(p.capacity(), DEFAULT_ROUTING_TRACE_CAPACITY);
    assert_eq!(p.capacity(), 64);
}

#[test]
fn capacity_zero_clamps_to_one() {
    let p = RoutingTraceProjection::with_capacity(0);
    assert_eq!(p.capacity(), 1);
}

#[test]
fn publish_ring_buffer_trims_oldest_at_capacity() {
    let p = RoutingTraceProjection::with_capacity(3);
    for k in 0..5u32 {
        p.on_publish(pubtrace(k), &routed_one("wss://r.example"));
    }
    let snap = p.snapshot_publishes();
    assert_eq!(snap.len(), 3);
    let kinds: Vec<u32> = snap.iter().map(|e| e.trace.kind).collect();
    assert_eq!(kinds, vec![2, 3, 4]);
}

#[test]
fn subscription_ring_buffer_trims_oldest_at_capacity() {
    let p = RoutingTraceProjection::with_capacity(2);
    for id in 0..4u64 {
        p.on_subscription(subtrace(id), &routed_one("wss://r.example"));
    }
    let snap = p.snapshot_subscriptions();
    assert_eq!(snap.len(), 2);
    let ids: Vec<u64> = snap.iter().map(|e| e.trace.interest_id).collect();
    assert_eq!(ids, vec![2, 3]);
}

#[test]
fn entries_retain_lane_attribution() {
    let p = RoutingTraceProjection::new();
    p.on_publish(pubtrace(1), &routed_one("wss://r.example"));
    let snap = p.snapshot_publishes();
    assert_eq!(snap.len(), 1);
    let (url, sources) = &snap[0].urls[0];
    assert_eq!(url, "wss://r.example");
    assert!(matches!(
        sources.iter().next().unwrap(),
        RoutingSource::ClassRouted { .. }
    ));
}

#[test]
fn publishes_and_subscriptions_are_independent_streams() {
    let p = RoutingTraceProjection::with_capacity(2);
    p.on_publish(pubtrace(1), &routed_one("wss://r.example"));
    p.on_subscription(subtrace(99), &routed_one("wss://r.example"));
    assert_eq!(p.publishes_len(), 1);
    assert_eq!(p.subscriptions_len(), 1);
}

#[test]
fn empty_projection_snapshots_are_empty_vecs() {
    let p = RoutingTraceProjection::new();
    assert!(p.snapshot_publishes().is_empty());
    assert!(p.snapshot_subscriptions().is_empty());
}

#[test]
fn allocation_contract_documented() {
    fn _accepts_ref<O: RoutingTraceObserver>(_o: &O, r: &RoutedRelaySet) {
        let _ = r;
    }
}

#[test]
fn kernel_routing_trace_captures_publish_with_nip65_lane() {
    use std::sync::Arc;

    use crate::kernel::Kernel;
    use crate::planner::LogicalInterest;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::substrate::{
        BlockedRelaySet, Direction, MailboxCache, OutboxRouter, RoutedRelaySet, RoutingContext,
        RoutingError, RoutingSource, SessionKeySet,
    };
    use nmp_signer_iface::UnsignedEvent;

    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Nip65WriteLaneRouter;
    impl OutboxRouter for Nip65WriteLaneRouter {
        fn route_publish(
            &self,
            evt: &UnsignedEvent,
            ctx: &RoutingContext<'_>,
        ) -> Result<RoutedRelaySet, RoutingError> {
            let writes = ctx
                .mailbox_cache
                .write_relays(&evt.pubkey)
                .ok_or_else(|| RoutingError::Unroutable(evt.pubkey.clone()))?;
            let mut out = RoutedRelaySet::new();
            for url in writes {
                out.add(
                    url,
                    RoutingSource::Nip65 {
                        direction: Direction::Write,
                    },
                );
            }
            if out.is_empty() {
                return Err(RoutingError::Unroutable(evt.pubkey.clone()));
            }
            Ok(out)
        }

        fn route_subscription(
            &self,
            interest: &LogicalInterest,
            _ctx: &RoutingContext<'_>,
        ) -> Result<RoutedRelaySet, RoutingError> {
            let pk = interest
                .shape
                .authors
                .iter()
                .next()
                .cloned()
                .unwrap_or_default();
            Err(RoutingError::Unroutable(pk))
        }
    }

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice.write/"]);
    let cache_arc: Arc<dyn MailboxCache> = kernel.mailbox_cache_arc();
    kernel.set_routing(Arc::new(Nip65WriteLaneRouter), cache_arc);

    let projection = kernel.routing_trace();
    assert_eq!(projection.publishes_len(), 0);

    let evt = UnsignedEvent {
        pubkey: ALICE.into(),
        kind: 1,
        tags: vec![],
        content: String::new(),
        created_at: 0,
    };
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let ctx = RoutingContext {
        active_account: Some(&ALICE.to_string()),
        session_keys: SessionKeySet {
            app_relays: &app,
            ..SessionKeySet::default()
        },
        mailbox_cache: &*kernel.mailbox_cache_arc(),
        blocked_relays: &blocked,
    };

    let routed = kernel.outbox_router().route_publish(&evt, &ctx).unwrap();
    assert!(routed.urls().any(|u| u == "wss://alice.write"));
    projection.on_publish(
        crate::substrate::PublishTrace {
            kind: 1,
            author: ALICE.to_string(),
            event_id_short: crate::substrate::truncate_event_id(None),
            attempts: vec![],
        },
        &routed,
    );

    let snap = projection.snapshot_publishes();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].trace.kind, 1);
    assert_eq!(snap[0].trace.author, ALICE);
    let (url, sources) = &snap[0].urls[0];
    assert_eq!(url, "wss://alice.write");
    assert!(sources.contains(&RoutingSource::Nip65 {
        direction: crate::substrate::Direction::Write,
    }));
}
