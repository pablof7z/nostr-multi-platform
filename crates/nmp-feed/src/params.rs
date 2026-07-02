//! Typed feed-session declaration model (#1740 step 1).
//!
//! This module defines the **declaration** an app submits to open a feed:
//! [`FeedParams`] with explicit, separately-typed phases — acquisition source,
//! admission policy, ranking/order, window, and item projection — plus the
//! app's primary content kinds. It also defines the closed [`FeedSourceExpr`]
//! algebra ([`FeedScope`]) used to name acquisition sources, and the
//! [`FeedHandle`] returned when a session is opened.
//!
//! Doctrine map:
//! - D0: the model names no app noun and no protocol token — variants are
//!   framework-neutral set operations and opaque registered ids, never a
//!   `Perspective` trait and never a wire/protocol kind. Primary-kind validation
//!   (which kinds are derived acquisition vs. primary input) is protocol
//!   knowledge and lives in the composition/compiler layer, not here.
//! - D8: window limits ride on the typed [`FeedWindow`].
//!
//! Step 1 is **definition only**: `open_feed` does not yet consume these
//! (step 2). No native closure crosses FFI — app-defined admission/ranking is
//! referenced by an opaque [`CustomPerspectiveId`].

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT};
use nmp_ownership::{DynamicProjectionKey, SurfaceTokenError};

// ---------------------------------------------------------------------------
// Acquisition source — the closed `FeedSourceExpr` algebra.
// ---------------------------------------------------------------------------

/// Opaque identifier for an app-registered list (a NIP-51 set, a curated id).
///
/// The framework treats it as an opaque key; resolution to concrete members is
/// app/protocol-owned and happens below this declaration. (D0: the framework
/// does not interpret what the list *means*.)
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct ListId(pub String);

/// Opaque identifier for an app-defined admission/ranking perspective.
///
/// This is how app-defined policy enters the model **without** a `Perspective`
/// trait and **without** a native closure crossing FFI. The app registers its
/// admission/ranking logic out-of-band and names it here by id; the kernel sees
/// only the opaque id and dispatches the compiled, pre-registered predicate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct CustomPerspectiveId(pub String);

/// A web-of-trust seed pubkey (lowercase hex).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct WotSeed(pub String);

/// Opaque, app-registered WoT scoring/expansion rule id.
///
/// The concrete hop count / scoring function is registered out-of-band and
/// referenced by id, so no native closure crosses FFI.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct WotRulesId(pub String);

/// Opaque, app-registered relay-set id.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct RelaySetId(pub String);

/// A `#t` tag value or a free-text search term, scoping acquisition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct TagTerm(pub String);

/// A closed, exhaustive typed algebra for naming the source a feed draws from.
///
/// This is the framework-neutral source phase: it never names an app product.
/// It is a closed enum — adding a new acquisition shape is a deliberate,
/// reviewed model change, and every consumer that matches on it must stay
/// exhaustive. App-defined admission/ranking does **not** live here; it rides
/// on [`FeedAdmission::Custom`] / [`FeedRanking::Custom`] as an opaque id.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedSourceExpr {
    /// The active account's own follow set (reactive perspective state derived
    /// from the active account's kind:3; re-routed on account switch). The app
    /// supplies no concrete pubkeys.
    ActiveUserFollows,
    /// A STATIC, app-named author set: the primary-kind timeline authored BY
    /// these concrete pubkeys.
    ///
    /// Distinct from [`Self::ContactList`] — that names an owner whose *follows*
    /// (kind:3) seed the scope; this names the authors THEMSELVES. The app
    /// supplies the resolved hex pubkeys directly (e.g. one author for a profile
    /// screen, several for a curated author list), so the scope is fixed at
    /// declaration: it compiles to a fixed [`AdmitExpr::Authors`] admission over a
    /// fixed author+kind acquisition (no reactive projection, no account
    /// re-routing). An EMPTY set is fail-closed by the resolver (admits nobody,
    /// acquires nothing) — never silently "all authors".
    Authors {
        authors: std::collections::BTreeSet<String>,
    },
    /// The contact list (kind:3 follows) of a specific owner pubkey.
    ContactList { owner: String },
    /// The members of an app/defaults-registered list id.
    ///
    /// `nmp-feed` treats the id as opaque. The composition layer decides
    /// whether it names an addressable list, an active-account replaceable
    /// list, a curated source, or another protocol-owned pubkey-set reducer.
    ListMembers { list: ListId },
    /// A web-of-trust expansion from `seed` under an opaque, registered ruleset.
    Wot { seed: WotSeed, rules: WotRulesId },
    /// An app-registered relay set — acquisition is routed to those relays
    /// without synthesizing `authors`/`#p`/`#a`/`#e` filters.
    RelaySet { relays: RelaySetId },
    /// A `#t` tag or free-text search scope.
    Tag { term: TagTerm },
    /// Thread / referrer scope: the root event (by id) plus every primary-kind
    /// event or derived wrapper that references the root via an `#e` tag.
    /// An EMPTY event_id fails closed (admits nobody).
    Referrer { event_id: String },
    /// Target events referenced by pointer events authored by `pointers`.
    ///
    /// The pointer source names the authors whose pointer events are watched;
    /// `pointer_kinds` names the pointer event kinds (for example reactions and
    /// NIP-22 comments). The compiler extracts `e`/`a` targets from those
    /// pointer events and hydrates only the feed's primary kinds.
    PointerTargets {
        pointers: Box<FeedSourceExpr>,
        pointer_kinds: Vec<u32>,
    },
    /// The active account's hosted group set.
    ///
    /// This source draws rows from relay-hosted groups, not pubkeys. The
    /// composition compiler resolves the active account's group declarations
    /// and emits one relay-pinned group-tag interest per host relay. Empty
    /// declaration or no active account fails closed.
    ActiveUserHostedGroups,
    /// Set union of two sub-expressions.
    Union(Box<FeedSourceExpr>, Box<FeedSourceExpr>),
    /// Set intersection of two sub-expressions.
    Intersection(Box<FeedSourceExpr>, Box<FeedSourceExpr>),
    /// Set difference: members of `0` that are not in `1`.
    Difference(Box<FeedSourceExpr>, Box<FeedSourceExpr>),
    /// An app-defined acquisition perspective referenced by opaque registered
    /// id (no trait, no native closure).
    CustomPerspectiveId(CustomPerspectiveId),
}

/// Alias: the acquisition phase of a [`FeedParams`] is a [`FeedSourceExpr`].
///
/// The spec names this `FeedScope`; it is the same closed algebra. Kept as an
/// alias (not a duplicate type) so there is exactly one model (D4).
pub type FeedScope = FeedSourceExpr;

// ---------------------------------------------------------------------------
// Shape, admission, ranking, window, projection phases.
// ---------------------------------------------------------------------------

/// (a) SHAPE — how the session projects acquired, admitted rows.
///
/// `RootIndexed` produces a root-indexed reply rollup (OP-feed engine).
/// `Flat` produces a flat list with empty attribution (NIP-01 FlatFeed engine),
/// used for profile and thread screens.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedShape {
    /// Reply-centric: replies roll up as attribution under their parent OP.
    /// (root-indexed OP-feed engine)
    RootIndexed,
    /// Flat: every matching event is a top-level row; no attribution nesting.
    /// (profile/thread style, FlatFeed engine)
    Flat,
}

impl Default for FeedShape {
    fn default() -> Self {
        FeedShape::RootIndexed
    }
}

/// (b) ADMISSION policy — which acquired rows are allowed to render.
///
/// App-defined admission enters as an opaque [`CustomPerspectiveId`], never a
/// native closure or trait object.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedAdmission {
    /// Admit every acquired primary-kind row (mute/block/delete suppression is
    /// applied by the kernel regardless).
    All,
    /// Admit per an app-registered admission perspective (opaque id).
    Custom(CustomPerspectiveId),
}

/// (c) RANKING / ORDER — how admitted rows are ordered in the window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedRanking {
    /// Newest-first by `(created_at, id)` — the default chronological order.
    ChronologicalDesc,
    /// Oldest-first by `(created_at, id)`.
    ChronologicalAsc,
    /// Ranked per an app-registered ranking perspective (opaque id).
    Custom(CustomPerspectiveId),
}

/// (d) WINDOW — the bounded viewport over admitted, ranked rows (D8).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeedWindow {
    /// Initial visible page size. Clamped into
    /// `1..=MAX_FEED_WINDOW_LIMIT` by [`FeedWindow::bounded_limit`].
    pub initial_limit: usize,
}

impl Default for FeedWindow {
    fn default() -> Self {
        Self {
            initial_limit: DEFAULT_FEED_WINDOW_LIMIT,
        }
    }
}

impl FeedWindow {
    /// The window limit clamped into the bounded range. A zero limit falls back
    /// to the default; oversized limits are capped at [`MAX_FEED_WINDOW_LIMIT`].
    #[must_use]
    pub fn bounded_limit(&self) -> usize {
        if self.initial_limit == 0 {
            DEFAULT_FEED_WINDOW_LIMIT
        } else {
            self.initial_limit.min(MAX_FEED_WINDOW_LIMIT)
        }
    }
}

/// (e) ITEM PROJECTION — the app-owned dynamic projection key that renders
/// admitted rows into cards.
///
/// Dynamic feed keys are intentionally app/product-owned. Framework-owned
/// `nmp.*` projection keys must use declared projection tokens instead of this
/// dynamic lane.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectionKey(DynamicProjectionKey);

impl ProjectionKey {
    /// Build an app-owned dynamic feed projection key.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceTokenError`] when `value` is empty or uses the reserved
    /// `nmp.*` framework prefix.
    pub fn app_owned(value: impl Into<String>) -> Result<Self, SurfaceTokenError> {
        DynamicProjectionKey::app_owned(value).map(Self)
    }

    /// Borrow the projection key string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Clone the underlying dynamic projection token for registration APIs.
    #[must_use]
    pub fn dynamic_token(&self) -> DynamicProjectionKey {
        self.0.clone()
    }

    /// Consume into the owned key string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

impl TryFrom<String> for ProjectionKey {
    type Error = SurfaceTokenError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::app_owned(value)
    }
}

impl From<ProjectionKey> for String {
    fn from(value: ProjectionKey) -> Self {
        value.into_string()
    }
}

// ---------------------------------------------------------------------------
// FeedParams — the full typed declaration.
// ---------------------------------------------------------------------------

/// The typed declaration an app submits to open a feed session.
///
/// Each phase is a distinct typed field — not one opaque closure, and not three
/// separate lifecycles. The kernel consumes the **validated** form (step 2);
/// step 1 only defines and validates it.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeedParams {
    /// The app's PRIMARY content kinds (e.g. `[1]`, `[20]`, `[30023]`).
    ///
    /// These are the content kinds the app intends to render. Derived
    /// acquisition kinds (protocol wrapper kinds and the deletion kind) are NOT
    /// declared here — they are compiled below the app boundary by the
    /// composition/compiler layer, which validates that no derived-acquisition
    /// kind was declared as primary input (fail-closed).
    pub primary_kinds: Vec<u32>,
    /// (a) SHAPE.
    #[serde(default)]
    pub shape: FeedShape,
    /// (b) ACQUISITION source.
    pub acquisition: FeedScope,
    /// (c) ADMISSION policy.
    pub admission: FeedAdmission,
    /// (d) RANKING / ORDER.
    pub ranking: FeedRanking,
    /// (e) WINDOW.
    pub window: FeedWindow,
    /// (f) ITEM PROJECTION.
    pub projection: ProjectionKey,
}

/// Opaque feed-session identifier minted by the kernel when a session opens.
///
/// Apps treat it as an opaque token; only the kernel interprets it. Defined
/// here so step 2 can return it without reshaping the model.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct FeedSessionId(pub u64);

/// The handle returned when a feed session is opened.
///
/// Pairs the (opaque, registered) projection key the snapshot will surface under
/// with the opaque session id the kernel uses to address the live session.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeedHandle {
    /// The projection key whose snapshots this session emits.
    pub projection_key: ProjectionKey,
    /// Opaque session id (kernel-minted).
    pub session_id: FeedSessionId,
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
