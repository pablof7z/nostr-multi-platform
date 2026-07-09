//! Typed feed-session declaration model (#1740 step 1).
//!
//! This module defines the **declaration** an app submits to open a feed:
//! [`FeedParams`] with explicit, separately-typed phases — output key,
//! acquisition source, admission policy, ordering policy, window, and item
//! projection — plus the app's primary content kinds. It also defines the
//! closed [`FeedSourceExpr`] algebra ([`FeedScope`]) used to name acquisition
//! sources, and the [`FeedHandle`] returned when a session is opened.
//!
//! Doctrine map:
//! - D0: the model names no app noun and no protocol token — variants are
//!   framework-neutral set operations and opaque registered ids, never a
//!   `Perspective` trait and never a wire/protocol kind. Primary-kind validation
//!   (which kinds are derived acquisition vs. primary input) is protocol
//!   knowledge and lives in the composition/compiler layer, not here.
//! - D8: window limits ride on the typed [`FeedWindowPolicy`].
//!
//! `open_feed` consumes this declaration through the standard feed-session
//! compiler. No native closure crosses FFI — app-defined source, admission, and
//! order policies are referenced by phase-specific opaque ids.

use nmp_ownership::{DynamicProjectionKey, SurfaceTokenError};
use serde::{Deserialize, Serialize};

mod window;
pub use window::{FeedWindowPolicy, FeedWindowResetPolicy};

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

/// Opaque identifier for an app-registered custom acquisition source.
///
/// The framework treats this as a source-phase capability only: resolving it
/// yields another closed [`FeedSourceExpr`] tree. Admission and ordering use
/// their own id types so a source capability cannot be accidentally used as a
/// gate or ranker contract.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct CustomSourceId(pub String);

/// Opaque identifier for an app-registered custom admission gate.
///
/// Resolving this id yields a closed source expression used as an event-aware
/// admission gate. It is deliberately not interchangeable with a custom source
/// or order id.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct CustomAdmissionId(pub String);

/// Opaque identifier for an app-registered custom ordering policy.
///
/// Resolving this id yields a concrete order that the current feed engine must
/// be able to honor. It is deliberately not interchangeable with source or
/// admission ids.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct CustomOrderId(pub String);

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
/// exhaustive. App-defined admission/order does **not** live here; it rides
/// on [`FeedAdmission::Custom`] / [`FeedOrder::Custom`] as an opaque id.
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
    /// Explicit target hydration from pointer events authored by `pointers`.
    ///
    /// The pointer source names the authors whose pointer events are watched;
    /// `pointer_kinds` names the pointer event kinds (for example reactions and
    /// NIP-22 comments). The compiler extracts `e`/`a` targets from those
    /// pointer events and hydrates only the feed's primary kinds. This is not
    /// ordinary feed acquisition: it is a declared hydration dependency over
    /// targets discovered from a separate pointer-event source.
    PointerTargetHydration {
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
    /// An app-defined acquisition source referenced by opaque registered id
    /// (no trait, no native closure).
    CustomSource(CustomSourceId),
}

/// Alias: the acquisition phase of a [`FeedParams`] is a [`FeedSourceExpr`].
///
/// The spec names this `FeedScope`; it is the same closed algebra. Kept as an
/// alias (not a duplicate type) so there is exactly one model (D4).
pub type FeedScope = FeedSourceExpr;

// ---------------------------------------------------------------------------
// Shape, admission, order, window, key, and item-projection phases.
// ---------------------------------------------------------------------------

/// (a) SHAPE — how the session projects acquired, admitted rows.
///
/// Only `Flat` remains: every admitted event is a top-level row. The former
/// `RootIndexed` reply-rollup shape (the baked note/reply engine) was demolished
/// (#3082) — reply-rollup is no longer a framework behavior. An app that wants a
/// "so-and-so + N others replied" rollup rebuilds it on the four generic
/// [`crate::FlatFeed`] knobs (admission / identity / sort / merge).
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedShape {
    /// Flat: every admitted event is a top-level row; the app supplies identity
    /// (dedup), sort, and merge on the generic knobs.
    #[default]
    Flat,
}

/// (b) ADMISSION policy — which acquired rows are allowed to render.
///
/// App-defined admission enters as an opaque [`CustomAdmissionId`], never a
/// native closure or trait object.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedAdmission {
    /// Admit every acquired primary-kind row (mute/block/delete suppression is
    /// applied by the kernel regardless).
    All,
    /// Admit per an app-registered admission gate (opaque id).
    Custom(CustomAdmissionId),
}

/// (c) ORDER — how admitted rows are ordered in the window.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedOrder {
    /// Newest-first by feed position.
    ///
    /// Direct event feeds currently map feed position to event `(created_at, id)`.
    /// Wrapper/protocol compilers can map this contract to source position
    /// without changing the app-facing order name.
    NewestByFeedPosition,
    /// Ordered per an app-registered order perspective (opaque id).
    Custom(CustomOrderId),
}

/// (e) OUTPUT KEY — the app-owned dynamic projection key this feed session
/// emits snapshots under.
///
/// Dynamic feed output keys are intentionally app/product-owned. Framework-owned
/// `nmp.*` projection keys must use declared projection tokens instead of this
/// dynamic lane.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectionKey(DynamicProjectionKey);

impl ProjectionKey {
    /// Build an app-owned dynamic feed projection key.
    ///
    /// Alias for [`Self::app_owned`] so app-facing examples can read as
    /// `FeedKey::app("app.my.feed")`.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceTokenError`] when `value` is empty or uses the reserved
    /// `nmp.*` framework prefix.
    pub fn app(value: impl Into<String>) -> Result<Self, SurfaceTokenError> {
        Self::app_owned(value)
    }

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

/// App-owned feed output key.
///
/// This is an alias, not a second type: the canonical serializable field on
/// [`FeedParams`] remains [`ProjectionKey`]. The alias lets builder-facing code
/// say `FeedKey::app("app.my.feed")` while retaining one runtime model.
pub type FeedKey = ProjectionKey;

/// (f) ITEM PROJECTION — the row/schema contract carried inside the feed
/// snapshot emitted under [`FeedParams::key`].
///
/// The current generic feed session emits NMP's typed feed-window rows. Keeping
/// this as an explicit declaration prevents the feed's output identity from
/// being mistaken for the row projection contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedItemProjection {
    /// NMP's generic feed-window row payload.
    FeedRows,
}

impl FeedItemProjection {
    /// NMP's generic feed-window row payload.
    #[must_use]
    pub fn feed_rows() -> Self {
        Self::FeedRows
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
    pub source: FeedScope,
    /// (c) ADMISSION policy.
    pub admission: FeedAdmission,
    /// (d) ORDER.
    pub order: FeedOrder,
    /// (e) WINDOW.
    pub window: FeedWindowPolicy,
    /// (f) OUTPUT KEY.
    pub key: ProjectionKey,
    /// (g) ITEM PROJECTION.
    pub item_projection: FeedItemProjection,
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
    ///
    /// The Rust field keeps the internal-runtime `session_id` name (#2508:
    /// "session" is legitimate runtime-bookkeeping vocabulary), but the wire
    /// serialization is `handle_id` — "session" is not public/FFI vocabulary
    /// (#2783). Every FFI-facing surface (UniFFI records, the browser wasm
    /// JSON wire, TypeScript) sees `handle_id`.
    #[serde(rename = "handle_id")]
    pub session_id: FeedSessionId,
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
