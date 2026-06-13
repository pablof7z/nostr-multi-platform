//! Trait shims for dependencies that have not landed yet.
//!
//! The publish engine is built against these traits so the implementations
//! from #43 (Signer), #46 (`RelayManager`), M2 (NIP-65 outbox resolver), and M3
//! (LMDB store) can swap in without rewriting the engine. Each trait is
//! intentionally minimal; richer surfaces ship inside the milestones that
//! own them.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::action::{PublishHandle, PublishTarget, RelayUrl};
use super::state::{PerRelayState, RelayAck};
use crate::substrate::{BlockedRelaySet, SignedEvent};

/// Structured reason a relay was added to a publish set.
///
/// Display-free — human-readable formatting happens only at the kernel
/// projection boundary (`crate::kernel::publish_outbox::format_relay_reason`).
/// Keeping the internal pipeline typed means the resolver never owns
/// English strings, persistence stores a stable enum payload, and tests
/// assert against variants instead of fragile reason strings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RelaySelectionReason {
    /// Relay listed as a `write` (or unmarked) entry in the author's NIP-65
    /// kind:10002 — the canonical write target.
    AuthorWriteRelay,
    /// Active account has no kind:10002 yet; the relay was pulled from local
    /// app configuration as a write fallback.
    LocalConfigRelay,
    /// Discovery indexer fan-out for replaceable / profile / contacts kinds
    /// (kind:0, kind:3, kind:10000–19999). Carries the originating kind so
    /// the projection can render it diagnostically.
    DiscoveryIndexer { kind: u32 },
    /// Recipient inbox relay pulled from a `#p`-tagged author's kind:10002
    /// read list. Carries the recipient hex pubkey verbatim — the kernel
    /// projection formats it as `Inbox relay for <hex pubkey>` and the
    /// shell/display layer is responsible for any abbreviation (D6 — backend
    /// projections never call `display::*` helpers).
    RecipientInbox { pubkey: String },
    /// Caller passed `PublishTarget::Explicit { relays }` — the user or app
    /// chose this relay directly.
    Explicit,
}

/// A relay URL paired with the structured reason it was selected.
///
/// Returned from [`OutboxResolver::resolve`]. The same canonical URL may appear
/// more than once with different reasons (e.g. a relay that is both the
/// author's NIP-65 write relay and a discovery indexer). The publish engine
/// deduplicates by canonical URL and collects distinct reasons into a
/// `Vec<RelaySelectionReason>` per URL; the kernel projection formats each
/// reason into a single English string at the wire boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedRelay {
    pub url: RelayUrl,
    pub reason: RelaySelectionReason,
}

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

impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "signer unavailable: {msg}"),
            Self::Rejected(msg) => write!(f, "signing rejected: {msg}"),
        }
    }
}

impl std::error::Error for SignerError {}

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
/// with small `#p` recipient sets' read relays. Discovery kinds additionally
/// fan out to configured indexers; non-discovery kinds fail closed when the
/// author has no published or local write relay list.
pub trait OutboxResolver: Send + Sync {
    /// Resolve the publish target to a list of relays, each annotated with the
    /// human-readable reason it was selected. The returned `Vec` may contain
    /// the same canonical URL more than once with different reasons — the
    /// engine deduplicates and merges reasons at the call site.
    ///
    /// `blocked` is the active account's kind:10006 blocked-relay set. Every
    /// impl MUST exclude blocked relays from its output — including the
    /// `PublishTarget::Explicit` path (a user-named relay that the account
    /// also blocked is still blocked; blocking is a privacy decision the
    /// resolver must honour unconditionally). This mirrors the subscribe-side
    /// `GenericOutboxRouter`, which filters `blocked_relays.contains` on every
    /// lane. Without this filter the outbox resolver silently leaked the
    /// author's events to relays they explicitly told us never to publish to.
    fn resolve(
        &self,
        author_pubkey: &str,
        p_tags: &[String],
        target: &PublishTarget,
        kind: u32,
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay>;
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
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        if let PublishTarget::Explicit { relays } = target {
            return relays
                .iter()
                .filter(|url| !blocked.contains(url))
                .map(|url| ResolvedRelay {
                    url: url.clone(),
                    reason: RelaySelectionReason::Explicit,
                })
                .collect();
        }
        let mut out: Vec<ResolvedRelay> = Vec::new();
        match self.author_writes.get(author) {
            Some(writes) if !writes.is_empty() => {
                for url in writes {
                    out.push(ResolvedRelay {
                        url: url.clone(),
                        reason: RelaySelectionReason::AuthorWriteRelay,
                    });
                }
            }
            _ => {
                // Indexer fallback for a static stub is closest in spirit to
                // the production `LocalConfigRelay` lane — the relay didn't
                // come from a NIP-65 list, it came from a configured set.
                for url in &self.indexer_fallback {
                    out.push(ResolvedRelay {
                        url: url.clone(),
                        reason: RelaySelectionReason::LocalConfigRelay,
                    });
                }
            }
        }
        // Mirror `nmp_router::Nip65OutboxResolver`'s recipient-inbox fanout
        // threshold so the bootstrap `StaticOutbox` rolls off recipient
        // inboxes on broadcast-ish events the same way the production
        // resolver does. Inlined (rather than re-imported from `nmp-router`)
        // because `nmp-core` cannot depend on `nmp-router` (Layer 3 → Layer
        // 2 would invert the dependency arrow). The canonical constant is
        // `nmp_router::RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD`; changes
        // there must be mirrored here.
        const RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD: usize = 15;
        if p_tags.len() < RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD {
            for p in p_tags {
                if let Some(reads) = self.p_tag_reads.get(p) {
                    for url in reads {
                        out.push(ResolvedRelay {
                            url: url.clone(),
                            reason: RelaySelectionReason::RecipientInbox { pubkey: p.clone() },
                        });
                    }
                }
            }
        }
        // Privacy post-filter: never emit a blocked relay (parity with the
        // subscribe-side router and `Nip65OutboxResolver`).
        out.retain(|r| !blocked.contains(&r.url));
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
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        if let PublishTarget::Explicit { relays } = target {
            return relays
                .iter()
                .filter(|url| !blocked.contains(url))
                .map(|url| ResolvedRelay {
                    url: url.clone(),
                    reason: RelaySelectionReason::Explicit,
                })
                .collect();
        }
        Vec::new()
    }
}

// ---------------- Relay dispatcher (M8 / task #46) ----------------

/// Send a single frame to a single relay. Implementations may be async +
/// websocket-backed (M8 `RelayManager`) or in-process replay queues (tests).
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn script(&self, relay_url: &str, acks: Vec<RelayAck>) {
        // D2: a poisoned mutex must never panic at a shared boundary — recover
        // the inner value instead. A poisoned lock here only means a prior
        // panic occurred while the guard was held; the buffered data is still
        // structurally sound to read/write.
        self.scripts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(relay_url.to_string(), acks);
    }

    #[must_use]
    pub fn sent_frames(&self) -> Vec<(RelayUrl, String)> {
        self.sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl RelayDispatcher for ReplayDispatcher {
    fn dispatch(&self, relay_url: &str, frame: &str) -> Vec<RelayAck> {
        self.sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((relay_url.to_string(), frame.to_string()));
        let mut scripts = self
            .scripts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain every queued frame in FIFO order. Returned `(relay_url, frame)`
    /// pairs are ready for the kernel to wrap as `OutboundMessage`s.
    #[must_use]
    pub fn drain(&self) -> Vec<(RelayUrl, String)> {
        // D2: recover from a poisoned lock rather than panic — this seam is
        // driven by the single actor thread and a panic here would take the
        // kernel down. The queued frames remain a valid `Vec` regardless.
        std::mem::take(
            &mut *self
                .queued
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

impl RelayDispatcher for QueueDispatcher {
    fn dispatch(&self, relay_url: &str, frame: &str) -> Vec<RelayAck> {
        self.queued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    /// Per-relay selection rationale (`relay_url → [reason]`). Persisted at
    /// publish time so the structured "why was this relay targeted?" payload
    /// survives kernel restart and is available to the snapshot projection
    /// without re-running the outbox resolver. The `Vec<RelaySelectionReason>`
    /// shape captures the case where one canonical URL was selected for
    /// multiple reasons (e.g. a relay that is both the author's NIP-65 write
    /// relay AND a discovery indexer). Defaults to empty so older serialised
    /// rows keep deserialising; in that case the relay rows project with an
    /// empty `relay_reason` (the projection's `skip_serializing_if =
    /// "String::is_empty"` keeps the JSON shape).
    #[serde(default)]
    pub relay_reasons: Vec<(RelayUrl, Vec<RelaySelectionReason>)>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishStoreError {
    NotFound,
    Backend(String),
}

impl std::fmt::Display for PublishStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "publish record not found"),
            Self::Backend(msg) => write!(f, "publish store backend error: {msg}"),
        }
    }
}

impl std::error::Error for PublishStoreError {}

#[derive(Default)]
pub struct InMemoryPublishStore {
    rows: Mutex<HashMap<PublishHandle, PublishRecord>>,
}

impl InMemoryPublishStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl PublishStore for InMemoryPublishStore {
    fn upsert(&self, record: &PublishRecord) -> Result<(), PublishStoreError> {
        // D2: poison recovery — never panic at this shared store boundary.
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(record.handle.clone(), record.clone());
        Ok(())
    }

    fn delete(&self, handle: &PublishHandle) -> Result<(), PublishStoreError> {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(handle);
        Ok(())
    }

    fn load_pending(&self) -> Result<Vec<PublishRecord>, PublishStoreError> {
        Ok(self
            .rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
