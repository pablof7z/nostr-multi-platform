use std::sync::Arc;

use crate::substrate::{ContentParser, MailboxCache, OutboxRouter};

use super::Kernel;

impl Kernel {
    pub fn set_routing(&mut self, router: Arc<dyn OutboxRouter>, cache: Arc<dyn MailboxCache>) {
        self.outbox_router = router;
        self.mailbox_cache = cache;
    }

    pub fn set_content_parser(&mut self, parser: Arc<dyn ContentParser>) {
        self.content_parser = parser;
    }

    /// Test seam for delivering a raw event through the genuine post-store
    /// projection path.
    ///
    /// This bypasses signature verification but not parser dispatch:
    /// `project_accepted_event` fans the event to the registered ingest parser
    /// for `kind`, then runs the kernel-owned derived effects such as the
    /// active-account contacts transition.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn project_raw_event_for_test(
        &mut self,
        id: &str,
        pubkey: &str,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: &str,
    ) {
        let verified = crate::store::VerifiedEvent::from_raw_unchecked(crate::store::RawEvent {
            id: id.to_string(),
            pubkey: pubkey.to_string(),
            created_at,
            kind,
            tags,
            content: content.to_string(),
            sig: "a".repeat(128),
        });
        self.project_accepted_event(&verified);
    }
}
