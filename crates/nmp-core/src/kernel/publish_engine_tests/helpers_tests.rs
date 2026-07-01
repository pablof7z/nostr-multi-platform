#![cfg(test)]
//! Shared test helpers for `publish_engine_tests`.
//!
//! Constants and functions used across the sibling `*_tests.rs` files. Kept
//! in a separate file so `mod.rs` can be a pure module-declaration hub and
//! doctrine-lint's `file_is_test_only` check exempts this file by its
//! `_tests.rs` suffix.

use std::sync::Arc;

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::publish::PublishStore;
use crate::store::{RawEvent, VerifiedEvent};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

/// T117 test relay URLs — two explicit write relays declared in kind:10002
/// (replaces the old `FALLBACK_R1/R2` indexer-fallback constants; these are
/// now NIP-65-routed, not fallback-routed).
pub(crate) const WRITE_R1: &str = "wss://write-r1.test";
pub(crate) const WRITE_R2: &str = "wss://write-r2.test";

pub(crate) fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
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
pub(crate) fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
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
    kernel.seed_mailbox_relay_list(
        author_pubkey,
        Vec::new(),
        write_urls.iter().map(|url| (*url).to_string()).collect(),
        Vec::new(),
    );
}

pub(crate) fn ok_payload<'a>(
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
/// `resume_publish_engine` uses the kernel wall-clock seam, so this returns
/// the same wall-clock domain the engine already saw.
pub(crate) fn now_ms_after_resume(_signed: &SignedEvent) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build a persisted publish record directly in the store — simulating a row
/// written before the fix (or by a hostile/legacy path) that bypassed the
/// entry gate. `reasons` controls whether each relay was an explicit pin.
pub(crate) fn persist_pending_record(
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
