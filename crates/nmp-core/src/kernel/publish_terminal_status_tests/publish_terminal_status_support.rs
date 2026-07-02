//! Shared fixtures for the T128 terminal-status tests: a fake signed-event
//! builder, an `OkFramePayload` builder, kind:10002 mailbox seeding (so
//! `Nip65OutboxResolver` resolves write relays), and the queue-entry lookup
//! both sibling test modules assert against.

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

/// T128 test relay URLs — declared as NIP-65 write relays in kind:10002.
pub(super) const WRITE_R1: &str = "wss://t128-write-r1.test";
pub(super) const WRITE_R2: &str = "wss://t128-write-r2.test";

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

pub(super) fn ok_payload<'a>(event_id: &'a str, accepted: bool, reason: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message: reason,
    }
}

/// Seed parsed kind:10002 mailbox facts for `author_pubkey`. Required by
/// T-publish-resolver-indexer: without a kind:10002 the resolver returns empty
/// (NoTargets).
pub(super) fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    kernel.seed_kind10002_for_test(author_pubkey, write_urls);
}

/// Helper: locate the queue entry for `event_id` in the kernel's snapshot.
/// Panics if missing — every T128 test pushes one entry before asserting.
pub(super) fn entry_for<'a>(kernel: &'a Kernel, event_id: &str) -> &'a crate::kernel::PublishQueueEntry {
    kernel
        .publish_queue_snapshot()
        .iter()
        .find(|e| e.event_id == event_id)
        .expect("queue entry must exist for the publish under test")
}
