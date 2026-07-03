use std::sync::Arc;

/// Draft facts for a reaction publish path.
///
/// The kernel may resolve substrate facts such as the target event author, but
/// it does not own the protocol grammar that turns those facts into wire tags
/// and content. Protocol crates register a builder through this seam.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionDraft {
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// Protocol-owned builder for reaction draft tags/content.
pub trait ReactionDraftBuilder: Send + Sync {
    fn build_reaction_draft(
        &self,
        target_event_id: &str,
        target_author_pubkey: Option<&str>,
        reaction: &str,
    ) -> Option<ReactionDraft>;
}

#[derive(Debug, Default)]
struct EmptyReactionDraftBuilder;

impl ReactionDraftBuilder for EmptyReactionDraftBuilder {
    fn build_reaction_draft(
        &self,
        _target_event_id: &str,
        _target_author_pubkey: Option<&str>,
        _reaction: &str,
    ) -> Option<ReactionDraft> {
        None
    }
}

#[must_use]
pub fn empty_reaction_draft_builder() -> Arc<dyn ReactionDraftBuilder> {
    Arc::new(EmptyReactionDraftBuilder)
}
