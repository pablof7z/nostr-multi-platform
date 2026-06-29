//! [`PointerSourceModel`] — the pure pointer-source read-model state machine.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_nip18::AddressCoordinate;

use crate::embed_registry::{EmbedTarget, ResolvedEvent};

use super::projection::{PointerItem, PointerSortMode};

/// Metadata retained per pointer event so the reverse index can be maintained
/// incrementally on both add and drop.
#[derive(Clone, Debug)]
struct PointerRecord {
    author: String,
    created_at: u64,
    targets: BTreeSet<EmbedTarget>,
}

/// Pointer-source read model: pointer ingest, `pointedBy` index, target
/// hydration, and sorted projection. See the module docs for how the
/// composition layer drives it.
#[derive(Clone, Debug, Default)]
pub struct PointerSourceModel {
    sort: PointerSortMode,
    /// Pointer event id -> its author / time / extracted targets.
    pointers: BTreeMap<EventId, PointerRecord>,
    /// `pointedBy`: target -> set of pointer event ids referencing it. A target
    /// with an empty set is removed entirely, so the key set is exactly the live
    /// materialization demand.
    pointed_by: BTreeMap<EmbedTarget, BTreeSet<EventId>>,
    /// Hydrated target events keyed by target identity.
    resolved: BTreeMap<EmbedTarget, ResolvedEvent>,
}

impl PointerSourceModel {
    /// Build an empty model with the given initial sort mode.
    #[must_use]
    pub fn new(sort: PointerSortMode) -> Self {
        Self {
            sort,
            ..Self::default()
        }
    }

    /// The active sort mode.
    #[must_use]
    pub fn sort_mode(&self) -> PointerSortMode {
        self.sort
    }

    /// Change the projection sort mode. Returns whether the mode changed.
    ///
    /// This never alters the demanded target set — only [`Self::items`] output
    /// order — so the composition layer must NOT re-sync interests on a sort
    /// change.
    pub fn set_sort(&mut self, sort: PointerSortMode) -> bool {
        let changed = self.sort != sort;
        self.sort = sort;
        changed
    }

    /// Clear all pointer and target state. Returns whether any live demand or
    /// hydrated output was withdrawn.
    pub fn clear(&mut self) -> bool {
        let changed = !self.pointed_by.is_empty() || !self.resolved.is_empty();
        self.pointers.clear();
        self.pointed_by.clear();
        self.resolved.clear();
        changed
    }

    /// Ingest a pointer event. Returns `true` when the demanded target set
    /// changed (a previously-unseen target appeared), signalling the composition
    /// layer to re-materialize the dependent-interest set.
    ///
    /// Re-delivered pointers (same id, common on reconnect) are idempotent.
    /// Pointer events that carry no `e` / `a` reference contribute nothing — they
    /// never widen demand, so an all-empty reduction stays closed (no wildcard
    /// target query).
    pub fn apply_pointer(&mut self, event: &KernelEvent) -> bool {
        if self.pointers.contains_key(&event.id) {
            return false;
        }
        let targets = extract_targets(event);
        if targets.is_empty() {
            return false;
        }
        let mut set_changed = false;
        for target in &targets {
            let referencing = self.pointed_by.entry(target.clone()).or_default();
            if referencing.is_empty() {
                set_changed = true;
            }
            referencing.insert(event.id.clone());
        }
        self.pointers.insert(
            event.id.clone(),
            PointerRecord {
                author: event.author.clone(),
                created_at: event.created_at,
                targets,
            },
        );
        set_changed
    }

    /// Drop a pointer event (source shrink — e.g. the pointer interest narrowed
    /// or the pointer was deleted). Returns `true` when the demanded target set
    /// shrank, signalling the composition layer to withdraw the now-unreferenced
    /// target children.
    pub fn drop_pointer(&mut self, pointer_id: &EventId) -> bool {
        let Some(record) = self.pointers.remove(pointer_id) else {
            return false;
        };
        let mut set_changed = false;
        for target in &record.targets {
            if let Some(referencing) = self.pointed_by.get_mut(target) {
                referencing.remove(pointer_id);
                if referencing.is_empty() {
                    self.pointed_by.remove(target);
                    self.resolved.remove(target);
                    set_changed = true;
                }
            }
        }
        set_changed
    }

    /// Ingest a target event delivered for the current demand. Only events that
    /// satisfy a currently-demanded target are stored; an addressable target
    /// keeps the newest version (newest `created_at` wins). Returns whether the
    /// resolved projection changed.
    pub fn apply_target(&mut self, event: &KernelEvent) -> bool {
        let mut changed = false;

        let by_id = EmbedTarget::Event(event.id.clone());
        if self.pointed_by.contains_key(&by_id) {
            changed |= self.store_resolved(by_id, event);
        }

        if let Some(coord) = AddressCoordinate::from_event(event) {
            let by_addr = EmbedTarget::Address {
                kind: coord.kind,
                pubkey: coord.pubkey,
                identifier: coord.identifier,
            };
            if self.pointed_by.contains_key(&by_addr) {
                changed |= self.store_resolved(by_addr, event);
            }
        }

        changed
    }

    /// The live materialization demand: every target referenced by at least one
    /// pointer event. The composition layer maps each entry to one dependent
    /// interest (`event_ids` for [`EmbedTarget::Event`], `addresses` for
    /// [`EmbedTarget::Address`]).
    pub fn target_demand(&self) -> impl ExactSizeIterator<Item = &EmbedTarget> {
        self.pointed_by.keys()
    }

    /// Whether any target is currently demanded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pointed_by.is_empty()
    }

    /// The pointer events that reference `target`, in id order. Empty when the
    /// target is not (or no longer) demanded.
    #[must_use]
    pub fn pointed_by(&self, target: &EmbedTarget) -> Vec<EventId> {
        self.pointed_by
            .get(target)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The sorted projection of every hydrated target. Targets that are demanded
    /// but not yet hydrated are omitted (they appear once their event arrives).
    #[must_use]
    pub fn items(&self) -> Vec<PointerItem> {
        let mut items: Vec<PointerItem> = self
            .pointed_by
            .iter()
            .filter_map(|(target, referencing)| {
                let event = self.resolved.get(target)?.clone();
                Some(self.build_item(target.clone(), event, referencing))
            })
            .collect();
        self.sort.order(&mut items);
        items
    }

    fn store_resolved(&mut self, target: EmbedTarget, event: &KernelEvent) -> bool {
        match self.resolved.get(&target) {
            Some(existing) if existing.created_at >= event.created_at => false,
            _ => {
                self.resolved.insert(target, ResolvedEvent::from(event));
                true
            }
        }
    }

    fn build_item(
        &self,
        target: EmbedTarget,
        event: ResolvedEvent,
        referencing: &BTreeSet<EventId>,
    ) -> PointerItem {
        let mut authors = BTreeSet::new();
        let mut latest_pointer_at = 0;
        for pointer_id in referencing {
            if let Some(record) = self.pointers.get(pointer_id) {
                authors.insert(record.author.as_str());
                latest_pointer_at = latest_pointer_at.max(record.created_at);
            }
        }
        PointerItem {
            target,
            event,
            pointer_count: referencing.len(),
            unique_authors: authors.len(),
            latest_pointer_at,
        }
    }
}

/// Extract every `e` (event-id) and `a` (address-coordinate) reference from a
/// pointer event into the demanded target set.
///
/// This is the generic NIP-01 reference shape; *which* pointer kinds are
/// subscribed to (kind:6/7/9802/1111/…) is a product decision encoded by the
/// caller's pointer interest, not here, keeping this module D0-clean. Bare or
/// malformed references fail closed (they are dropped, never fabricated into a
/// wildcard).
fn extract_targets(event: &KernelEvent) -> BTreeSet<EmbedTarget> {
    let mut targets = BTreeSet::new();
    for tag in &event.tags {
        match tag.first().map(String::as_str) {
            Some("e" | "E") => {
                if let Some(id) = tag.get(1) {
                    if !id.is_empty() {
                        targets.insert(EmbedTarget::Event(id.clone()));
                    }
                }
            }
            Some("a" | "A") => {
                if let Some(raw) = tag.get(1) {
                    if let Some(coord) = AddressCoordinate::parse(raw) {
                        targets.insert(EmbedTarget::Address {
                            kind: coord.kind,
                            pubkey: coord.pubkey,
                            identifier: coord.identifier,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    targets
}
