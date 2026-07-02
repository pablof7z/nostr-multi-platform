//! Shared routing-context builders and the `AttemptCapture` trace observer
//! every V-75 scenario in the sibling test modules builds on.

use std::sync::Mutex;

use crate::router::*;
use nmp_core::substrate::{BlockedRelaySet, MailboxCache, RouteAttempt, SessionKeySet};
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, InterestShape};

pub(super) fn pubkey() -> String {
    "alice".into()
}

pub(super) fn unsigned_evt() -> UnsignedEvent {
    UnsignedEvent {
        pubkey: pubkey(),
        kind: 1,
        tags: vec![],
        content: String::new(),
        created_at: 0,
    }
}

pub(super) fn interest_for(authors: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(0),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|s| (*s).into()).collect(),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

pub(super) fn ctx_app_only<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
    app_relays: &'a [String],
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            app_relays,
            ..SessionKeySet::default()
        },
        mailbox_cache: cache,
        blocked_relays: blocked,
    }
}

pub(super) fn ctx_nip65_only<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet::default(),
        mailbox_cache: cache,
        blocked_relays: blocked,
    }
}

/// Test observer that captures the full `PublishTrace` / `SubscriptionTrace`
/// including the V-75 `attempts` field.
#[derive(Default)]
pub(super) struct AttemptCapture {
    pub(super) publish_attempts: Mutex<Vec<Vec<RouteAttempt>>>,
    pub(super) subscription_attempts: Mutex<Vec<Vec<RouteAttempt>>>,
}

impl RoutingTraceObserver for AttemptCapture {
    fn on_publish(&self, summary: PublishTrace, _routed: &RoutedRelaySet) {
        self.publish_attempts.lock().unwrap().push(summary.attempts);
    }
    fn on_subscription(&self, summary: SubscriptionTrace, _routed: &RoutedRelaySet) {
        self.subscription_attempts
            .lock()
            .unwrap()
            .push(summary.attempts);
    }
}
