//! `EmbedClaimRegistry` — refcounted per-id claim/release for embedded events.
//!
//! When N timeline rows render the same `nevent1…` (or `naddr1…`), the
//! registry guarantees only ONE upstream subscription opens. Calls to
//! `claim` on the same target dedupe by id and return [`ClaimHandle`]
//! tokens; the last `release` decays the entry.
//!
//! Implemented as a [`ViewModule`] per PD-013 — D0-clean (no kernel
//! coupling beyond `ViewContext` + `KernelEvent`), debug-inspectable via
//! standard snapshot machinery, namespace `nmp.content.embed_registry`.
//!
//! # Lifecycle (apps integrate later)
//! Layer A owns the in-memory dedupe map only. The kernel-side wiring —
//! opening a subscription on first claim and closing it after the last
//! release's grace period — is M16-adjacent (`content-rendering.md` §9
//! Phase 2). Until then this struct gives apps the dedupe primitive without
//! the kernel cycle.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use nmp_core::nip21::NostrUri;
use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

/// Stable identity for an embed target — covers both event-id-addressed
/// (`note1`/`nevent1`) and coordinate-addressed (`naddr1`) embeds.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum EmbedTarget {
    /// `nevent1…` / `note1…` — 32-byte hex event id.
    Event(EventId),
    /// `naddr1…` — `(kind, pubkey, d_tag)` coordinate.
    Address {
        /// Event kind.
        kind: u32,
        /// Author pubkey (hex).
        pubkey: String,
        /// `d` tag identifier.
        identifier: String,
    },
}

impl EmbedTarget {
    /// Project a [`NostrUri`] onto the embed-target shape. `Profile` URIs
    /// return `None` — they aren't embeds.
    pub fn from_uri(uri: &NostrUri) -> Option<Self> {
        match uri {
            NostrUri::Profile { .. } => None,
            NostrUri::Event { event_id, .. } => Some(Self::Event(event_id.clone())),
            NostrUri::Address { identifier, pubkey, kind, .. } => Some(Self::Address {
                kind: *kind,
                pubkey: pubkey.clone(),
                identifier: identifier.clone(),
            }),
        }
    }
}

/// Opaque handle returned by [`EmbedClaimRegistry::claim`]. Hold while the
/// embed is visible; pass to `release` when it scrolls offscreen.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClaimHandle {
    target: EmbedTarget,
    handle_id: u64,
}

impl ClaimHandle {
    /// The target this handle refcounts.
    pub fn target(&self) -> &EmbedTarget {
        &self.target
    }

    /// Per-handle unique id — distinguishes 2 distinct claims for the
    /// same target.
    pub fn handle_id(&self) -> u64 {
        self.handle_id
    }
}

/// In-memory entry per target.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct Entry {
    refcount: usize,
    /// Resolved event payload — `None` until kernel ingest delivers it via
    /// `on_event_inserted`.
    resolved: Option<ResolvedEvent>,
}

/// Snapshot of a resolved embed event. Layer A doesn't need the full
/// `StoredEvent` shape — apps that want it look up the kernel store
/// directly using `id`. This is the minimum projection apps need to render
/// the embed card without a follow-up fetch.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ResolvedEvent {
    /// Hex event id.
    pub id: EventId,
    /// Hex author pubkey.
    pub author: String,
    /// Event kind.
    pub kind: u32,
    /// Unix seconds.
    pub created_at: u64,
    /// Raw content string (renderer tokenizes).
    pub content: String,
    /// Tag rows.
    pub tags: Vec<Vec<String>>,
}

impl From<&KernelEvent> for ResolvedEvent {
    fn from(e: &KernelEvent) -> Self {
        Self {
            id: e.id.clone(),
            author: e.author.clone(),
            kind: e.kind,
            created_at: e.created_at,
            content: e.content.clone(),
            tags: e.tags.clone(),
        }
    }
}

/// `ViewModule::Spec` — apps don't actually open one `EmbedClaimRegistry`
/// per "view"; the spec is unit-shaped. The registry is conceptually
/// app-singleton, with `claim`/`release` driving its state. We satisfy the
/// `ViewModule` trait for ADR-0009 conformance.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct EmbedClaimSpec;

/// Outward-facing snapshot of the registry — claimed targets + their
/// resolved payloads (when present).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EmbedRegistrySnapshot {
    /// Each entry: (target, refcount, optional resolved event).
    pub entries: Vec<(EmbedTarget, usize, Option<ResolvedEvent>)>,
}

/// Delta emitted on registry change.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum EmbedClaimDelta {
    /// A target's refcount or resolution changed.
    Updated {
        /// The affected target.
        target: EmbedTarget,
        /// Current refcount (0 = released).
        refcount: usize,
        /// Resolved event, if any.
        resolved: Option<ResolvedEvent>,
    },
}

/// State held inside the actor — the map of target → entry plus a counter
/// for handle uniqueness.
pub struct EmbedClaimState {
    entries: BTreeMap<EmbedTarget, Entry>,
    handle_seq: AtomicU64,
}

impl EmbedClaimState {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            handle_seq: AtomicU64::new(0),
        }
    }
}

/// The registry type — methods are inherent (not on the trait) because the
/// `ViewModule` shape is callback-driven. The free `claim` / `release`
/// methods are the API apps actually call from FFI bindings.
pub struct EmbedClaimRegistry;

impl EmbedClaimRegistry {
    /// Module namespace (matches the brief — `nmp.content.embed_registry`).
    pub const NAMESPACE: &'static str = "nmp.content.embed_registry";

    /// Initialise a fresh state — apps that don't run the full
    /// `ViewModule::open` machinery can hold an [`EmbedClaimState`]
    /// directly and call the inherent methods.
    pub fn state() -> EmbedClaimState {
        EmbedClaimState::new()
    }

    /// Claim a target. Increments refcount; returns a handle that must be
    /// released with [`release`] when the embed is no longer visible.
    /// Also returns the current [`ResolvedEvent`] when present (cold-start
    /// → `None`; warm or post-fetch → `Some`).
    pub fn claim(
        state: &mut EmbedClaimState,
        target: EmbedTarget,
    ) -> (ClaimHandle, Option<ResolvedEvent>) {
        let handle_id = state.handle_seq.fetch_add(1, Ordering::Relaxed);
        let entry = state.entries.entry(target.clone()).or_default();
        entry.refcount = entry.refcount.saturating_add(1);
        let resolved = entry.resolved.clone();
        (ClaimHandle { target, handle_id }, resolved)
    }

    /// Release a previously-claimed handle. Returns `true` if this was the
    /// last claim for that target (so the caller can act on the "all
    /// observers gone" signal — e.g. start a grace-period timer before
    /// closing the upstream subscription).
    pub fn release(state: &mut EmbedClaimState, handle: &ClaimHandle) -> bool {
        let Some(entry) = state.entries.get_mut(&handle.target) else {
            return false;
        };
        entry.refcount = entry.refcount.saturating_sub(1);
        if entry.refcount == 0 {
            state.entries.remove(&handle.target);
            true
        } else {
            false
        }
    }

    /// True if any handle is currently outstanding for `target`.
    pub fn is_claimed(state: &EmbedClaimState, target: &EmbedTarget) -> bool {
        state.entries.get(target).is_some_and(|e| e.refcount > 0)
    }

    /// Current refcount for `target` (0 if absent).
    pub fn refcount(state: &EmbedClaimState, target: &EmbedTarget) -> usize {
        state.entries.get(target).map(|e| e.refcount).unwrap_or(0)
    }

    /// Number of distinct targets currently being claimed. This is the
    /// "how many upstream subscriptions would be open" count — apps assert
    /// `claim_count == 1` when many components claim the same id.
    pub fn claim_count(state: &EmbedClaimState) -> usize {
        state.entries.len()
    }

    /// Look up a resolved payload, if any.
    pub fn resolved(state: &EmbedClaimState, target: &EmbedTarget) -> Option<ResolvedEvent> {
        state.entries.get(target).and_then(|e| e.resolved.clone())
    }

    fn snapshot(state: &EmbedClaimState) -> EmbedRegistrySnapshot {
        let entries = state
            .entries
            .iter()
            .map(|(t, e)| (t.clone(), e.refcount, e.resolved.clone()))
            .collect();
        EmbedRegistrySnapshot { entries }
    }

    fn update_resolution(
        state: &mut EmbedClaimState,
        target: &EmbedTarget,
        resolved: Option<ResolvedEvent>,
    ) -> Option<EmbedClaimDelta> {
        let entry = state.entries.get_mut(target)?;
        let same = entry.resolved == resolved;
        entry.resolved = resolved.clone();
        if same {
            None
        } else {
            Some(EmbedClaimDelta::Updated {
                target: target.clone(),
                refcount: entry.refcount,
                resolved,
            })
        }
    }
}

impl ViewModule for EmbedClaimRegistry {
    const NAMESPACE: &'static str = "nmp.content.embed_registry";

    type Spec = EmbedClaimSpec;
    type Payload = EmbedRegistrySnapshot;
    type Delta = EmbedClaimDelta;
    type Key = (); // singleton
    type State = EmbedClaimState;

    fn key(_spec: &Self::Spec) -> Self::Key {}

    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        // The registry consumes whatever events apps claim into it — there
        // is no static dependency declaration. The kernel-side subscription
        // opener (M16 follow-up) materialises dependencies on first claim.
        ViewDependencies::default()
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = EmbedClaimState::new();
        let payload = Self::snapshot(&state);
        (state, payload)
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta> {
        // If this event is currently claimed, update its resolution.
        let target = EmbedTarget::Event(event.id.clone());
        if state.entries.contains_key(&target) {
            return Self::update_resolution(state, &target, Some(event.into()));
        }
        // Address-coordinated: look for an Address target matching
        // (kind, author, d-tag).
        let d_tag = event
            .tags
            .iter()
            .find(|t| t.len() >= 2 && t[0] == "d")
            .map(|t| t[1].clone());
        if let Some(identifier) = d_tag {
            let target = EmbedTarget::Address {
                kind: event.kind,
                pubkey: event.author.clone(),
                identifier,
            };
            if state.entries.contains_key(&target) {
                return Self::update_resolution(state, &target, Some(event.into()));
            }
        }
        None
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        let target = EmbedTarget::Event(id.clone());
        if state.entries.contains_key(&target) {
            Self::update_resolution(state, &target, None)
        } else {
            None
        }
    }

    fn on_event_replaced(
        ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        let _ = Self::on_event_removed(ctx, state, old_id);
        Self::on_event_inserted(ctx, state, new_event)
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        Self::snapshot(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, kind: u32) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: "deadbeef".to_string(),
            kind,
            created_at: 1_700_000_000,
            tags: Vec::new(),
            content: "body".to_string(),
        }
    }

    #[test]
    fn three_claims_for_same_event_share_one_entry() {
        let mut state = EmbedClaimRegistry::state();
        let target = EmbedTarget::Event("abc".into());
        let (h1, r1) = EmbedClaimRegistry::claim(&mut state, target.clone());
        let (h2, r2) = EmbedClaimRegistry::claim(&mut state, target.clone());
        let (h3, r3) = EmbedClaimRegistry::claim(&mut state, target.clone());
        assert_eq!(EmbedClaimRegistry::claim_count(&state), 1);
        assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 3);
        assert!(r1.is_none());
        assert!(r2.is_none());
        assert!(r3.is_none());
        assert_ne!(h1.handle_id, h2.handle_id);
        assert_ne!(h2.handle_id, h3.handle_id);
    }

    #[test]
    fn last_release_returns_true_and_removes_entry() {
        let mut state = EmbedClaimRegistry::state();
        let target = EmbedTarget::Event("abc".into());
        let (h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
        let (h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
        assert!(!EmbedClaimRegistry::release(&mut state, &h1));
        assert!(EmbedClaimRegistry::release(&mut state, &h2));
        assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
        assert!(!EmbedClaimRegistry::is_claimed(&state, &target));
    }

    #[test]
    fn release_unknown_handle_returns_false() {
        let mut state = EmbedClaimRegistry::state();
        let phantom = ClaimHandle {
            target: EmbedTarget::Event("never-claimed".into()),
            handle_id: 99,
        };
        assert!(!EmbedClaimRegistry::release(&mut state, &phantom));
    }

    #[test]
    fn event_insert_updates_resolution_for_claimed_event() {
        let mut state = EmbedClaimRegistry::state();
        let id = "feedface".to_string();
        let target = EmbedTarget::Event(id.clone());
        let (_h, before) = EmbedClaimRegistry::claim(&mut state, target.clone());
        assert!(before.is_none());

        let ctx = ViewContext::default();
        let delta = <EmbedClaimRegistry as ViewModule>::on_event_inserted(&ctx, &mut state, &ev(&id, 1));
        assert!(delta.is_some());
        assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());
    }

    #[test]
    fn event_insert_for_unclaimed_target_is_noop() {
        let mut state = EmbedClaimRegistry::state();
        let ctx = ViewContext::default();
        let delta = <EmbedClaimRegistry as ViewModule>::on_event_inserted(
            &ctx,
            &mut state,
            &ev("uninterested", 1),
        );
        assert!(delta.is_none());
        assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
    }

    #[test]
    fn address_coordinated_embed_resolves_via_d_tag() {
        let mut state = EmbedClaimRegistry::state();
        let target = EmbedTarget::Address {
            kind: 30023,
            pubkey: "deadbeef".to_string(),
            identifier: "my-article".to_string(),
        };
        let (_h, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
        let mut event = ev("art-id", 30023);
        event.tags.push(vec!["d".to_string(), "my-article".to_string()]);

        let ctx = ViewContext::default();
        let delta = <EmbedClaimRegistry as ViewModule>::on_event_inserted(&ctx, &mut state, &event);
        assert!(delta.is_some());
        assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());
    }

    #[test]
    fn from_uri_skips_profile_returns_event_or_address() {
        let profile = NostrUri::Profile { pubkey: "p".into(), relays: vec![] };
        assert!(EmbedTarget::from_uri(&profile).is_none());

        let event = NostrUri::Event {
            event_id: "e".into(),
            relays: vec![],
            author: None,
            kind: None,
        };
        assert!(matches!(EmbedTarget::from_uri(&event), Some(EmbedTarget::Event(_))));

        let addr = NostrUri::Address {
            identifier: "d".into(),
            pubkey: "p".into(),
            kind: 30023,
            relays: vec![],
        };
        assert!(matches!(EmbedTarget::from_uri(&addr), Some(EmbedTarget::Address { .. })));
    }

    #[test]
    fn snapshot_includes_refcount_and_resolution() {
        let mut state = EmbedClaimRegistry::state();
        let target = EmbedTarget::Event("xyz".into());
        let (_h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
        let (_h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
        let ctx = ViewContext::default();
        let snap = <EmbedClaimRegistry as ViewModule>::snapshot(&ctx, &state);
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.entries[0].1, 2);
        assert!(snap.entries[0].2.is_none());
    }
}
