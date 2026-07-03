pub(crate) struct TestStoreContactListReader {
    store: std::sync::Arc<dyn crate::store::EventStore>,
}

impl TestStoreContactListReader {
    pub(crate) fn new(store: std::sync::Arc<dyn crate::store::EventStore>) -> Self {
        Self { store }
    }

    fn latest_event(&self, author_hex: &str) -> Option<crate::slots::ContactListEvent> {
        let author = crate::kernel::hex_to_pubkey_bytes(author_hex)?;
        let mut iter = self
            .store
            .scan_by_author_kind(&author, &[crate::kinds::KIND_CONTACT_LIST], None, None, 1)
            .ok()?;
        iter.next()?
            .ok()
            .map(|stored| crate::slots::ContactListEvent {
                tags: stored.raw.tags.clone(),
                content: stored.raw.content.clone(),
                created_at: stored.raw.created_at,
            })
    }
}

impl crate::slots::ContactListReader for TestStoreContactListReader {
    fn follows(&self, author_hex: &str) -> Option<Vec<String>> {
        self.latest_event(author_hex)
            .map(|event| test_contact_follows(&event.tags))
    }

    fn event_for_edit(&self, author_hex: &str) -> Option<crate::slots::ContactListEvent> {
        self.latest_event(author_hex)
    }

    fn draft_after_add(
        &self,
        author_hex: &str,
        current: &crate::slots::ContactListEvent,
        target: &str,
        created_at: u64,
    ) -> Option<crate::slots::ContactListDraft> {
        let mut tags = current.tags.clone();
        let already_present = tags.iter().any(|tag| {
            tag.first().map(String::as_str) == Some("p")
                && tag.get(1).map(String::as_str) == Some(target)
        });
        if !already_present {
            tags.push(vec!["p".to_string(), target.to_string()]);
        }
        Some(crate::slots::ContactListDraft {
            pubkey: author_hex.to_string(),
            kind: crate::kinds::KIND_CONTACT_LIST,
            tags,
            content: current.content.clone(),
            created_at,
        })
    }

    fn draft_after_remove(
        &self,
        author_hex: &str,
        current: &crate::slots::ContactListEvent,
        target: &str,
        created_at: u64,
    ) -> Option<crate::slots::ContactListDraft> {
        Some(crate::slots::ContactListDraft {
            pubkey: author_hex.to_string(),
            kind: crate::kinds::KIND_CONTACT_LIST,
            tags: current
                .tags
                .iter()
                .filter(|tag| {
                    !(tag.first().map(String::as_str) == Some("p")
                        && tag.get(1).map(String::as_str) == Some(target))
                })
                .cloned()
                .collect(),
            content: current.content.clone(),
            created_at,
        })
    }

    fn initial_draft(
        &self,
        author_hex: &str,
        follows: &[String],
        created_at: u64,
    ) -> Option<crate::slots::ContactListDraft> {
        Some(crate::slots::ContactListDraft {
            pubkey: author_hex.to_string(),
            kind: crate::kinds::KIND_CONTACT_LIST,
            tags: follows
                .iter()
                .map(|pubkey| vec!["p".to_string(), pubkey.clone()])
                .collect(),
            content: String::new(),
            created_at,
        })
    }
}

fn test_contact_follows(tags: &[Vec<String>]) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) == Some("p") {
                tag.get(1)
                    .filter(|value| crate::kernel::is_hex_pubkey(value))
                    .cloned()
            } else {
                None
            }
        })
        .collect()
}
