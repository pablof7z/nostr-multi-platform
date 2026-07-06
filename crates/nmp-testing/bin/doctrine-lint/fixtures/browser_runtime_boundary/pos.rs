// Positive fixture: violating browser-runtime transport adapter.
// This file should be flagged by the browser_runtime_boundary linter.

use std::sync::Arc;

/// Worker with policy routing logic — VIOLATION.
pub struct WorkerDispatch {
    outbox_resolver: Box<dyn OutboxResolver>,
    routing_rules: Vec<RoutingRule>,
}

impl WorkerDispatch {
    pub fn new() -> Self {
        Self {
            outbox_resolver: Box::new(MockResolver),
            routing_rules: vec![],
        }
    }

    /// Routes to outbox — policy forbidden in transport adapter.
    pub fn route_to_outbox(&self, pubkey: &str) -> Vec<String> {
        self.outbox_resolver.resolve_outbox(pubkey)
    }

    /// Direct Nip65 routing — policy forbidden.
    pub fn use_nip65_relays(&self) -> Vec<String> {
        let resolver = Nip65Resolver::new();
        vec![]
    }

    /// Manages signer_kind — signer policy forbidden.
    pub fn set_signer_kind(&mut self, kind: u32) {
        // signer_kind management
    }

    /// publish_target logic — routing policy forbidden.
    pub fn compute_publish_target(&self) -> String {
        String::new()
    }
}

pub trait OutboxResolver {
    fn resolve_outbox(&self, pubkey: &str) -> Vec<String>;
}

pub struct RoutingRule {
    destination: String,
}

pub struct MockResolver;

impl OutboxResolver for MockResolver {
    fn resolve_outbox(&self, _pubkey: &str) -> Vec<String> {
        vec![]
    }
}

pub struct Nip65Resolver;

impl Nip65Resolver {
    pub fn new() -> Self {
        Self
    }
}

pub struct mailbox {
    messages: Vec<String>,
}
