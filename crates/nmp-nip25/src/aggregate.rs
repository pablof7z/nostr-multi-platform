//! NIP-25 reaction **aggregation** read model — kind:7 reactions folded by the
//! event they target.
//!
//! Where [`crate::projection::ReactionProjection`] is a per-target list of
//! individual reaction rows + the viewer's own reaction, this projection is the
//! *aggregate* a chat / thread UI needs to draw reaction chips: per target
//! event, the total reaction count, the per-emoji breakdown, and the distinct
//! reactor pubkeys (for a "who reacted" sheet).
//!
//! ## Doctrine
//!
//! NIP-25 owns kind:7 reaction semantics, so the parsing + aggregation live
//! here, NOT in `nmp-nip01` / `nmp-core`. This projection is **kind-agnostic
//! about scope**: it folds every kind:7 event it is handed, keyed by target id.
//! Scoping the fold to one NIP-29 group (the `["h", local_id]` tag) is composed
//! at the app layer by the relay-pinned `#h` + `kinds:[7]` interest filter that
//! feeds this observer — this crate never names the `h` tag (it does not depend
//! on `nmp-nip29`).
//!
//! ## Display separation (aim.md §2)
//!
//! Every value is raw: the emoji `token` is the verbatim reaction content (with
//! the single NIP-25 normalization that an empty content is the `"+"` like
//! token), and `reactors` are raw hex pubkeys. No `display::` formatting runs on
//! the encode path.
//!
//! ## Why not reuse a richer rust-nostr parser
//!
//! rust-nostr models a reaction's content as a plain string; the only NIP-25
//! semantic on the token is the empty → `"+"` (like) normalization, which this
//! module applies. There is no higher-order rust-nostr reaction aggregator to
//! reuse, so the fold is implemented here over raw [`KernelEvent`]s.

use std::collections::BTreeMap;
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::action::{KIND_REACTION, KIND_REACTION_DELETE};

/// One emoji's tally within a [`ReactionTargetAggregate`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactionEmojiCount {
    /// The raw reaction token (verbatim content; empty content normalizes to
    /// the NIP-25 `"+"` like token).
    pub token: String,
    /// How many reactions carried this token for the target.
    pub count: u64,
}

/// One of the **viewer's own** surviving reactions on a target: the raw emoji
/// token plus the raw hex id of the viewer's kind:7 reaction event.
///
/// This is the retraction handle: to toggle a reaction off, the app deletes the
/// kind:7 event named by [`ViewerReaction::reaction_event_id`] — NIP-25 builds
/// the kind:5 deletion (`nmp.nip25.unreact`); in a NIP-29 group the app routes
/// that finished event through the generic host-pinned
/// `nmp.nip29.publish_group_event` envelope (kind-blind transport). Surfacing the
/// id here is what makes toggle-off supportable — the aggregate is the only place
/// that knows which of the viewer's events backs a given (target, emoji).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ViewerReaction {
    /// The raw reaction token (verbatim content; empty content normalizes to
    /// the NIP-25 `"+"` like token).
    pub token: String,
    /// The raw hex id of the viewer's kind:7 reaction event for this (target,
    /// token). The id the app deletes to retract.
    pub reaction_event_id: String,
}

/// The aggregated reactions for one target event.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactionTargetAggregate {
    /// The reacted-to event id (raw hex).
    pub target_event_id: String,
    /// Total reaction count across all emoji (one per surviving kind:7 event).
    pub total: u64,
    /// Per-emoji counts, ordered by `count` descending then `token` ascending
    /// so the order is total and stable across snapshot ticks.
    pub by_emoji: Vec<ReactionEmojiCount>,
    /// Distinct reactor pubkeys (raw hex), ascending. For a "who reacted" view.
    pub reactors: Vec<String>,
    /// The **viewer's own** surviving reactions on this target — one entry per
    /// (token, reaction event id), ordered by `token` then `reaction_event_id`
    /// ascending. Empty when the projection has no viewer pubkey or the viewer
    /// has not reacted. Each entry's `reaction_event_id` is the kind:7 the app
    /// deletes to retract that reaction (toggle-off). Omitted from the serde
    /// shape when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mine: Vec<ViewerReaction>,
}

/// The serialised reaction-aggregate read model: one
/// [`ReactionTargetAggregate`] per target event that has at least one surviving
/// reaction, ordered by `target_event_id` ascending.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactionAggregateSnapshot {
    pub targets: Vec<ReactionTargetAggregate>,
}

impl ReactionAggregateSnapshot {
    /// An empty snapshot — a freshly-constructed projection or a poisoned
    /// internal mutex (D6).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            targets: Vec::new(),
        }
    }
}

/// One stored reaction, keyed inside the projection by its reaction event id so
/// re-delivery is idempotent and a NIP-09 delete can remove it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ReactionRecord {
    target_event_id: String,
    author_pubkey: String,
    token: String,
}

/// Accumulates kind:7 reactions and folds them, on read, into per-target
/// aggregates.
///
/// Register the same `Arc` as an [`ObservedProjectionSink`] (ingest) and capture
/// it in a snapshot-projection closure (output), exactly like the NIP-29 group
/// views. Group scoping is the feeding interest's job (see module docs).
pub struct ReactionAggregateProjection {
    /// The viewer (active account) pubkey, raw hex, used to compute the per-target
    /// `mine` retraction handles. `None` (or empty) means "no viewer" and every
    /// `mine` is empty. Wired the same way as
    /// [`crate::projection::ReactionProjection`]'s viewer pubkey.
    viewer_pubkey: Mutex<Option<String>>,
    /// Reactions keyed by reaction event id. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`]; the aggregate is computed on read, not here.
    entries: Mutex<BoundedMessageMap<String, ReactionRecord>>,
}

impl ReactionAggregateProjection {
    /// Construct an empty projection for `viewer_pubkey` (the active account, raw
    /// hex; `None` or empty disables the per-target `mine` handles).
    #[must_use]
    pub fn new(viewer_pubkey: Option<String>) -> Self {
        Self {
            viewer_pubkey: Mutex::new(viewer_pubkey.filter(|p| !p.is_empty())),
            entries: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// Update the viewer (active account) pubkey, e.g. after an account switch.
    /// `None`/empty clears it. The next snapshot recomputes `mine` accordingly.
    pub fn set_viewer_pubkey(&self, viewer_pubkey: Option<String>) {
        if let Ok(mut current) = self.viewer_pubkey.lock() {
            *current = viewer_pubkey.filter(|p| !p.is_empty());
        }
    }

    /// Fold the current reaction set into a [`ReactionAggregateSnapshot`].
    ///
    /// D6: a poisoned mutex degrades to [`ReactionAggregateSnapshot::empty`]
    /// rather than panicking — this can run on the actor thread inside a
    /// snapshot tick.
    #[must_use]
    pub fn snapshot(&self) -> ReactionAggregateSnapshot {
        let Ok(entries) = self.entries.lock() else {
            return ReactionAggregateSnapshot::empty();
        };
        let viewer = self.viewer_pubkey.lock().ok().and_then(|v| v.clone());
        let viewer = viewer.as_deref();

        // target -> (token -> count, distinct reactors, viewer's own reactions).
        // `iter()` yields the map key (the reaction event id) so the viewer's
        // retraction handle (`mine`) carries the id to delete.
        let mut by_target: BTreeMap<&str, TargetAccumulator<'_>> = BTreeMap::new();
        for (reaction_event_id, record) in entries.iter() {
            let acc = by_target.entry(&record.target_event_id).or_default();
            acc.total += 1;
            *acc.by_emoji.entry(&record.token).or_insert(0) += 1;
            acc.reactors.insert(&record.author_pubkey);
            if viewer == Some(record.author_pubkey.as_str()) {
                acc.mine.push((&record.token, reaction_event_id));
            }
        }

        let targets = by_target
            .into_iter()
            .map(|(target, acc)| acc.finish(target))
            .collect();
        ReactionAggregateSnapshot { targets }
    }

    /// The aggregate for a single target event (`None` when it has no surviving
    /// reactions). Convenience for a host rendering one row.
    #[must_use]
    pub fn aggregate_for(&self, target_event_id: &str) -> Option<ReactionTargetAggregate> {
        self.snapshot()
            .targets
            .into_iter()
            .find(|t| t.target_event_id == target_event_id)
    }

    /// Snapshot as a `serde_json::Value` — the generic-fallback shape a host
    /// `register_snapshot_projection` closure returns alongside the typed
    /// sidecar.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "targets": [] }))
    }

    fn ingest(&self, event: &KernelEvent) {
        match event.kind {
            KIND_REACTION => self.ingest_reaction(event),
            KIND_REACTION_DELETE => self.ingest_delete(event),
            _ => {}
        }
    }

    fn ingest_reaction(&self, event: &KernelEvent) {
        // NIP-25: the reacted-to event is the LAST `e` tag.
        let Some(target_event_id) = last_tag_value(&event.tags, "e") else {
            return;
        };
        let record = ReactionRecord {
            target_event_id,
            author_pubkey: event.author.clone(),
            token: normalize_reaction(&event.content),
        };
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(event.id.clone(), record);
        }
    }

    fn ingest_delete(&self, event: &KernelEvent) {
        let deleted_ids: Vec<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|name| name == "e") {
                    tag.get(1).cloned()
                } else {
                    None
                }
            })
            .collect();
        if deleted_ids.is_empty() {
            return;
        }
        if let Ok(mut entries) = self.entries.lock() {
            for id in deleted_ids {
                // Only the original reactor may retract their reaction.
                if entries
                    .get(&id)
                    .is_some_and(|record| record.author_pubkey == event.author)
                {
                    entries.remove(&id);
                }
            }
        }
    }
}

impl Default for ReactionAggregateProjection {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ObservedProjectionSink for ReactionAggregateProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

/// Per-target fold state used while building a snapshot. Borrows the stored
/// strings so the hot accumulation path clones nothing until `finish`.
#[derive(Default)]
struct TargetAccumulator<'a> {
    total: u64,
    by_emoji: BTreeMap<&'a str, u64>,
    reactors: std::collections::BTreeSet<&'a str>,
    /// The viewer's own (token, reaction event id) pairs for this target.
    mine: Vec<(&'a str, &'a str)>,
}

impl TargetAccumulator<'_> {
    fn finish(self, target_event_id: &str) -> ReactionTargetAggregate {
        // Emoji ordered by count descending, then token ascending — a total,
        // stable order independent of map iteration.
        let mut by_emoji: Vec<ReactionEmojiCount> = self
            .by_emoji
            .into_iter()
            .map(|(token, count)| ReactionEmojiCount {
                token: token.to_string(),
                count,
            })
            .collect();
        by_emoji.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.token.cmp(&b.token)));

        let reactors = self.reactors.into_iter().map(str::to_string).collect();

        // Viewer's own reactions ordered by token then reaction event id — a
        // total, stable order independent of map iteration.
        let mut mine: Vec<ViewerReaction> = self
            .mine
            .into_iter()
            .map(|(token, reaction_event_id)| ViewerReaction {
                token: token.to_string(),
                reaction_event_id: reaction_event_id.to_string(),
            })
            .collect();
        mine.sort_by(|a, b| {
            a.token
                .cmp(&b.token)
                .then_with(|| a.reaction_event_id.cmp(&b.reaction_event_id))
        });

        ReactionTargetAggregate {
            target_event_id: target_event_id.to_string(),
            total: self.total,
            by_emoji,
            reactors,
            mine,
        }
    }
}

/// NIP-25 reaction-token normalization: an empty (whitespace-only) content is
/// the `"+"` like token; otherwise the content is the verbatim token.
fn normalize_reaction(content: &str) -> String {
    if content.trim().is_empty() {
        "+".to_string()
    } else {
        content.to_string()
    }
}

/// The LAST value of a single-letter tag `name`, ignoring empty values.
fn last_tag_value(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter()
        .rev()
        .find_map(|tag| {
            if tag.first().is_some_and(|candidate| candidate == name) {
                tag.get(1).filter(|value| !value.is_empty()).cloned()
            } else {
                None
            }
        })
}

#[cfg(test)]
#[path = "aggregate_tests.rs"]
mod tests;
