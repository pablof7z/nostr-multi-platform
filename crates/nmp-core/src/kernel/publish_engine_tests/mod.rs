//! T117 integration tests — kernel publish path goes through `PublishEngine`.
//!
//! These tests drive the kernel's engine seam directly:
//! - The engine's `Nip65OutboxResolver` resolves relays from the kernel's
//!   event store. A kind:10002 for the author is seeded via `seed_kind10002`
//!   so `Nip65OutboxResolver` has real NIP-65 write relays to route to.
//!   (T-publish-resolver-indexer / codex f81f735: the old indexer-fallback
//!   path is removed — an author with no kind:10002 produces `NoTargets`, not
//!   a silent publish to arbitrary public relays.)
//! - The engine pushes per-relay `EVENT` frames into the `QueueDispatcher`,
//!   which the kernel drains into `OutboundMessage`s.
//! - OK frames are folded back via `Kernel::handle_publish_ok_at` (the
//!   time-injected variant; the wire path calls `handle_publish_ok` which
//!   reads `SystemTime::now()`).
//! - Retries fire on `tick_publish_engine(now_ms)`.
//!
//! Time is injected throughout (`now_ms` deterministic), no sockets, no
//! sleeps. The four bullets the spec calls out:
//! 1. Successful multi-relay publish: engine settles each per-relay to Ok →
//!    snapshot `recent_ok` carries the relay set.
//! 2. AUTH-REQUIRED on one relay, OK on the other: the auth relay PARKS
//!    (availability gate, no retry budget) until it reaches `Authenticated`,
//!    then re-dispatches and settles; untouched relay stays Ok.
//! 3. Transient failure × 3: 1s backoff → 4s backoff → give-up;
//!    `FailedAfterRetries` row appears on the snapshot.
//! 4. Restart with a Pending row: build a second Kernel sharing the same
//!    `Arc<dyn PublishStore>`; engine resumes via `resume_publish_engine`.

mod chokepoint;
mod t117;
mod t127;
mod user_actions;

use std::sync::Arc;

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{SignedEvent, UnsignedEvent};

/// T117 test relay URLs — two explicit write relays declared in kind:10002
/// (replaces the old `FALLBACK_R1/R2` indexer-fallback constants; these are
/// now NIP-65-routed, not fallback-routed).
pub(super) const WRITE_R1: &str = "wss://write-r1.test";
pub(super) const WRITE_R2: &str = "wss://write-r2.test";

pub(super) fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{}", id),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

/// Seed a kind:10002 into the kernel's event store for `author_pubkey` with
/// `write_urls` as its write-marker relay tags. Required so
/// `Nip65OutboxResolver` has real NIP-65 data and does not return `NoTargets`.
pub(super) fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    let tags: Vec<Vec<String>> = write_urls
        .iter()
        .map(|url| vec!["r".to_string(), url.to_string(), "write".to_string()])
        .collect();
    // Use the author pubkey as the event id — guaranteed valid hex (64 hex
    // chars) and unique per author in a fresh-kernel test.  The old two-char
    // prefix approach embedded a literal 'k' which is not a valid hex
    // character; V-70 strengthened `is_structurally_valid()` to check hex
    // chars, so those synthetic events were rejected as Malformed and never
    // entered the store (mirrors the canonical `seed_kind10002_for_test`).
    let id = author_pubkey.to_string();
    let raw = RawEvent {
        id,
        pubkey: author_pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel
        .store
        .insert(verified, &"wss://seed".to_string(), 1_700_000_000_000)
        .expect("seed_kind10002 insert");
}

pub(super) fn ok_payload<'a>(
    event_id: &'a str,
    accepted: bool,
    reason: &'a str,
) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message: reason,
    }
}

/// Helper for the boot-resume test: `handle_publish_ok_at` needs a `now_ms`
/// strictly past the engine's most recent recorded ack timestamp, otherwise
/// `apply_ack`'s late-ack idempotence path would discard the OK as stale.
/// `resume_publish_engine` uses wall-clock `now_epoch_ms()`, so this returns
/// the same wall-clock time the engine already saw.
pub(super) fn now_ms_after_resume(_signed: &SignedEvent) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build a persisted publish record directly in the store — simulating a row
/// written before the fix (or by a hostile/legacy path) that bypassed the
/// entry gate. `reasons` controls whether each relay was an explicit pin.
pub(super) fn persist_pending_record(
    store: &Arc<dyn PublishStore>,
    signed: &SignedEvent,
    relay: &str,
    reason: crate::publish::RelaySelectionReason,
) {
    use crate::publish::{PerRelayState, PublishRecord};
    let record = PublishRecord {
        handle: signed.id.clone(),
        event: signed.clone(),
        per_relay: vec![(relay.to_string(), PerRelayState::Pending)],
        pending_retries: Vec::new(),
        relay_reasons: vec![(relay.to_string(), vec![reason])],
    };
    store.upsert(&record).expect("seed persisted publish row");
}
