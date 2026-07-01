//! Test-support NIP-65 parser adapter.
//!
//! The relay-list rules live in `nmp-nip65-types`; this adapter only bridges
//! those parsed wire tags into the kernel's test mailbox-cache trait object.

use std::sync::Arc;

use crate::store::VerifiedEvent;
use crate::substrate::{IngestParser, MailboxCache, ParsedRelayList};

pub struct TestNip65RelayListParser {
    cache: Arc<dyn MailboxCache>,
}

impl TestNip65RelayListParser {
    #[must_use]
    pub fn new(cache: Arc<dyn MailboxCache>) -> Self {
        Self { cache }
    }
}

impl IngestParser for TestNip65RelayListParser {
    fn parse(&self, evt: &VerifiedEvent) {
        let raw = evt.raw();
        let Some(parsed) = nmp_nip65_types::parse_event_tags(raw.kind, &raw.tags) else {
            return;
        };
        if parsed.is_empty() {
            self.cache.remove(&raw.pubkey);
        } else {
            self.cache.upsert(
                raw.pubkey.clone(),
                ParsedRelayList {
                    read: parsed.read,
                    write: parsed.write,
                    both: parsed.both,
                },
            );
        }
    }
}
