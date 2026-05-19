//! Trait shims for dependencies that have not landed yet.
//!
//! The publish engine is built against these traits so the implementations
//! from #43 (Signer), #46 (RelayManager), M2 (NIP-65 outbox resolver), and M3
//! (LMDB store) can swap in without rewriting the engine. Each trait is
//! intentionally minimal; richer surfaces ship inside the milestones that
//! own them.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::action::{PublishHandle, PublishTarget, RelayUrl};
use super::state::{PerRelayState, RelayAck};
use crate::substrate::SignedEvent;

// ---------------- Signer (M6 / task #43) ----------------

/// What the publish engine needs from the signer for `AUTH-REQUIRED` retries.
///
/// The full `Signer` trait lands in M6 (sessions + signers + write path). This
/// shim names only the operation the publish engine triggers: produce an
/// `AUTH` event for a given relay challenge.
pub trait Signer: Send + Sync {
    fn sign_auth(&self, challenge: &str, relay_url: &str) -> Result<SignedEvent, SignerError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SignerError {
    Unavailable(String),
    Rejected(String),
}

/// Test-only signer that refuses every AUTH request. Used in tests that
/// exercise non-auth paths.
#[derive(Clone, Debug, Default)]
pub struct NoopSigner;

impl Signer for NoopSigner {
    fn sign_auth(&self, _challenge: &str, _relay_url: &str) -> Result<SignedEvent, SignerError> {
        Err(SignerError::Unavailable("noop signer".to_string()))
    }
}

// ---------------- Outbox resolver (M2 / NIP-65) ----------------

/// Resolve `PublishTarget::Auto` to a concrete relay set per NIP-65.
///
/// The real implementation lives in `nmp-nip65` (folded into M2 per the
/// 2026-05-18 scope adjustments): author kind:10002 write relays union'd
/// with each `#p`-tagged recipient's read relays, falling back to a
/// configurable indexer set when the author has no published list.
pub trait OutboxResolver: Send + Sync {
    fn resolve(
        &self,
        author_pubkey: &str,
        p_tags: &[String],
        target: &PublishTarget,
        kind: u32,
    ) -> BTreeSet<RelayUrl>;
}

/// Test/bootstrap resolver — pure data, no I/O. The kernel uses this when
/// no NIP-65 data is available yet (cold start, no contacts).
#[derive(Clone, Debug, Default)]
pub struct StaticOutbox {
    pub author_writes: HashMap<String, Vec<RelayUrl>>,
    pub p_tag_reads: HashMap<String, Vec<RelayUrl>>,
    pub indexer_fallback: Vec<RelayUrl>,
}

impl OutboxResolver for StaticOutbox {
    fn resolve(
        &self,
        author: &str,
        p_tags: &[String],
        target: &PublishTarget,
        _kind: u32,
    ) -> BTreeSet<RelayUrl> {
        if let PublishTarget::Explicit { relays } = target {
            return relays.iter().cloned().collect();
        }
        let mut out = BTreeSet::new();
        match self.author_writes.get(author) {
            Some(writes) if !writes.is_empty() => out.extend(writes.iter().cloned()),
            _ => out.extend(self.indexer_fallback.iter().cloned()),
        }
        for p in p_tags {
            if let Some(reads) = self.p_tag_reads.get(p) {
                out.extend(reads.iter().cloned());
            }
        }
        out
    }
}

/// Always returns empty — proves "no targets" path in tests.
#[derive(Clone, Debug, Default)]
pub struct NoopOutboxResolver;

impl OutboxResolver for NoopOutboxResolver {
    fn resolve(
        &self,
        _author: &str,
        _p_tags: &[String],
        target: &PublishTarget,
        _kind: u32,
    ) -> BTreeSet<RelayUrl> {
        if let PublishTarget::Explicit { relays } = target {
            return relays.iter().cloned().collect();
        }
        BTreeSet::new()
    }
}

// ---------------- Relay dispatcher (M8 / task #46) ----------------

/// Send a single frame to a single relay. Implementations may be async +
/// websocket-backed (M8 RelayManager) or in-process replay queues (tests).
///
/// Per D7, the dispatcher returns raw transport results; classification +
/// retry policy live in the engine.
pub trait RelayDispatcher: Send + Sync {
    fn dispatch(&self, relay_url: &str, frame: &str) -> Vec<RelayAck>;
}

/// Test-only dispatcher: per-relay scripted ack queue. Each call to
/// `dispatch` for a relay pops the next ack from that relay's queue (or
/// returns a `TimedOut` if the queue is empty, modelling "no response").
#[derive(Default)]
pub struct ReplayDispatcher {
    scripts: Mutex<HashMap<RelayUrl, Vec<RelayAck>>>,
    sent: Mutex<Vec<(RelayUrl, String)>>,
}

impl ReplayDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn script(&self, relay_url: &str, acks: Vec<RelayAck>) {
        self.scripts
            .lock()
            .unwrap()
            .insert(relay_url.to_string(), acks);
    }

    pub fn sent_frames(&self) -> Vec<(RelayUrl, String)> {
        self.sent.lock().unwrap().clone()
    }
}

impl RelayDispatcher for ReplayDispatcher {
    fn dispatch(&self, relay_url: &str, frame: &str) -> Vec<RelayAck> {
        self.sent
            .lock()
            .unwrap()
            .push((relay_url.to_string(), frame.to_string()));
        let mut scripts = self.scripts.lock().unwrap();
        if let Some(queue) = scripts.get_mut(relay_url) {
            if !queue.is_empty() {
                return vec![queue.remove(0)];
            }
        }
        vec![RelayAck::timed_out(relay_url)]
    }
}

/// Production dispatcher seam used by the kernel.
///
/// The publish engine's [`RelayDispatcher::dispatch`] contract is synchronous,
/// but the live wire path is async — the engine emits an `EVENT` frame, the
/// transport dials the relay, an `OK` arrives back later as a `RelayEvent`.
/// `QueueDispatcher` reconciles the two by buffering each frame the engine
/// "sends" and returning an empty `Vec<RelayAck>` synchronously (no
/// pre-classified ack). The kernel drains the buffer after `start_publish` /
/// `tick` and hands the frames to the actor as `OutboundMessage`s; the inbound
/// `OK` frame is folded back in via `PublishEngine::on_ack` (D7 — engine owns
/// classification, dispatcher only reports facts).
///
/// Thread-safe so a single instance can be shared between the kernel and the
/// engine; both are driven by the single actor thread (D4) but the trait
/// bound is `Send + Sync` for the engine's `Arc<dyn RelayDispatcher>` field.
#[derive(Default)]
pub struct QueueDispatcher {
    queued: Mutex<Vec<(RelayUrl, String)>>,
}

impl QueueDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain every queued frame in FIFO order. Returned `(relay_url, frame)`
    /// pairs are ready for the kernel to wrap as `OutboundMessage`s.
    pub fn drain(&self) -> Vec<(RelayUrl, String)> {
        std::mem::take(&mut *self.queued.lock().unwrap())
    }
}

impl RelayDispatcher for QueueDispatcher {
    fn dispatch(&self, relay_url: &str, frame: &str) -> Vec<RelayAck> {
        self.queued
            .lock()
            .unwrap()
            .push((relay_url.to_string(), frame.to_string()));
        // Async path: no synchronous ack. The engine's
        // `dispatch_pending` tolerates an empty ack vector — every relay
        // stays InFlight until the kernel feeds the real OK frame in via
        // `on_ack`.
        Vec::new()
    }
}

// ---------------- Durable store (M3 / LMDB) ----------------

/// Persist publish state so a kernel restart resumes pending publishes.
///
/// The real impl is an LMDB-backed table inside `EventStore` (M3). This shim
/// names the read/write surface the engine needs; the LMDB impl satisfies it
/// without exposing LMDB types here.
pub trait PublishStore: Send + Sync {
    fn upsert(&self, record: &PublishRecord) -> Result<(), PublishStoreError>;
    fn delete(&self, handle: &PublishHandle) -> Result<(), PublishStoreError>;
    fn load_pending(&self) -> Result<Vec<PublishRecord>, PublishStoreError>;
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PublishRecord {
    pub handle: PublishHandle,
    pub event: SignedEvent,
    pub per_relay: Vec<(RelayUrl, PerRelayState)>,
    /// Per-relay scheduled retry deadlines (`relay_url → earliest_retry_ms`).
    /// Persisted so a mid-backoff state survives kernel restart — without
    /// this, a process that died one tick after scheduling a 4-second retry
    /// would lose the backoff and either retry instantly (thundering herd)
    /// or never (silent drop). Defaults to empty so older serialised rows
    /// keep deserialising.
    #[serde(default)]
    pub pending_retries: Vec<(RelayUrl, u64)>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishStoreError {
    NotFound,
    Backend(String),
}

#[derive(Default)]
pub struct InMemoryPublishStore {
    rows: Mutex<HashMap<PublishHandle, PublishRecord>>,
}

impl InMemoryPublishStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PublishStore for InMemoryPublishStore {
    fn upsert(&self, record: &PublishRecord) -> Result<(), PublishStoreError> {
        self.rows
            .lock()
            .unwrap()
            .insert(record.handle.clone(), record.clone());
        Ok(())
    }

    fn delete(&self, handle: &PublishHandle) -> Result<(), PublishStoreError> {
        self.rows.lock().unwrap().remove(handle);
        Ok(())
    }

    fn load_pending(&self) -> Result<Vec<PublishRecord>, PublishStoreError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .values()
            .filter(|record| {
                record
                    .per_relay
                    .iter()
                    .any(|(_, state)| !state.is_terminal())
            })
            .cloned()
            .collect())
    }
}
