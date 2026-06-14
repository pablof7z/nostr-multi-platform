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
}
