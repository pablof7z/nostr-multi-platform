//! `nmp-nip78` — generic NIP-78 kind:30078 app-data mechanics.
//!
//! # Scope
//!
//! This crate owns only reusable NIP-78 mechanics:
//!
//! - kind:30078 event construction with a required `d` tag;
//! - active-account ingest of kind:30078 events;
//! - deterministic addressable-event supersession by `(author, d-tag)`;
//! - bounded, app-neutral raw records keyed by `d`.
//!
//! It deliberately does not define application key names, relay defaults,
//! import UX, cache thresholds, or device-local preferences. Apps interpret
//! `d` values and `content`; NMP only supplies the reusable protocol shape.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_signer_iface::UnsignedEvent;
use nmp_core::KernelEventObserver;
use nmp_kinds::KIND_APP_DATA;
use serde::{Deserialize, Serialize};

/// Default maximum number of active-account app-data records retained.
pub const DEFAULT_MAX_RECORDS: usize = 256;

/// Errors returned by the NIP-78 builder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppDataError {
    /// The addressable event key must include a non-empty `d` tag.
    EmptyDTag,
    /// Extra tags cannot be empty and cannot add another `d` tag.
    InvalidExtraTag,
}

impl std::fmt::Display for AppDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDTag => write!(f, "NIP-78 app data requires a non-empty d tag"),
            Self::InvalidExtraTag => {
                write!(f, "NIP-78 extra tags must be non-empty and cannot use d")
            }
        }
    }
}

impl std::error::Error for AppDataError {}

/// Build a kind:30078 unsigned event for the active account publish path.
///
/// The actor overwrites `pubkey` with the signing account during publish, but
/// callers still pass the current pubkey hint so the unsigned value is useful
/// in tests and signer hand-offs.
pub fn build_app_data_event(
    pubkey: impl Into<String>,
    d_tag: impl Into<String>,
    content: impl Into<String>,
    created_at: u64,
    extra_tags: Vec<Vec<String>>,
) -> Result<UnsignedEvent, AppDataError> {
    let d_tag = d_tag.into();
    if d_tag.is_empty() {
        return Err(AppDataError::EmptyDTag);
    }
    if extra_tags
        .iter()
        .any(|tag| tag.is_empty() || tag.first().is_some_and(|name| name == "d"))
    {
        return Err(AppDataError::InvalidExtraTag);
    }

    let mut tags = Vec::with_capacity(extra_tags.len() + 1);
    tags.push(vec!["d".to_string(), d_tag]);
    tags.extend(extra_tags);
    Ok(UnsignedEvent {
        pubkey: pubkey.into(),
        kind: KIND_APP_DATA,
        tags,
        content: content.into(),
        created_at,
    })
}

/// One projected active-account kind:30078 record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppDataRecord {
    /// The NIP-78 address key from the first `d` tag.
    pub d_tag: String,
    /// Raw event content. Apps own its format.
    pub content: String,
    /// Raw event tags, including the `d` tag. Apps may define additional tags.
    pub tags: Vec<Vec<String>>,
    /// Event `created_at` unix seconds.
    pub created_at: u64,
    /// Event id that supplied the currently winning record.
    pub event_id: String,
}

/// Deterministic snapshot of retained active-account app-data records.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppDataSnapshot {
    /// Account that owns the records. `None` means no visible active data.
    pub owner_pubkey: Option<String>,
    /// Records sorted by `d_tag`.
    pub records: Vec<AppDataRecord>,
}

/// Active-account projection for NIP-78 app-data events.
pub struct AppDataProjection {
    active_pubkey: Arc<Mutex<Option<String>>>,
    max_records: usize,
    inner: Mutex<ProjectionState>,
}

#[derive(Default)]
struct ProjectionState {
    owner_pubkey: Option<String>,
    records: BTreeMap<String, AppDataRecord>,
}

impl AppDataProjection {
    /// Construct a projection with the default bound.
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self::with_max_records(active_pubkey, DEFAULT_MAX_RECORDS)
    }

    /// Construct a projection with an explicit record bound.
    #[must_use]
    pub fn with_max_records(active_pubkey: Arc<Mutex<Option<String>>>, max_records: usize) -> Self {
        Self {
            active_pubkey,
            max_records: max_records.max(1),
            inner: Mutex::new(ProjectionState::default()),
        }
    }

    /// Return the currently winning active-account record for `d_tag`.
    #[must_use]
    pub fn get(&self, d_tag: &str) -> Option<AppDataRecord> {
        let active = self.active_account()?;
        let Ok(inner) = self.inner.lock() else {
            return None;
        };
        if inner.owner_pubkey.as_deref() != Some(active.as_str()) {
            return None;
        }
        inner.records.get(d_tag).cloned()
    }

    /// Return all retained records for the active account.
    #[must_use]
    pub fn snapshot(&self) -> AppDataSnapshot {
        let Some(active) = self.active_account() else {
            return AppDataSnapshot::default();
        };
        let Ok(inner) = self.inner.lock() else {
            return AppDataSnapshot::default();
        };
        if inner.owner_pubkey.as_deref() != Some(active.as_str()) {
            return AppDataSnapshot::default();
        }
        AppDataSnapshot {
            owner_pubkey: Some(active),
            records: inner.records.values().cloned().collect(),
        }
    }

    fn active_account(&self) -> Option<String> {
        self.active_pubkey.lock().ok()?.as_ref().cloned()
    }

    fn ingest(&self, event: &KernelEvent) {
        if event.kind != KIND_APP_DATA {
            return;
        }
        let Some(active) = self.active_account() else {
            return;
        };
        if event.author != active {
            return;
        }
        let Some(d_tag) = first_d_tag(&event.tags) else {
            return;
        };

        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner.owner_pubkey.as_deref() != Some(active.as_str()) {
            inner.owner_pubkey = Some(active);
            inner.records.clear();
        }

        let incoming = AppDataRecord {
            d_tag: d_tag.clone(),
            content: event.content.clone(),
            tags: event.tags.clone(),
            created_at: event.created_at,
            event_id: event.id.clone(),
        };
        let should_replace = inner
            .records
            .get(&d_tag)
            .is_none_or(|current| incoming_wins(&incoming, current));
        if should_replace {
            inner.records.insert(d_tag, incoming);
            trim_to_bound(&mut inner.records, self.max_records);
        }
    }
}

impl KernelEventObserver for AppDataProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

fn first_d_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter()
        .find(|tag| tag.first().is_some_and(|name| name == "d"))
        .and_then(|tag| tag.get(1))
        .filter(|value| !value.is_empty())
        .cloned()
}

fn incoming_wins(incoming: &AppDataRecord, current: &AppDataRecord) -> bool {
    incoming.created_at > current.created_at
        || (incoming.created_at == current.created_at && incoming.event_id < current.event_id)
}

fn trim_to_bound(records: &mut BTreeMap<String, AppDataRecord>, max_records: usize) {
    while records.len() > max_records {
        let Some(stale_key) = records
            .iter()
            .min_by(|(_, a), (_, b)| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| b.event_id.cmp(&a.event_id))
            })
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        records.remove(&stale_key);
    }
}

#[cfg(test)]
mod tests;
