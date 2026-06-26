//! Feed-session registry (#1740 step 2) — handle-based ownership of one open
//! feed's full lifecycle, with idempotent handle-driven teardown.
//!
//! A feed session is the single owner of everything an `open_feed` call wired:
//! the projection key, the registered observer/interest ids, the pull
//! controller registration, and the typed sidecar projection. The registry here
//! is **engine-agnostic** (D0/D4): it never names an OP-feed engine, a follow
//! set, or any app noun. Each session records its teardown as an opaque list of
//! [`TeardownAction`] closures the composition layer supplies; closing a session
//! runs exactly those actions, in reverse registration order, **once**.
//!
//! Why a closure list and not a typed teardown record: the concrete things to
//! release (an `unregister_feed(key)`, an observed-projection close, a
//! dependent-interest clear) live above this crate in `nmp-ffi` / `nmp-defaults`.
//! Recording them as opaque `FnOnce` actions keeps the session
//! registry from importing those layers (it sits at the bottom of the DAG) and
//! keeps a single source of truth for feed state: the registry owns no feed
//! state of its own, only the recipe to release whatever the existing mechanics
//! registered (D4 — no second feed engine).
//!
//! Doctrine map:
//! - D0: no app noun. Sessions hold an opaque [`ProjectionKey`] + opaque
//!   teardown closures; the algebra of what a feed *is* lives in `params.rs`.
//! - D4: the registry is a wrapper, not a second feed engine. Teardown reuses
//!   the existing per-mechanism unregister paths via the recorded closures.
//! - D6: a poisoned lock degrades to a best-effort no-op, never a panic; double
//!   close is a safe no-op.
//! - D8: a closed session is removed from the map and its closures dropped, so
//!   nothing the session held outlives the close (no leak).

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use crate::params::{FeedSessionId, ProjectionKey};

/// A single teardown step recorded when a session opens.
///
/// Boxed `FnOnce` so each action runs exactly once on close. `Send` so the
/// session registry (held behind an `Arc<Mutex<…>>` on `NmpApp`) is `Send`.
pub type TeardownAction = Box<dyn FnOnce() + Send>;

/// One open feed session: the projection key it surfaces under, plus the ordered
/// list of teardown actions that release everything the open wired.
struct FeedSession {
    projection_key: ProjectionKey,
    /// Teardown steps in **registration** order. [`close`](FeedSessionRegistry::close)
    /// runs them in reverse so the last thing wired is the first released.
    teardown: Vec<TeardownAction>,
}

/// The recipe an `open_feed` compiler hands to [`FeedSessionRegistry::open`]:
/// the projection key the session emits under, and the ordered teardown actions
/// that release everything the compile registered.
///
/// This is the single value that crosses from the composition layer
/// (`nmp-defaults`, which names the OP-feed engine) into the engine-agnostic
/// session registry. The compiler is responsible for performing the actual
/// registration; it returns here only the key + how to undo it.
pub struct FeedSessionBuild {
    /// The projection key the opened session's snapshots surface under.
    pub projection_key: ProjectionKey,
    /// Teardown steps in registration order (run reversed on close).
    pub teardown: Vec<TeardownAction>,
}

/// Registry of live feed sessions, keyed by an opaque, monotonically-minted
/// [`FeedSessionId`].
///
/// `open` mints an id and stores the session's projection key + teardown recipe.
/// `close` removes the session and runs its teardown actions **once**; a second
/// close of the same id is a no-op (the entry is already gone). The registry is
/// the sole owner of the teardown closures, so dropping a session (on close or
/// on registry drop) releases everything they capture (D8).
pub struct FeedSessionRegistry {
    next_id: AtomicU64,
    sessions: Mutex<std::collections::BTreeMap<FeedSessionId, FeedSession>>,
}

impl Default for FeedSessionRegistry {
    fn default() -> Self {
        Self {
            // Start at 1 so a `FeedSessionId(0)` can be reserved as a sentinel
            // by higher layers if ever needed; 0 is never minted.
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

impl FeedSessionRegistry {
    /// Mint a new session id, record the build's projection key + teardown
    /// recipe, and return the id. The caller pairs this id with
    /// `build.projection_key` to form the `FeedHandle` it returns to the app.
    ///
    /// D6 — a poisoned sessions lock means the registry cannot track the
    /// session; the build's teardown actions are run immediately (so nothing
    /// the compile registered leaks) and a sentinel `FeedSessionId(0)` is
    /// returned. A higher layer treats `0` as "open failed, already torn down".
    pub fn open(&self, build: FeedSessionBuild) -> FeedSessionId {
        let id = FeedSessionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let FeedSessionBuild {
            projection_key,
            teardown,
        } = build;
        match self.sessions.lock() {
            Ok(mut sessions) => {
                sessions.insert(
                    id.clone(),
                    FeedSession {
                        projection_key,
                        teardown,
                    },
                );
                id
            }
            Err(_) => {
                // Cannot track it — release immediately so nothing leaks, and
                // signal failure with the reserved sentinel id.
                run_teardown(teardown);
                FeedSessionId(0)
            }
        }
    }

    /// Close the session identified by `id`, running its teardown actions in
    /// reverse registration order **exactly once**, and returning `true` when a
    /// session was actually present and torn down.
    ///
    /// Idempotent (D6): closing an unknown or already-closed id is a no-op that
    /// returns `false`, never a panic. The session entry is removed *before* its
    /// closures run, so the registry no longer references the session's
    /// resources once teardown begins (no leak, no re-entrancy on the lock).
    pub fn close(&self, id: &FeedSessionId) -> bool {
        // Remove under the lock, then drop the lock before running teardown so a
        // teardown action may itself touch the registry without deadlocking.
        let session = match self.sessions.lock() {
            Ok(mut sessions) => sessions.remove(id),
            Err(_) => None, // poisoned ⇒ fail closed, treat as already gone
        };
        match session {
            Some(session) => {
                run_teardown(session.teardown);
                true
            }
            None => false,
        }
    }

    /// Whether a session with `id` is currently live (test/diagnostic).
    #[must_use]
    pub fn is_open(&self, id: &FeedSessionId) -> bool {
        self.sessions
            .lock()
            .map(|sessions| sessions.contains_key(id))
            .unwrap_or(false)
    }

    /// The number of live sessions (test/diagnostic — proves teardown frees the
    /// map entry rather than merely flipping a flag).
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// The projection key of a live session, or `None` if absent (diagnostic).
    #[must_use]
    pub fn projection_key(&self, id: &FeedSessionId) -> Option<ProjectionKey> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(id).map(|s| s.projection_key.clone()))
    }
}

/// Run teardown actions in reverse registration order. Reverse order means the
/// last resource wired is the first released — the standard nested-acquire /
/// reverse-release discipline.
fn run_teardown(teardown: Vec<TeardownAction>) {
    for action in teardown.into_iter().rev() {
        action();
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
