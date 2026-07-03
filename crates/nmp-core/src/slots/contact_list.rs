use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactListEvent {
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactListDraft {
    pub pubkey: String,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub created_at: u64,
}

/// Protocol-owned reader/writer for an account's contact/follow state.
pub trait ContactListReader: Send + Sync {
    fn follows(&self, author_hex: &str) -> Option<Vec<String>>;

    fn event_for_edit(&self, author_hex: &str) -> Option<ContactListEvent>;

    fn draft_after_add(
        &self,
        author_hex: &str,
        current: &ContactListEvent,
        target: &str,
        created_at: u64,
    ) -> Option<ContactListDraft>;

    fn draft_after_remove(
        &self,
        author_hex: &str,
        current: &ContactListEvent,
        target: &str,
        created_at: u64,
    ) -> Option<ContactListDraft>;

    fn initial_draft(
        &self,
        author_hex: &str,
        follows: &[String],
        created_at: u64,
    ) -> Option<ContactListDraft>;
}

#[derive(Debug, Default)]
pub struct EmptyContactListReader;

impl ContactListReader for EmptyContactListReader {
    fn follows(&self, _author_hex: &str) -> Option<Vec<String>> {
        None
    }

    fn event_for_edit(&self, _author_hex: &str) -> Option<ContactListEvent> {
        None
    }

    fn draft_after_add(
        &self,
        _author_hex: &str,
        _current: &ContactListEvent,
        _target: &str,
        _created_at: u64,
    ) -> Option<ContactListDraft> {
        None
    }

    fn draft_after_remove(
        &self,
        _author_hex: &str,
        _current: &ContactListEvent,
        _target: &str,
        _created_at: u64,
    ) -> Option<ContactListDraft> {
        None
    }

    fn initial_draft(
        &self,
        _author_hex: &str,
        _follows: &[String],
        _created_at: u64,
    ) -> Option<ContactListDraft> {
        None
    }
}

#[must_use]
pub fn empty_contact_list_reader() -> Arc<dyn ContactListReader> {
    Arc::new(EmptyContactListReader)
}
