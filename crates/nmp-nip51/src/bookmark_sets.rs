//! NIP-51 bookmark and curation sets (kind:30003 / kind:30004).
//!
//! These are addressable lists keyed by `d`. The projection is deliberately
//! raw: author, kind, identifier, metadata, and public list item tags. Product
//! concepts such as libraries, vaults, following policy, grouping, or ranking
//! belong in the consuming app crate.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, KernelEvent,
};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::{KIND_ARTICLE_CURATION_SET, KIND_BOOKMARK_SET};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::bookmarks::{
    action_rejection_message, item_key, nonempty_option, nonempty_trimmed, normalize_item,
    tag_to_item, BookmarkItem, BookmarkListMetadata,
};

/// NIP-51 set kinds this module projects and writes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BookmarkSetKind {
    /// kind:30003 bookmark set.
    BookmarkSet,
    /// kind:30004 article/note curation set.
    CurationSet,
}

impl BookmarkSetKind {
    #[must_use]
    pub fn kind(self) -> u32 {
        match self {
            Self::BookmarkSet => KIND_BOOKMARK_SET,
            Self::CurationSet => KIND_ARTICLE_CURATION_SET,
        }
    }
}

impl TryFrom<u32> for BookmarkSetKind {
    type Error = ();

    fn try_from(kind: u32) -> Result<Self, Self::Error> {
        match kind {
            KIND_BOOKMARK_SET => Ok(Self::BookmarkSet),
            KIND_ARTICLE_CURATION_SET => Ok(Self::CurationSet),
            _ => Err(()),
        }
    }
}

/// One addressable NIP-51 bookmark/curation set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BookmarkSetSnapshot {
    pub author: String,
    pub set_kind: BookmarkSetKind,
    pub identifier: String,
    pub event_id: String,
    pub created_at: u64,
    pub items: Vec<BookmarkItem>,
    pub metadata: BookmarkListMetadata,
}

/// Snapshot of all set events currently delivered to the projection.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct BookmarkSetsSnapshot {
    pub sets: Vec<BookmarkSetSnapshot>,
}

#[derive(Clone, Debug)]
struct SetEntry {
    created_at: u64,
    snapshot: BookmarkSetSnapshot,
}

/// Projects kind:30003 bookmark sets and kind:30004 curation sets.
pub struct BookmarkSetsProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    sets: Mutex<BTreeMap<SetKey, SetEntry>>,
}

type SetKey = (String, u32, String);

impl BookmarkSetsProjection {
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            sets: Mutex::new(BTreeMap::new()),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> BookmarkSetsSnapshot {
        let Ok(sets) = self.sets.lock() else {
            return BookmarkSetsSnapshot::default();
        };
        BookmarkSetsSnapshot {
            sets: sets.values().map(|entry| entry.snapshot.clone()).collect(),
        }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| serde_json::json!({ "sets": [] }))
    }

    #[must_use]
    pub fn snapshot_for_authors<I, S>(&self, authors: I) -> BookmarkSetsSnapshot
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let authors: BTreeSet<String> = authors.into_iter().map(Into::into).collect();
        let Ok(sets) = self.sets.lock() else {
            return BookmarkSetsSnapshot::default();
        };
        BookmarkSetsSnapshot {
            sets: sets
                .values()
                .filter(|entry| authors.contains(&entry.snapshot.author))
                .map(|entry| entry.snapshot.clone())
                .collect(),
        }
    }

    #[must_use]
    pub fn snapshot_for_set(
        &self,
        author: &str,
        set_kind: BookmarkSetKind,
        identifier: &str,
    ) -> Option<BookmarkSetSnapshot> {
        let Ok(sets) = self.sets.lock() else {
            return None;
        };
        sets.get(&set_key(author, set_kind, identifier))
            .map(|entry| entry.snapshot.clone())
    }

    fn snapshot_for_write(
        &self,
        account_pubkey: &str,
        set_kind: BookmarkSetKind,
        identifier: &str,
    ) -> BookmarkSetSnapshot {
        self.snapshot_for_set(account_pubkey, set_kind, identifier)
            .unwrap_or_else(|| BookmarkSetSnapshot {
                author: account_pubkey.to_string(),
                set_kind,
                identifier: identifier.to_string(),
                event_id: String::new(),
                created_at: 0,
                items: Vec::new(),
                metadata: BookmarkListMetadata::default(),
            })
    }

    fn ensure_active_account(&self, account_pubkey: &str) -> Result<(), ActionRejection> {
        let active = self
            .active_pubkey
            .lock()
            .map_err(|_| ActionRejection::Invalid("bookmark set state unavailable".to_string()))?
            .as_ref()
            .cloned();
        match active {
            Some(active) if active == account_pubkey => Ok(()),
            Some(active) => Err(ActionRejection::Unauthorized(format!(
                "bookmark set action account {account_pubkey} does not match active account {active}"
            ))),
            None => Err(ActionRejection::Unauthorized(
                "bookmark set action requires an active account".to_string(),
            )),
        }
    }
}

impl ObservedProjectionSink for BookmarkSetsProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Ok(set_kind) = BookmarkSetKind::try_from(event.kind) else {
            return;
        };
        let Some(snapshot) = parse_bookmark_set(event, set_kind) else {
            return;
        };
        let key = set_key(&snapshot.author, snapshot.set_kind, &snapshot.identifier);
        let Ok(mut sets) = self.sets.lock() else {
            return;
        };
        if sets
            .get(&key)
            .is_some_and(|entry| event.created_at < entry.created_at)
        {
            return;
        }
        sets.insert(
            key,
            SetEntry {
                created_at: event.created_at,
                snapshot,
            },
        );
    }
}

/// Build a kind:30003 or kind:30004 unsigned event from a set snapshot.
#[must_use]
pub fn build_bookmark_set_event(snapshot: &BookmarkSetSnapshot) -> UnsignedEvent {
    let mut tags = Vec::with_capacity(snapshot.items.len() + 4);
    tags.push(vec!["d".to_string(), snapshot.identifier.clone()]);
    if let Some(title) = nonempty_option(snapshot.metadata.title.as_deref()) {
        tags.push(vec!["title".to_string(), title.to_string()]);
    }
    if let Some(image) = nonempty_option(snapshot.metadata.image.as_deref()) {
        tags.push(vec!["image".to_string(), image.to_string()]);
    }
    if let Some(description) = nonempty_option(snapshot.metadata.description.as_deref()) {
        tags.push(vec!["description".to_string(), description.to_string()]);
    }
    for item in &snapshot.items {
        tags.push(item.to_tag());
    }
    UnsignedEvent {
        pubkey: String::new(),
        kind: snapshot.set_kind.kind(),
        tags,
        content: String::new(),
        created_at: 0,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BookmarkSetUpdateInput {
    pub account_pubkey: String,
    pub set_kind: BookmarkSetKind,
    pub identifier: String,
    pub item: BookmarkItem,
}

pub struct AddBookmarkSetItemAction {
    projection: Arc<BookmarkSetsProjection>,
}

impl AddBookmarkSetItemAction {
    #[must_use]
    pub fn new(projection: Arc<BookmarkSetsProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for AddBookmarkSetItemAction {
    const NAMESPACE: &'static str = "nmp.nip51.add_bookmark_set_item";
    type Action = BookmarkSetUpdateInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let input = normalize_update_input(&action).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&input.account_pubkey)?;
        let snapshot = self.projection.snapshot_for_write(
            &input.account_pubkey,
            input.set_kind,
            &input.identifier,
        );
        if snapshot
            .items
            .iter()
            .any(|candidate| item_key(candidate) == item_key(&input.item))
        {
            return Err(ActionRejection::Conflict(
                "bookmark set item is already present".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let input = normalize_update_input(&action)?;
        self.projection
            .ensure_active_account(&input.account_pubkey)
            .map_err(action_rejection_message)?;
        let mut snapshot = self.projection.snapshot_for_write(
            &input.account_pubkey,
            input.set_kind,
            &input.identifier,
        );
        snapshot.items.push(input.item);
        snapshot.items = dedupe_items(snapshot.items);
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_bookmark_set_event(&snapshot),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub struct RemoveBookmarkSetItemAction {
    projection: Arc<BookmarkSetsProjection>,
}

impl RemoveBookmarkSetItemAction {
    #[must_use]
    pub fn new(projection: Arc<BookmarkSetsProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for RemoveBookmarkSetItemAction {
    const NAMESPACE: &'static str = "nmp.nip51.remove_bookmark_set_item";
    type Action = BookmarkSetUpdateInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let input = normalize_update_input(&action).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&input.account_pubkey)?;
        let snapshot = self.projection.snapshot_for_write(
            &input.account_pubkey,
            input.set_kind,
            &input.identifier,
        );
        if !snapshot
            .items
            .iter()
            .any(|candidate| item_key(candidate) == item_key(&input.item))
        {
            return Err(ActionRejection::Conflict(
                "bookmark set item is not present".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let input = normalize_update_input(&action)?;
        self.projection
            .ensure_active_account(&input.account_pubkey)
            .map_err(action_rejection_message)?;
        let mut snapshot = self.projection.snapshot_for_write(
            &input.account_pubkey,
            input.set_kind,
            &input.identifier,
        );
        let key = item_key(&input.item);
        snapshot
            .items
            .retain(|candidate| item_key(candidate) != key);
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_bookmark_set_event(&snapshot),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub fn register_bookmark_set_actions(
    app: &mut impl ActionRegistrar,
    projection: Arc<BookmarkSetsProjection>,
) {
    app.register_default_action(AddBookmarkSetItemAction::new(Arc::clone(&projection)));
    app.register_default_action(RemoveBookmarkSetItemAction::new(projection));
}

fn parse_bookmark_set(
    event: &KernelEvent,
    set_kind: BookmarkSetKind,
) -> Option<BookmarkSetSnapshot> {
    let identifier = event
        .tags
        .iter()
        .find(|tag| tag.first().is_some_and(|name| name == "d"))
        .and_then(|tag| tag.get(1))
        .and_then(|value| nonempty_trimmed(value))?
        .to_string();

    let mut metadata = BookmarkListMetadata::default();
    let mut items = Vec::new();
    for tag in &event.tags {
        match tag.first().map(String::as_str) {
            Some("title") => {
                metadata.title = metadata.title.take().or_else(|| tag.get(1).cloned());
            }
            Some("image") => {
                metadata.image = metadata.image.take().or_else(|| tag.get(1).cloned());
            }
            Some("description") => {
                metadata.description = metadata.description.take().or_else(|| tag.get(1).cloned());
            }
            Some("e") | Some("a") | Some("r") | Some("t") => {
                if let Some(item) = tag_to_item(tag) {
                    items.push(item);
                }
            }
            _ => {}
        }
    }

    Some(BookmarkSetSnapshot {
        author: event.author.clone(),
        set_kind,
        identifier,
        event_id: event.id.clone(),
        created_at: event.created_at,
        items: dedupe_items(items),
        metadata,
    })
}

fn normalize_update_input(
    input: &BookmarkSetUpdateInput,
) -> Result<BookmarkSetUpdateInput, String> {
    let identifier = nonempty_trimmed(&input.identifier)
        .ok_or_else(|| "bookmark set identifier must be non-empty".to_string())?
        .to_string();
    Ok(BookmarkSetUpdateInput {
        account_pubkey: input.account_pubkey.clone(),
        set_kind: input.set_kind,
        identifier,
        item: normalize_item(&input.item)?,
    })
}

fn dedupe_items(items: Vec<BookmarkItem>) -> Vec<BookmarkItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item_key(item)))
        .collect()
}

fn set_key(author: &str, set_kind: BookmarkSetKind, identifier: &str) -> SetKey {
    (
        author.to_string(),
        set_kind.kind(),
        identifier.trim().to_string(),
    )
}

#[cfg(test)]
#[path = "bookmark_sets/tests.rs"]
mod tests;
