//! Active account NIP-51 kind:10003 bookmark-list facts and writes.
//!
//! The kind:10003 list is a normal replaceable NIP-51 list. This module keeps
//! the protocol facts raw: event ids, address coordinates, web URLs, hashtags,
//! and list metadata. App-specific grouping, vault semantics, labels, and UI
//! language belong in the consuming app crate.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, KernelEvent,
};
use nmp_signer_iface::UnsignedEvent;
use nmp_core::{canonical_relay_url, KernelEventObserver};
use nmp_core::actor::{ActorCommand};
use nmp_core::actor::{PublishCommand};
use nmp_kinds::KIND_BOOKMARK_LIST;
use serde::{Deserialize, Serialize};

/// NIP-51 metadata tags carried by a bookmark list.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct BookmarkListMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One raw public item from a kind:10003 bookmark list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BookmarkItem {
    /// `["e", <event-id>, <optional-relay>]`.
    Event {
        event_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
    },
    /// `["a", <kind:pubkey:d>, <optional-relay>]`.
    Address {
        coordinate: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
    },
    /// `["r", <http-or-https-url>]`.
    Url { url: String },
    /// `["t", <hashtag>]`.
    Hashtag { hashtag: String },
}

/// Snapshot shape for the active account's global NIP-51 bookmarks.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct BookmarkListSnapshot {
    pub items: Vec<BookmarkItem>,
    pub metadata: BookmarkListMetadata,
}

#[derive(Default)]
struct BookmarkListState {
    owner_pubkey: Option<String>,
    created_at: u64,
    snapshot: BookmarkListSnapshot,
}

/// Projects the active account's kind:10003 bookmark list.
pub struct BookmarkListProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    state: Mutex<BookmarkListState>,
}

impl BookmarkListProjection {
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            state: Mutex::new(BookmarkListState::default()),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> BookmarkListSnapshot {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return BookmarkListSnapshot::default(),
        };
        let Ok(state) = self.state.lock() else {
            return BookmarkListSnapshot::default();
        };
        if state.owner_pubkey.as_deref() != active.as_deref() {
            return BookmarkListSnapshot::default();
        }
        state.snapshot.clone()
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "items": [], "metadata": {} }))
    }

    fn snapshot_for_account(&self, account_pubkey: &str) -> BookmarkListSnapshot {
        let Ok(state) = self.state.lock() else {
            return BookmarkListSnapshot::default();
        };
        if state.owner_pubkey.as_deref() != Some(account_pubkey) {
            return BookmarkListSnapshot::default();
        }
        state.snapshot.clone()
    }

    fn ensure_active_account(&self, account_pubkey: &str) -> Result<(), ActionRejection> {
        let active = self
            .active_pubkey
            .lock()
            .map_err(|_| ActionRejection::Invalid("bookmark list state unavailable".to_string()))?
            .as_ref()
            .cloned();
        match active {
            Some(active) if active == account_pubkey => Ok(()),
            Some(active) => Err(ActionRejection::Unauthorized(format!(
                "bookmark action account {account_pubkey} does not match active account {active}"
            ))),
            None => Err(ActionRejection::Unauthorized(
                "bookmark action requires an active account".to_string(),
            )),
        }
    }
}

impl KernelEventObserver for BookmarkListProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_BOOKMARK_LIST {
            return;
        }

        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return,
        };
        if active.as_deref() != Some(event.author.as_str()) {
            return;
        }

        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.owner_pubkey.as_deref() == Some(event.author.as_str())
            && event.created_at < state.created_at
        {
            return;
        }
        *state = BookmarkListState {
            owner_pubkey: Some(event.author.clone()),
            created_at: event.created_at,
            snapshot: parse_bookmark_list(event),
        };
    }
}

/// Build a kind:10003 unsigned event from a bookmark snapshot.
#[must_use]
pub fn build_bookmark_list_event(snapshot: &BookmarkListSnapshot) -> UnsignedEvent {
    let mut tags = Vec::with_capacity(snapshot.items.len() + 3);
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
        kind: KIND_BOOKMARK_LIST,
        tags,
        content: String::new(),
        created_at: 0,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BookmarkUpdateInput {
    pub account_pubkey: String,
    pub item: BookmarkItem,
}

pub struct AddBookmarkAction {
    projection: Arc<BookmarkListProjection>,
}

impl AddBookmarkAction {
    #[must_use]
    pub fn new(projection: Arc<BookmarkListProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for AddBookmarkAction {
    const NAMESPACE: &'static str = "nmp.nip51.add_bookmark";
    type Action = BookmarkUpdateInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let item = normalize_item(&action.item).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)?;
        if snapshot_contains(
            &self.projection.snapshot_for_account(&action.account_pubkey),
            &item,
        ) {
            return Err(ActionRejection::Conflict(
                "bookmark item is already present in kind:10003".to_string(),
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
        let item = normalize_item(&action.item)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)
            .map_err(action_rejection_message)?;
        let mut snapshot = self.projection.snapshot_for_account(&action.account_pubkey);
        snapshot.items.push(item);
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_bookmark_list_event(&dedupe_snapshot(snapshot)),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub struct RemoveBookmarkAction {
    projection: Arc<BookmarkListProjection>,
}

impl RemoveBookmarkAction {
    #[must_use]
    pub fn new(projection: Arc<BookmarkListProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for RemoveBookmarkAction {
    const NAMESPACE: &'static str = "nmp.nip51.remove_bookmark";
    type Action = BookmarkUpdateInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let item = normalize_item(&action.item).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)?;
        if !snapshot_contains(
            &self.projection.snapshot_for_account(&action.account_pubkey),
            &item,
        ) {
            return Err(ActionRejection::Conflict(
                "bookmark item is not present in kind:10003".to_string(),
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
        let item = normalize_item(&action.item)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)
            .map_err(action_rejection_message)?;
        let mut snapshot = self.projection.snapshot_for_account(&action.account_pubkey);
        snapshot
            .items
            .retain(|candidate| !same_item(candidate, &item));
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_bookmark_list_event(&snapshot),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub fn register_bookmark_actions(
    app: &mut impl ActionRegistrar,
    projection: Arc<BookmarkListProjection>,
) {
    app.register_default_action(AddBookmarkAction::new(Arc::clone(&projection)));
    app.register_default_action(RemoveBookmarkAction::new(projection));
}

fn parse_bookmark_list(event: &KernelEvent) -> BookmarkListSnapshot {
    let mut metadata = BookmarkListMetadata::default();
    let mut items = Vec::new();
    for tag in &event.tags {
        match tag.first().map(String::as_str) {
            Some("title") => metadata.title = metadata.title.take().or_else(|| tag.get(1).cloned()),
            Some("image") => metadata.image = metadata.image.take().or_else(|| tag.get(1).cloned()),
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
    dedupe_snapshot(BookmarkListSnapshot { items, metadata })
}

fn tag_to_item(tag: &[String]) -> Option<BookmarkItem> {
    let value = tag.get(1)?;
    match tag.first().map(String::as_str)? {
        "e" if is_hex64(value) => Some(BookmarkItem::Event {
            event_id: value.to_ascii_lowercase(),
            relay: tag.get(2).and_then(|relay| canonical_relay_url(relay)),
        }),
        "a" if valid_address_coordinate(value) => Some(BookmarkItem::Address {
            coordinate: normalize_address_coordinate(value),
            relay: tag.get(2).and_then(|relay| canonical_relay_url(relay)),
        }),
        "r" if valid_http_url(value) => Some(BookmarkItem::Url {
            url: value.trim().to_string(),
        }),
        "t" => normalize_hashtag(value).map(|hashtag| BookmarkItem::Hashtag { hashtag }),
        _ => None,
    }
}

fn normalize_item(item: &BookmarkItem) -> Result<BookmarkItem, String> {
    match item {
        BookmarkItem::Event { event_id, relay } => {
            if !is_hex64(event_id) {
                return Err("event bookmark requires a 64-hex event_id".to_string());
            }
            Ok(BookmarkItem::Event {
                event_id: event_id.to_ascii_lowercase(),
                relay: normalize_optional_relay(relay)?,
            })
        }
        BookmarkItem::Address { coordinate, relay } => {
            if !valid_address_coordinate(coordinate) {
                return Err(
                    "address bookmark requires a kind:pubkey:d address coordinate".to_string(),
                );
            }
            Ok(BookmarkItem::Address {
                coordinate: normalize_address_coordinate(coordinate),
                relay: normalize_optional_relay(relay)?,
            })
        }
        BookmarkItem::Url { url } => {
            if !valid_http_url(url) {
                return Err("URL bookmark requires an http:// or https:// URL".to_string());
            }
            Ok(BookmarkItem::Url {
                url: url.trim().to_string(),
            })
        }
        BookmarkItem::Hashtag { hashtag } => normalize_hashtag(hashtag)
            .map(|hashtag| BookmarkItem::Hashtag { hashtag })
            .ok_or_else(|| "hashtag bookmark requires a non-empty hashtag".to_string()),
    }
}

fn normalize_optional_relay(relay: &Option<String>) -> Result<Option<String>, String> {
    match relay.as_deref().and_then(nonempty_trimmed) {
        Some(raw) => canonical_relay_url(raw)
            .map(Some)
            .ok_or_else(|| "bookmark relay hint must be a ws:// or wss:// URL".to_string()),
        None => Ok(None),
    }
}

fn dedupe_snapshot(mut snapshot: BookmarkListSnapshot) -> BookmarkListSnapshot {
    let mut seen = HashSet::new();
    snapshot.items.retain(|item| seen.insert(item_key(item)));
    snapshot
}

fn snapshot_contains(snapshot: &BookmarkListSnapshot, item: &BookmarkItem) -> bool {
    snapshot
        .items
        .iter()
        .any(|candidate| same_item(candidate, item))
}

fn same_item(left: &BookmarkItem, right: &BookmarkItem) -> bool {
    item_key(left) == item_key(right)
}

fn action_rejection_message(rejection: ActionRejection) -> String {
    match rejection {
        ActionRejection::Invalid(message)
        | ActionRejection::Unauthorized(message)
        | ActionRejection::Conflict(message) => message,
        ActionRejection::InvalidCoded { message, .. } => message,
    }
}

fn item_key(item: &BookmarkItem) -> (u8, String) {
    match item {
        BookmarkItem::Event { event_id, .. } => (0, event_id.to_ascii_lowercase()),
        BookmarkItem::Address { coordinate, .. } => (1, normalize_address_coordinate(coordinate)),
        BookmarkItem::Url { url } => (2, url.trim().to_string()),
        BookmarkItem::Hashtag { hashtag } => (3, hashtag.trim_start_matches('#').to_string()),
    }
}

impl BookmarkItem {
    fn to_tag(&self) -> Vec<String> {
        match self {
            Self::Event { event_id, relay } => tag_with_optional_relay("e", event_id, relay),
            Self::Address { coordinate, relay } => tag_with_optional_relay("a", coordinate, relay),
            Self::Url { url } => vec!["r".to_string(), url.clone()],
            Self::Hashtag { hashtag } => vec!["t".to_string(), hashtag.clone()],
        }
    }
}

fn tag_with_optional_relay(kind: &str, value: &str, relay: &Option<String>) -> Vec<String> {
    let mut tag = vec![kind.to_string(), value.to_string()];
    if let Some(relay) = relay {
        tag.push(relay.clone());
    }
    tag
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_address_coordinate(value: &str) -> bool {
    let mut parts = value.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(kind), Some(pubkey), Some(identifier)) => {
            kind.parse::<u32>().is_ok() && is_hex64(pubkey) && !identifier.is_empty()
        }
        _ => false,
    }
}

fn normalize_address_coordinate(value: &str) -> String {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next().unwrap_or_default();
    let pubkey = parts.next().unwrap_or_default().to_ascii_lowercase();
    let identifier = parts.next().unwrap_or_default();
    format!("{kind}:{pubkey}:{identifier}")
}

fn valid_http_url(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("https://") || trimmed.starts_with("http://")
}

fn normalize_hashtag(value: &str) -> Option<String> {
    let hashtag = nonempty_trimmed(value)?.trim_start_matches('#');
    (!hashtag.is_empty()).then(|| hashtag.to_string())
}

fn nonempty_trimmed(value: &str) -> Option<&str> {
    nonempty_option(Some(value.trim()))
}

fn nonempty_option(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| (!value.trim().is_empty()).then_some(value.trim()))
}

#[cfg(test)]
#[path = "bookmarks/tests.rs"]
mod tests;
