//! Feed-session registry (#1740 step 2) — feed's typed door onto the ONE
//! read-lifecycle engine (`nmp-read-session`, #2777 step 1).
//!
//! A feed session is one open feed's full lifecycle: the projection key, the
//! registered observer/interest ids, the pull-controller registration, and the
//! typed sidecar. The mechanics — handle allocation, open/close, reverse
//! teardown, one leak audit — are NOT re-implemented here: they are the single
//! [`nmp_read_session::ReadSessionRegistry`] that every concept-owned active
//! read shares. This type only adds feed's typed surface over that engine: a
//! [`ProjectionKey`]-typed build and a [`FeedSessionId`] handle id. Feed
//! therefore "compiles through the extracted spine" — there is no second feed
//! engine (D4) and no second registry (#2777).
//!
//! Why a closure list and not a typed teardown record: the concrete things to
//! release (an `unregister_feed(key)`, an observed-projection close, a
//! dependent-interest clear) live above this crate, wired by explicit per-app
//! composition on top of `nmp-native-runtime` / `nmp-uniffi`. Recording them as
//! opaque `FnOnce` actions keeps the registry at the bottom of the DAG (D4).
//!
//! Doctrine map:
//! - D0: no app noun. Sessions hold an opaque [`ProjectionKey`] + opaque
//!   teardown closures; the algebra of what a feed *is* lives in `params.rs`.
//! - D4: this is a wrapper, not a second feed engine. The lifecycle mechanics
//!   are the shared engine.
//! - D6: a poisoned lock degrades to a best-effort no-op; double close is safe.
//! - D8: a closed session is removed and its closures dropped, so nothing it
//!   held outlives the close (no leak).

use nmp_read_session::{ReadSessionBuild, ReadSessionId, ReadSessionRegistry};

use crate::params::{FeedSessionId, ProjectionKey};

/// A single teardown step recorded when a session opens.
///
/// Re-exported from the engine so feed composition code names one teardown type.
pub type TeardownAction = nmp_read_session::TeardownAction;

/// The recipe an `open_feed` compiler hands to [`FeedSessionRegistry::open`]:
/// the projection key the session emits under, and the ordered teardown actions
/// that release everything the compile registered.
///
/// This is feed's [`ProjectionKey`]-typed door onto the engine's
/// [`ReadSessionBuild`]; the compiler performs the actual registration and
/// returns here only the key + how to undo it.
pub struct FeedSessionBuild {
    /// The projection key the opened session's snapshots surface under.
    pub projection_key: ProjectionKey,
    /// Teardown steps in registration order (run reversed on close).
    pub teardown: Vec<TeardownAction>,
}

impl FeedSessionBuild {
    /// Lower feed's typed build into the concept-neutral engine build (opaque
    /// projection-key string). This is the seam through which a feed session
    /// enters the shared registry.
    #[must_use]
    pub fn into_read_session_build(self) -> ReadSessionBuild {
        ReadSessionBuild {
            projection_key: self.projection_key.into_string(),
            teardown: self.teardown,
            demand_set: None,
        }
    }
}

/// Registry of live feed sessions — feed's [`FeedSessionId`]-keyed door onto the
/// shared [`ReadSessionRegistry`].
///
/// `open` records the session's projection key + teardown recipe in the engine
/// and returns the feed handle id; `close` runs the engine's reverse teardown
/// exactly once. Feed adds no lifecycle logic of its own.
#[derive(Default)]
pub struct FeedSessionRegistry {
    inner: ReadSessionRegistry,
}

impl FeedSessionRegistry {
    /// Record the build in the shared engine and return the minted feed session
    /// id (`FeedSessionId(0)` sentinel when the engine could not track it).
    pub fn open(&self, build: FeedSessionBuild) -> FeedSessionId {
        FeedSessionId(self.inner.open(build.into_read_session_build()).0)
    }

    /// Close the session `id`, running its reverse teardown exactly once.
    /// Idempotent (D6).
    pub fn close(&self, id: &FeedSessionId) -> bool {
        self.inner.close(&ReadSessionId(id.0))
    }

    /// Whether the session `id` is currently live (test/diagnostic).
    #[must_use]
    pub fn is_open(&self, id: &FeedSessionId) -> bool {
        self.inner.is_open(&ReadSessionId(id.0))
    }

    /// The number of live feed sessions (part of the engine's one leak audit).
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.inner.live_count()
    }

    /// Borrow the shared engine registry underneath feed's typed door.
    ///
    /// A runtime keeps ONE [`FeedSessionRegistry`] and drives every OTHER
    /// concept-owned active read (replies, reactions, …) through this same
    /// engine registry via the [`nmp_read_session::ReadHost`] seam, so all open
    /// reads land in one leak audit ([`Self::live_count`]) — not a per-concept
    /// registry each (#2777).
    #[must_use]
    pub fn as_read_sessions(&self) -> &ReadSessionRegistry {
        &self.inner
    }

    /// The projection key of a live session, or `None` if absent (diagnostic).
    #[must_use]
    pub fn projection_key(&self, id: &FeedSessionId) -> Option<ProjectionKey> {
        self.inner
            .projection_key(&ReadSessionId(id.0))
            .and_then(|key| ProjectionKey::app_owned(key).ok())
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
