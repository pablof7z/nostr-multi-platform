//! NIP-B0 web bookmarks (kind:39701).
//!
//! A web bookmark is an addressable event keyed by a scheme-less `d` tag. This
//! module projects the raw bookmark facts and provides one safe publish action
//! for upserting the active account's bookmark event.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, KernelEvent,
};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_WEB_BOOKMARK;
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::bookmarks::{action_rejection_message, nonempty_option, normalize_hashtag};

/// One projected kind:39701 web bookmark.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WebBookmarkSnapshot {
    pub author: String,
    pub url_without_scheme: String,
    pub event_id: String,
    pub created_at: u64,
    pub title: Option<String>,
    pub description: String,
    pub published_at: Option<u64>,
    pub hashtags: Vec<String>,
}

/// Snapshot of all web bookmark events currently delivered to the projection.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WebBookmarksSnapshot {
    pub bookmarks: Vec<WebBookmarkSnapshot>,
}

#[derive(Clone, Debug)]
struct WebBookmarkEntry {
    created_at: u64,
    snapshot: WebBookmarkSnapshot,
}

/// Projects kind:39701 web bookmarks.
pub struct WebBookmarksProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    bookmarks: Mutex<BTreeMap<WebBookmarkKey, WebBookmarkEntry>>,
}

type WebBookmarkKey = (String, String);

impl WebBookmarksProjection {
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            bookmarks: Mutex::new(BTreeMap::new()),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> WebBookmarksSnapshot {
        let Ok(bookmarks) = self.bookmarks.lock() else {
            return WebBookmarksSnapshot::default();
        };
        WebBookmarksSnapshot {
            bookmarks: bookmarks
                .values()
                .map(|entry| entry.snapshot.clone())
                .collect(),
        }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "bookmarks": [] }))
    }

    #[must_use]
    pub fn snapshot_for_authors<I, S>(&self, authors: I) -> WebBookmarksSnapshot
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let authors: BTreeSet<String> = authors.into_iter().map(Into::into).collect();
        let Ok(bookmarks) = self.bookmarks.lock() else {
            return WebBookmarksSnapshot::default();
        };
        WebBookmarksSnapshot {
            bookmarks: bookmarks
                .values()
                .filter(|entry| authors.contains(&entry.snapshot.author))
                .map(|entry| entry.snapshot.clone())
                .collect(),
        }
    }

    #[must_use]
    pub fn snapshot_for_bookmark(
        &self,
        author: &str,
        url_without_scheme: &str,
    ) -> Option<WebBookmarkSnapshot> {
        let Ok(bookmarks) = self.bookmarks.lock() else {
            return None;
        };
        bookmarks
            .get(&web_bookmark_key(author, url_without_scheme))
            .map(|entry| entry.snapshot.clone())
    }

    fn ensure_active_account(&self, account_pubkey: &str) -> Result<(), ActionRejection> {
        let active = self
            .active_pubkey
            .lock()
            .map_err(|_| ActionRejection::Invalid("web bookmark state unavailable".to_string()))?
            .as_ref()
            .cloned();
        match active {
            Some(active) if active == account_pubkey => Ok(()),
            Some(active) => Err(ActionRejection::Unauthorized(format!(
                "web bookmark action account {account_pubkey} does not match active account {active}"
            ))),
            None => Err(ActionRejection::Unauthorized(
                "web bookmark action requires an active account".to_string(),
            )),
        }
    }
}

impl ObservedProjectionSink for WebBookmarksProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_WEB_BOOKMARK {
            return;
        }
        let Some(snapshot) = parse_web_bookmark(event) else {
            return;
        };
        let key = web_bookmark_key(&snapshot.author, &snapshot.url_without_scheme);
        let Ok(mut bookmarks) = self.bookmarks.lock() else {
            return;
        };
        if bookmarks
            .get(&key)
            .is_some_and(|entry| event.created_at < entry.created_at)
        {
            return;
        }
        bookmarks.insert(
            key,
            WebBookmarkEntry {
                created_at: event.created_at,
                snapshot,
            },
        );
    }
}

/// Draft data for a kind:39701 web bookmark publish.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebBookmarkDraft {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashtags: Vec<String>,
}

/// Build a kind:39701 unsigned event from a validated web bookmark draft.
pub fn build_web_bookmark_event(draft: &WebBookmarkDraft) -> Result<UnsignedEvent, String> {
    let bookmark = normalize_web_bookmark_draft(draft)?;
    let mut tags = Vec::with_capacity(bookmark.hashtags.len() + 3);
    tags.push(vec!["d".to_string(), bookmark.url_without_scheme]);
    if let Some(published_at) = bookmark.published_at {
        tags.push(vec!["published_at".to_string(), published_at.to_string()]);
    }
    if let Some(title) = bookmark.title {
        tags.push(vec!["title".to_string(), title]);
    }
    for hashtag in bookmark.hashtags {
        tags.push(vec!["t".to_string(), hashtag]);
    }
    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: KIND_WEB_BOOKMARK,
        tags,
        content: bookmark.description.unwrap_or_default(),
        created_at: 0,
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishWebBookmarkInput {
    pub account_pubkey: String,
    pub bookmark: WebBookmarkDraft,
}

pub struct PublishWebBookmarkAction {
    projection: Arc<WebBookmarksProjection>,
}

impl PublishWebBookmarkAction {
    #[must_use]
    pub fn new(projection: Arc<WebBookmarksProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for PublishWebBookmarkAction {
    const NAMESPACE: &'static str = "nmp.nip51.publish_web_bookmark";
    type Action = PublishWebBookmarkInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        normalize_web_bookmark_draft(&action.bookmark).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        self.projection
            .ensure_active_account(&action.account_pubkey)
            .map_err(action_rejection_message)?;
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_web_bookmark_event(&action.bookmark)?,
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

pub fn register_web_bookmark_actions(
    app: &mut impl ActionRegistrar,
    projection: Arc<WebBookmarksProjection>,
) {
    app.register_default_action(PublishWebBookmarkAction::new(projection));
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NormalizedWebBookmarkDraft {
    url_without_scheme: String,
    title: Option<String>,
    description: Option<String>,
    published_at: Option<u64>,
    hashtags: Vec<String>,
}

fn parse_web_bookmark(event: &KernelEvent) -> Option<WebBookmarkSnapshot> {
    let url_without_scheme = event
        .tags
        .iter()
        .find(|tag| tag.first().is_some_and(|name| name == "d"))
        .and_then(|tag| tag.get(1))
        .and_then(|value| normalize_web_bookmark_identifier(value).ok())?;

    let mut title = None;
    let mut published_at = None;
    let mut hashtags = Vec::new();
    for tag in &event.tags {
        match tag.first().map(String::as_str) {
            Some("title") => title = title.take().or_else(|| tag.get(1).cloned()),
            Some("published_at") => {
                published_at = published_at
                    .take()
                    .or_else(|| tag.get(1).and_then(|value| value.parse::<u64>().ok()));
            }
            Some("t") => {
                if let Some(hashtag) = tag.get(1).and_then(|value| normalize_hashtag(value)) {
                    hashtags.push(hashtag);
                }
            }
            _ => {}
        }
    }

    Some(WebBookmarkSnapshot {
        author: event.author.clone(),
        url_without_scheme,
        event_id: event.id.clone(),
        created_at: event.created_at,
        title,
        description: event.content.clone(),
        published_at,
        hashtags: dedupe_strings(hashtags),
    })
}

fn normalize_web_bookmark_draft(
    draft: &WebBookmarkDraft,
) -> Result<NormalizedWebBookmarkDraft, String> {
    let url_without_scheme = normalize_web_bookmark_url(&draft.url)?;
    let title = nonempty_option(draft.title.as_deref()).map(str::to_string);
    let description = nonempty_option(draft.description.as_deref()).map(str::to_string);
    let hashtags = dedupe_strings(
        draft
            .hashtags
            .iter()
            .filter_map(|hashtag| normalize_hashtag(hashtag))
            .collect(),
    );
    Ok(NormalizedWebBookmarkDraft {
        url_without_scheme,
        title,
        description,
        published_at: draft.published_at,
        hashtags,
    })
}

fn normalize_web_bookmark_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| "web bookmark URL must start with http:// or https://".to_string())?;
    normalize_web_bookmark_identifier(without_scheme)
}

fn normalize_web_bookmark_identifier(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("web bookmark d tag must be non-empty".to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.contains("://")
    {
        return Err("web bookmark d tag must omit the URL scheme".to_string());
    }
    Ok(trimmed.to_string())
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn web_bookmark_key(author: &str, url_without_scheme: &str) -> WebBookmarkKey {
    (author.to_string(), url_without_scheme.trim().to_string())
}

#[cfg(test)]
#[path = "web_bookmarks/tests.rs"]
mod tests;
