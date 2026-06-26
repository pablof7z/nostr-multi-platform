//! Reusable NIP-18 repost activity projection.
//!
//! This module owns the read-side shape for "which targets were reposted by
//! these authors?" It is deliberately protocol-generic: apps can ask for all
//! repost targets or filter by a target kind such as NIP-23 long-form articles
//! (`kind:30023`) without adding app-specific logic to `nmp-core`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_LONG_FORM_ARTICLE;
use nmp_planner::InterestShape;

use crate::{
    try_from_kernel_event, AddressCoordinate, DeleteRecord, RepostRecord, KIND_DELETE,
    KIND_GENERIC_REPOST, KIND_REPOST,
};

/// Canonical target identity for repost read-surfacing.
///
/// Addressable targets use their NIP-01 coordinate, never a wrapper event id.
/// Event-id-only reposts remain event keyed and carry `kind` only when the
/// wrapper proves it through a `k` tag or embedded event.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RepostTarget {
    Event { event_id: String, kind: Option<u32> },
    Address(AddressCoordinate),
}

impl RepostTarget {
    /// The proven target kind, when the wrapper supplies enough information.
    #[must_use]
    pub fn kind(&self) -> Option<u32> {
        match self {
            Self::Event { kind, .. } => *kind,
            Self::Address(coord) => Some(coord.kind),
        }
    }

    fn from_record(record: &RepostRecord) -> Option<Self> {
        if let Some(address) = record.target_address.clone() {
            return Some(Self::Address(address));
        }
        record.target_event_id.clone().map(|event_id| Self::Event {
            event_id,
            kind: record.target_kind,
        })
    }
}

/// One accepted repost wrapper after NIP-18 decoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepostObservation {
    pub repost_event_id: String,
    pub author_pubkey: String,
    pub created_at: u64,
    pub target: RepostTarget,
}

impl RepostObservation {
    fn from_record(record: RepostRecord) -> Option<Self> {
        let target = RepostTarget::from_record(&record)?;
        Some(Self {
            repost_event_id: record.event_id,
            author_pubkey: record.author,
            created_at: record.created_at,
            target,
        })
    }
}

/// Aggregated activity for one repost target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepostActivity {
    pub target: RepostTarget,
    pub interactor_pubkeys: BTreeSet<String>,
    pub latest_activity_at: u64,
}

/// In-process projection of accepted NIP-18 repost wrappers.
///
/// Register this as a [`ObservedProjectionSink`] and keep the same `Arc` for read
/// queries. The projection is bounded by event id and holds only facts delivered
/// through active interests; it never scans storage or relay history.
pub struct RepostActivityProjection {
    reposts: Mutex<BoundedMessageMap<String, RepostObservation>>,
}

impl Default for RepostActivityProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl RepostActivityProjection {
    /// Construct an empty projection with the workspace projection bound.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(MAX_PROJECTION_MESSAGES)
    }

    /// Construct an empty projection with an explicit capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            reposts: Mutex::new(BoundedMessageMap::new(capacity)),
        }
    }

    /// Ingest one accepted kernel event. Returns `true` when the projection
    /// changed. Poisoned mutexes and irrelevant events are silent no-ops (D6).
    pub fn ingest_event(&self, event: &KernelEvent) -> bool {
        if let Some(delete) = DeleteRecord::try_from_kernel_event(event) {
            return self.apply_delete(&delete);
        }
        let Some(record) = try_from_kernel_event(event) else {
            return false;
        };
        let Some(observation) = RepostObservation::from_record(record) else {
            return false;
        };
        let Ok(mut reposts) = self.reposts.lock() else {
            return false;
        };
        reposts.insert(observation.repost_event_id.clone(), observation);
        true
    }

    /// Aggregate repost activity authored by `authors`, optionally narrowed to
    /// a target kind such as `kind:30023`.
    #[must_use]
    pub fn activity_for_authors(
        &self,
        authors: &BTreeSet<String>,
        target_kind: Option<u32>,
    ) -> Vec<RepostActivity> {
        if authors.is_empty() {
            return Vec::new();
        }
        let Ok(reposts) = self.reposts.lock() else {
            return Vec::new();
        };

        let mut by_target: BTreeMap<RepostTarget, RepostActivity> = BTreeMap::new();
        for observation in reposts.values() {
            if !authors.contains(&observation.author_pubkey) {
                continue;
            }
            if target_kind.is_some_and(|kind| observation.target.kind() != Some(kind)) {
                continue;
            }
            let target = observation.target.clone();
            let activity = by_target
                .entry(target.clone())
                .or_insert_with(|| RepostActivity {
                    target,
                    interactor_pubkeys: BTreeSet::new(),
                    latest_activity_at: observation.created_at,
                });
            activity
                .interactor_pubkeys
                .insert(observation.author_pubkey.clone());
            activity.latest_activity_at = activity.latest_activity_at.max(observation.created_at);
        }

        let mut activities: Vec<_> = by_target.into_values().collect();
        activities.sort_by(|a, b| {
            b.latest_activity_at
                .cmp(&a.latest_activity_at)
                .then_with(|| a.target.cmp(&b.target))
        });
        activities
    }

    /// Convenience query for NIP-23 long-form article targets (`kind:30023`).
    #[must_use]
    pub fn article_activity_for_authors(&self, authors: &BTreeSet<String>) -> Vec<RepostActivity> {
        self.activity_for_authors(authors, Some(KIND_LONG_FORM_ARTICLE))
    }

    /// Return only target identities for authors and an optional target kind.
    #[must_use]
    pub fn targets_reposted_by_authors(
        &self,
        authors: &BTreeSet<String>,
        target_kind: Option<u32>,
    ) -> BTreeSet<RepostTarget> {
        self.activity_for_authors(authors, target_kind)
            .into_iter()
            .map(|activity| activity.target)
            .collect()
    }

    /// Convenience target query for NIP-23 long-form articles (`kind:30023`).
    #[must_use]
    pub fn article_targets_reposted_by_authors(
        &self,
        authors: &BTreeSet<String>,
    ) -> BTreeSet<RepostTarget> {
        self.targets_reposted_by_authors(authors, Some(KIND_LONG_FORM_ARTICLE))
    }

    /// Current number of retained repost wrapper events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reposts.lock().map_or(0, |reposts| reposts.len())
    }

    /// Whether the projection currently has no retained reposts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn apply_delete(&self, record: &DeleteRecord) -> bool {
        if record.event_targets.is_empty() {
            return false;
        }
        let Ok(mut reposts) = self.reposts.lock() else {
            return false;
        };
        let mut changed = false;
        for target in &record.event_targets {
            let owned = reposts
                .get(target)
                .is_some_and(|observation| observation.author_pubkey == record.author);
            if owned {
                reposts.remove(target);
                changed = true;
            }
        }
        changed
    }
}

impl ObservedProjectionSink for RepostActivityProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest_event(event);
    }
}

/// Build the pull/planner interest that feeds a repost activity projection.
///
/// The shape subscribes to both repost wrapper kinds plus NIP-09 deletes by the
/// same author set, so a deleted repost wrapper can retract the activity row.
/// Empty author sets fail closed with `None` rather than opening a wildcard
/// author subscription.
#[must_use]
pub fn repost_activity_interest_shape(authors: BTreeSet<String>) -> Option<InterestShape> {
    if authors.is_empty() {
        return None;
    }
    Some(InterestShape::timeline_for(
        authors,
        BTreeSet::from([KIND_REPOST, KIND_GENERIC_REPOST, KIND_DELETE]),
    ))
}

#[cfg(test)]
#[path = "repost_projection_tests.rs"]
mod tests;
