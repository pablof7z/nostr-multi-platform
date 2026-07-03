use std::sync::Arc;

use crate::substrate::{
    ContentParser, DraftBuildContext, DraftBuildError, DraftBuilderRegistry, DraftIntent,
    MailboxCache, OutboxRouter,
};

use super::Kernel;

impl Kernel {
    /// #2937: flips [`Kernel::router_composed`] to `true`. Every real
    /// caller — production composition (`NmpApp::set_routing_substrate`)
    /// and every in-tree test that exercises real routing — calls this
    /// exactly once before issuing routing decisions, so the flag faithfully
    /// tracks "was a real router ever installed" rather than guessing from
    /// behaviour.
    pub fn set_routing(&mut self, router: Arc<dyn OutboxRouter>, cache: Arc<dyn MailboxCache>) {
        self.outbox_router = router;
        self.mailbox_cache = cache;
        self.router_composed = true;
    }

    pub fn set_content_parser(&mut self, parser: Arc<dyn ContentParser>) {
        self.content_parser = parser;
    }

    pub fn set_draft_builder_registry(&mut self, registry: Arc<DraftBuilderRegistry>) {
        self.draft_builders = registry;
    }

    pub fn register_draft_builder(
        &self,
        kind: crate::substrate::DraftIntentKind,
        builder: Arc<dyn crate::substrate::DraftBuilder>,
    ) {
        self.draft_builders.register(kind, builder);
    }

    pub fn build_draft(
        &self,
        intent: &DraftIntent,
        author_pubkey: &str,
        created_at: u64,
    ) -> Result<nmp_signer_iface::UnsignedEvent, DraftBuildError> {
        self.draft_builders.build(
            intent,
            DraftBuildContext {
                event_store: &*self.event_store_handle(),
                author_pubkey,
                created_at,
            },
        )
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
