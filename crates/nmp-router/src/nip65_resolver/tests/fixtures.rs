//! Shared test scaffolding: relay-slot builders, `kind:10002` seeding
//! helpers, the `mk_resolver` constructor, and the pubkey/relay constants
//! every scenario in the sibling test modules builds on.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::nip65_resolver::{Nip65OutboxResolver, RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD};
use crate::{InMemoryMailboxCache, Kind10002Parser};
use nmp_core::publish::{RelaySelectionReason, ResolvedRelay};
use nmp_core::slots::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use nmp_core::substrate::{BlockedRelaySet, MailboxCache};
use nmp_store::{RawEvent, VerifiedEvent};

pub(super) const AUTHOR_HEX: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const RECIPIENT_HEX: &str =
    "2222222222222222222222222222222222222222222222222222222222222222";
pub(super) const AUTHOR_WRITE_RELAY: &str = "wss://author-write.example";
pub(super) const RECIPIENT_READ_RELAY: &str = "wss://recipient-read.example";

pub(super) fn no_block() -> BlockedRelaySet {
    BlockedRelaySet::new()
}

pub(super) fn indexer_slot_with(urls: Vec<String>) -> IndexerRelaysSlot {
    let slot = new_indexer_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

pub(super) fn local_write_slot_with(urls: Vec<String>) -> LocalWriteRelaysSlot {
    let slot = new_local_write_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

pub(super) fn urls_of(resolved: &[ResolvedRelay]) -> BTreeSet<String> {
    resolved.iter().map(|r| r.url.clone()).collect()
}

pub(super) fn find_reason<'a>(
    resolved: &'a [ResolvedRelay],
    url: &str,
) -> Option<&'a RelaySelectionReason> {
    resolved.iter().find(|r| r.url == url).map(|r| &r.reason)
}

pub(super) fn seed_kind10002(
    cache: &Arc<InMemoryMailboxCache>,
    author_hex: &str,
    tags: Vec<Vec<String>>,
) {
    let prefix = &author_hex[..2];
    let id = format!("{:0<64}", format!("{}e10002", prefix));
    let raw = RawEvent {
        id,
        pubkey: author_hex.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    Kind10002Parser::new(Arc::clone(cache)).parse_event(&verified);
}

pub(super) fn relay_tag(url: &str, marker: Option<&str>) -> Vec<String> {
    let mut tag = vec!["r".to_string(), url.to_string()];
    if let Some(marker) = marker {
        tag.push(marker.to_string());
    }
    tag
}

pub(super) fn seed_relay(
    cache: &Arc<InMemoryMailboxCache>,
    author_hex: &str,
    url: &str,
    marker: &str,
) {
    seed_kind10002(cache, author_hex, vec![relay_tag(url, Some(marker))]);
}

pub(super) fn mk_resolver(cache: &Arc<InMemoryMailboxCache>) -> Nip65OutboxResolver {
    let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
    Nip65OutboxResolver::new(mailbox_cache, new_indexer_relays_slot())
}

pub(super) fn pk(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

pub(super) fn threshold_recipients() -> Vec<String> {
    let mut recipients = vec![RECIPIENT_HEX.to_string()];
    recipients.extend((0..RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD - 1).map(|i| pk((i + 3) as u8)));
    recipients
}
