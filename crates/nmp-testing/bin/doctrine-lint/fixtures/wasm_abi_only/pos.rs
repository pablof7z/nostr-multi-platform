// Positive fixture: violating ABI module (violations of WASM_ABI_ONLY rule).
// This file should be flagged by the wasm_abi_only linter.

use nmp_router::OutboxRouter;
use nmp_signers::Signer;
use nmp_nip65::Nip65Resolver;
use apps::chirp::ChirpConfig;

pub struct WorkerABI {
    outbox_resolver: Box<dyn OutboxRouter>,
    signer_kind: u32,
}

impl WorkerABI {
    pub fn new(config: ChirpConfig) -> Self {
        Self {
            outbox_resolver: Box::new(MockRouter),
            signer_kind: 0,
        }
    }

    pub fn route_to(&self, target: &str) {
        // This function name is forbidden vocabulary.
    }

    pub fn nip65_lookup(&self) -> Box<dyn Nip65Resolver> {
        Box::new(MockResolver)
    }

    pub fn publish_target(&self) -> String {
        // publish_target is forbidden policy vocabulary
        String::new()
    }
}

struct MockRouter;
impl OutboxRouter for MockRouter {
    fn resolve(&self, _pubkey: &str) -> Vec<String> {
        vec![]
    }
}

struct MockResolver;
impl Nip65Resolver for MockResolver {
    fn resolve(&self, _pubkey: &str) -> Vec<(String, String)> {
        vec![]
    }
}
