//! Canonical, app-agnostic feed surface (ADR-0061).
//!
//! This module is the framework-owned vocabulary a host uses to *describe* a
//! feed and *report viewport facts* — never to drive pagination by hand. It
//! supersedes the per-call `load_older` trigger (ADR-0033) with **Option B,
//! auto-extend from declared viewport**: the shell sends raw viewport facts
//! (`first_visible`, `last_visible`, `rendered_len`); NMP owns the
//! prefetch threshold, page size, cap, in-flight guard, exhaustion, and the
//! decision to drive the existing seq-ordered pull pager (ADR-0058).
//!
//! ## What lives here (generic mechanism only — D0)
//!
//! * [`FeedProfile`] / [`FeedSurface::install_profile`] — the `{event_kinds,
//!   renderer, page_policy}` policy a composition root installs. The `{1,6}`
//!   note-kind choice now lives in a `FeedProfile` (installed by the app
//!   composition root), NOT in a shell or an app-named ABI.
//! * [`FeedDescriptor`] (`profile` + [`FeedSource`] + [`FeedScope`]) — the
//!   single open vocabulary that collapses home / author / thread / tag /
//!   arbitrary-shape into one call.
//! * [`canonical_feed_key`] — DETERMINISTIC descriptor → [`FeedKey`]: the same
//!   descriptor always yields the same key, so a Rust caller, the C ABI, and
//!   the wasm worker all agree on one identity for a feed.
//! * [`FeedSurface`] — the per-host registry that maps a `FeedKey` to an open
//!   feed and owns the viewport-driven pagination decision. It drives the
//!   EXISTING [`FeedController`] (in production a `PullFeedController`); it adds
//!   no parallel paging mechanism.
//!
//! ## What does NOT live here
//!
//! No protocol nouns, no app names, no `InterestShape` *construction*, no feed
//! engine. A [`FeedOpener`] (installed by the composition root) resolves a
//! descriptor to an already-wired [`FeedController`] + its [`FeedPagePolicy`];
//! the surface only decides *when* to call `load_older` on it. `open` returns
//! ONLY a deterministic [`FeedHandle`] — never feed state. Projection data
//! still flows exclusively through the push snapshot frame (ADR-0039).

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use nmp_core::planner::InterestShape as CoreInterestShape;
use nmp_core::stable_hash::StableHasher;

use crate::{FeedController, DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT};

// ─── Profile vocabulary ─────────────────────────────────────────────────────────

/// Stable identifier for a [`FeedProfile`] (e.g. `"notes"`). Serializes as a
/// bare string so a descriptor reads `"profile":"notes"`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeedProfileId(pub String);

impl FeedProfileId {
    /// The profile id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for FeedProfileId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for FeedProfileId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// How the rows of a profile are laid out for rendering. App-agnostic: every
/// variant is a structural rendering shape, never an app name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedRenderer {
    /// OP-rooted timeline: followed authors' replies roll up as attribution
    /// under other people's roots (the home-feed shape).
    OpRooted,
    /// Flat list: every matching note is its own top-level row (the profile /
    /// thread shape).
    Flat,
}

/// The pagination policy NMP owns for a profile. The shell never sees these
/// numbers; they drive the viewport decision in [`FeedSurface::set_viewport`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedPagePolicy {
    /// Target number of visible rows one `load_older` drain reveals.
    pub default_page: u32,
    /// Hard ceiling on how large the rendered window may grow before NMP stops
    /// auto-extending (back-pressure against an infinite scroll runaway).
    pub cap: u32,
    /// How close to the tail the last-visible row must be (in rows remaining
    /// below it) before NMP pre-fetches the next page.
    pub prefetch_threshold: u32,
}

impl Default for FeedPagePolicy {
    fn default() -> Self {
        Self {
            default_page: DEFAULT_FEED_WINDOW_LIMIT as u32,
            cap: MAX_FEED_WINDOW_LIMIT as u32,
            prefetch_threshold: 5,
        }
    }
}

/// A registered feed profile: the `{event_kinds, renderer, page_policy}` policy
/// a composition root installs once. The `{1,6}` note-kind set lives HERE, in
/// the Rust profile registry — not in any shell or app-named ABI.
#[derive(Clone, Debug)]
pub struct FeedProfile {
    /// Stable id (e.g. `"notes"`).
    pub id: FeedProfileId,
    /// The event kinds the profile admits (e.g. `{1, 6}`).
    pub event_kinds: BTreeSet<u32>,
    /// The row layout.
    pub renderer: FeedRenderer,
    /// NMP-owned pagination policy.
    pub page_policy: FeedPagePolicy,
}

// ─── Descriptor vocabulary ──────────────────────────────────────────────────────

/// The concrete thing a feed renders. Home / author / thread / tag / arbitrary
/// shape all collapse into one open call (descriptor collapse, ADR-0061).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedSource {
    /// The active account's follow set (resolved live by NMP).
    HomeFollowSet {},
    /// A single author's notes.
    Author {
        /// Hex pubkey.
        pubkey: String,
    },
    /// A thread rooted at an event.
    #[serde(rename_all = "camelCase")]
    Thread {
        /// Hex event id of the thread root.
        root_event_id: String,
    },
    /// A NIP-12 tag query (e.g. `#t = nostr`).
    Tag {
        /// Tag name (e.g. `"t"`).
        name: String,
        /// Tag value (e.g. `"nostr"`).
        value: String,
    },
    /// An arbitrary covered interest shape (the escape hatch for feeds that do
    /// not fit the named sources).
    InterestShape {
        /// The covered shape.
        shape: CoreInterestShape,
    },
}

/// Whether a feed re-routes on account switch (`ActiveAccount`) or pins a
/// concrete subject (`Global`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedScope {
    /// Re-routed on account switch (the home feed).
    ActiveAccount,
    /// Account-agnostic; pins a concrete author / root / tag.
    Global,
}

/// The full open vocabulary. Canonicalizes (deterministically) to a
/// [`FeedKey`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FeedDescriptor {
    /// Which [`FeedProfile`] supplies the kinds / renderer / page policy.
    pub profile: FeedProfileId,
    /// The concrete subject.
    pub source: FeedSource,
    /// Active-account vs global routing.
    pub scope: FeedScope,
}

/// A deterministic, process-stable feed identity. Equal descriptors ⇒ equal
/// keys, on every platform.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeedKey(pub String);

impl FeedKey {
    /// The key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What [`FeedSurface::open`] returns — ONLY the deterministic handle, never
/// feed state (ADR-0039: data stays push-only).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedHandle {
    /// The canonical key for the opened descriptor.
    pub key: FeedKey,
}

/// Raw viewport facts the shell reports. NMP derives every pagination decision
/// from these; the shell branches on nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedViewportIntent {
    /// Index of the first rendered row currently on screen.
    pub first_visible: u32,
    /// Index of the last rendered row currently on screen.
    pub last_visible: u32,
    /// Total number of rows the shell currently has rendered.
    pub rendered_len: u32,
}

/// Render-only tail state. NEVER a behavior input — the shell shows a spinner /
/// end-cap from this and nothing else. (Wired into the projection in the
/// shell-migration PR; exposed here for tests / future binding.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TailState {
    /// More rows may exist; the shell may keep scrolling.
    IdleMore,
    /// A drain is being driven for this feed.
    Loading,
    /// Fully caught up — no more older rows.
    Exhausted,
    /// No controller is bound for this feed (open returned a handle only).
    Unavailable,
}

// ─── Deterministic canonicalization ─────────────────────────────────────────────

/// Canonical JSON for a descriptor. `serde_json::Value` orders object keys
/// (default `BTreeMap` map impl), so this is byte-stable for a given
/// descriptor regardless of struct field order or platform.
#[must_use]
pub fn canonical_descriptor_json(descriptor: &FeedDescriptor) -> String {
    serde_json::to_value(descriptor)
        .ok()
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_default()
}

/// Deterministic descriptor → [`FeedKey`]. Hashes the canonical JSON with the
/// process-stable FNV-1a hasher (`StableHasher`) — pure integer math, identical
/// on Rust / C-ABI / wasm. The `nmp.feed.` prefix keeps the key
/// namespace-readable in diagnostics.
#[must_use]
pub fn canonical_feed_key(descriptor: &FeedDescriptor) -> FeedKey {
    let json = canonical_descriptor_json(descriptor);
    let mut hasher = StableHasher::new();
    hasher.write(json.as_bytes());
    FeedKey(format!("nmp.feed.{:016x}", hasher.finish()))
}

/// Parse a descriptor from JSON, or `None` on malformed input (fail closed).
#[must_use]
pub fn parse_descriptor(descriptor_json: &str) -> Option<FeedDescriptor> {
    serde_json::from_str(descriptor_json).ok()
}

// ─── Opener seam ────────────────────────────────────────────────────────────────

/// A feed the composition root resolved for a descriptor: the already-wired
/// controller plus the page policy NMP should drive it with. The controller is
/// the EXISTING mechanism (a `PullFeedController` in production) — never a new
/// paging path.
pub struct OpenedFeed {
    /// The reusable controller (already registered for render output by the
    /// composition root).
    pub controller: Arc<dyn FeedController>,
    /// The pagination policy to drive it with.
    pub page_policy: FeedPagePolicy,
}

/// Resolves a [`FeedDescriptor`] to an [`OpenedFeed`]. Installed by the app
/// composition root (D0: the surface never names a protocol or an app). Returns
/// `None` for a descriptor this opener does not handle, so the surface can try
/// the next opener.
pub trait FeedOpener: Send + Sync {
    /// Attempt to open `descriptor`. `None` ⇒ not handled by this opener.
    fn open(&self, descriptor: &FeedDescriptor) -> Option<OpenedFeed>;
}

// ─── The surface ────────────────────────────────────────────────────────────────

/// Per-feed viewport bookkeeping NMP owns (the shell owns none of this).
struct ViewportState {
    exhausted: bool,
    /// `rendered_len` at which the last drain was driven — guards against
    /// driving a duplicate drain for the same (un-grown) viewport.
    last_serviced_len: Option<u32>,
    tail: TailState,
}

struct OpenFeed {
    controller: Option<Arc<dyn FeedController>>,
    policy: FeedPagePolicy,
    state: ViewportState,
}

/// The per-host canonical feed surface. Owns the profile registry, the opener
/// chain, the open-feed map, and the viewport-driven pagination decision.
///
/// Both the native C-ABI (`NmpApp`) and the wasm worker hold one of these and
/// route their `open_feed` / `close_feed` / `set_feed_viewport` operations
/// through it, so the same descriptor yields the same key on both platforms and
/// pagination policy lives in exactly one place.
#[derive(Default)]
pub struct FeedSurface {
    profiles: Mutex<BTreeMap<FeedProfileId, FeedProfile>>,
    openers: Mutex<Vec<Arc<dyn FeedOpener>>>,
    open_feeds: Mutex<BTreeMap<String, OpenFeed>>,
}

impl FeedSurface {
    /// Install (or replace) a profile. Idempotent; last-writer-wins.
    pub fn install_profile(&self, profile: FeedProfile) {
        if let Ok(mut profiles) = self.profiles.lock() {
            profiles.insert(profile.id.clone(), profile);
        }
    }

    /// The installed profile for `id`, if any.
    #[must_use]
    pub fn profile(&self, id: &FeedProfileId) -> Option<FeedProfile> {
        self.profiles.lock().ok().and_then(|p| p.get(id).cloned())
    }

    /// Register a descriptor → controller opener (composition-root seam).
    pub fn register_opener(&self, opener: Arc<dyn FeedOpener>) {
        if let Ok(mut openers) = self.openers.lock() {
            openers.push(opener);
        }
    }

    /// Open a feed for `descriptor_json`. Returns ONLY the deterministic handle
    /// (ADR-0039: no state — projection data stays push-only). Idempotent: a
    /// re-open of the same descriptor returns the same key without re-binding.
    ///
    /// `None` only when the descriptor JSON is malformed (fail closed). When no
    /// opener handles the descriptor, a handle is still returned (the key is
    /// deterministic) with no controller bound — viewport reports are then inert
    /// until a composition root binds the source.
    #[must_use]
    pub fn open(&self, descriptor_json: &str) -> Option<FeedHandle> {
        let descriptor = parse_descriptor(descriptor_json)?;
        let key = canonical_feed_key(&descriptor);
        let mut open_feeds = self.open_feeds.lock().ok()?;
        if open_feeds.contains_key(key.as_str()) {
            return Some(FeedHandle { key });
        }
        let opened = self
            .openers
            .lock()
            .ok()
            .and_then(|openers| openers.iter().find_map(|opener| opener.open(&descriptor)));
        let entry = match opened {
            Some(opened) => OpenFeed {
                controller: Some(opened.controller),
                policy: opened.page_policy,
                state: ViewportState {
                    exhausted: false,
                    last_serviced_len: None,
                    tail: TailState::IdleMore,
                },
            },
            None => OpenFeed {
                controller: None,
                policy: FeedPagePolicy::default(),
                state: ViewportState {
                    exhausted: false,
                    last_serviced_len: None,
                    tail: TailState::Unavailable,
                },
            },
        };
        open_feeds.insert(key.0.clone(), entry);
        Some(FeedHandle { key })
    }

    /// Close (forget) an open feed. The bound controller's own render
    /// registration is owned elsewhere (the composition root) and is untouched;
    /// this only drops the surface's viewport bookkeeping. Returns `true` when
    /// an entry was removed.
    pub fn close(&self, key: &str) -> bool {
        self.open_feeds
            .lock()
            .ok()
            .and_then(|mut open_feeds| open_feeds.remove(key))
            .is_some()
    }

    /// Report viewport facts. NMP decides — using the bound profile's
    /// [`FeedPagePolicy`] — whether to drive one `load_older` drain on the
    /// existing controller. Returns `true` when the drain progressed (the
    /// caller should re-emit the projection).
    ///
    /// The decision (Option B): drive a drain when the last-visible row is
    /// within `prefetch_threshold` of the tail, the rendered window is below
    /// `cap`, the feed is not exhausted, and this `rendered_len` has not already
    /// been serviced (the in-flight / duplicate-drain guard). A `load_older`
    /// that does not progress marks the feed exhausted (no further drains).
    pub fn set_viewport(&self, key: &str, intent: FeedViewportIntent) -> bool {
        let Ok(mut open_feeds) = self.open_feeds.lock() else {
            return false;
        };
        let Some(entry) = open_feeds.get_mut(key) else {
            return false;
        };
        let Some(controller) = entry.controller.clone() else {
            entry.state.tail = TailState::Unavailable;
            return false;
        };
        if entry.state.exhausted {
            entry.state.tail = TailState::Exhausted;
            return false;
        }

        let policy = entry.policy;
        // Rows remaining below the last-visible row.
        let near_tail =
            intent.last_visible.saturating_add(policy.prefetch_threshold) >= intent.rendered_len;
        let within_cap = intent.rendered_len < policy.cap;
        // Duplicate-drain guard: only drive when the window has grown since the
        // last drive (or this is the first viewport report for the feed).
        let fresh = entry
            .state
            .last_serviced_len
            .is_none_or(|serviced| intent.rendered_len > serviced);

        if !(near_tail && within_cap && fresh) {
            entry.state.tail = TailState::IdleMore;
            return false;
        }

        entry.state.last_serviced_len = Some(intent.rendered_len);
        // Release the map lock before driving the controller (which ingests +
        // grows the feed's window); re-acquire to record the outcome.
        drop(open_feeds);
        let progressed = controller.load_older();
        if let Ok(mut open_feeds) = self.open_feeds.lock() {
            if let Some(entry) = open_feeds.get_mut(key) {
                if progressed {
                    entry.state.tail = TailState::IdleMore;
                } else {
                    entry.state.exhausted = true;
                    entry.state.tail = TailState::Exhausted;
                }
            }
        }
        progressed
    }

    /// The current render-only tail state for `key` (diagnostics / tests).
    /// `Unavailable` for an unknown key.
    #[must_use]
    pub fn tail_state(&self, key: &str) -> TailState {
        self.open_feeds
            .lock()
            .ok()
            .and_then(|open_feeds| open_feeds.get(key).map(|entry| entry.state.tail))
            .unwrap_or(TailState::Unavailable)
    }

    /// Whether a feed is currently open under `key`.
    #[must_use]
    pub fn is_open(&self, key: &str) -> bool {
        self.open_feeds
            .lock()
            .map(|open_feeds| open_feeds.contains_key(key))
            .unwrap_or(false)
    }
}

/// Shared-ownership handle to a [`FeedSurface`], mirroring the
/// `FeedRegistrySlot` pattern: a host keeps one clone and may hand others to
/// composition-root wiring.
pub type FeedSurfaceSlot = Arc<FeedSurface>;

/// Construct a fresh, empty [`FeedSurfaceSlot`].
#[must_use]
pub fn new_feed_surface_slot() -> FeedSurfaceSlot {
    Arc::new(FeedSurface::default())
}

#[cfg(test)]
#[path = "surface/tests.rs"]
mod tests;
