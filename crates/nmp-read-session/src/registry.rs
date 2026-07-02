//! The ONE read-session registry — concept-neutral handle ownership of an open
//! read's full lifecycle, with idempotent handle-driven reverse teardown.
//!
//! Extracted from the feed-session registry (#1740 step 2, `nmp_feed::session`)
//! and generalized to be concept-neutral (#2777 step 1): it is keyed by an
//! OPAQUE projection-key string, so feed (app-owned keys), replies (framework
//! `nmp.replies.*` keys), search, and group-feed all record their sessions in
//! ONE registry → one leak audit. It never names an engine, a follow set, a
//! feed row, or any app noun. Each session records its teardown as an opaque
//! list of [`TeardownAction`] closures the concept-driven [`crate::open_read`]
//! supplies; closing a session runs exactly those actions, in reverse
//! registration order, **once**.
//!
//! Doctrine map:
//! - D0: no app/protocol noun. Sessions hold an opaque projection-key string +
//!   opaque teardown closures; what a read *is* lives in its concept crate.
//! - D6: a poisoned lock degrades to a best-effort no-op, never a panic; double
//!   close is a safe no-op.
//! - D8: a closed session is removed from the map and its closures dropped, so
//!   nothing the session held outlives the close (no leak).

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

/// A single teardown step recorded when a read opens.
///
/// Boxed `FnOnce` so each action runs exactly once on close. `Send` so the
/// registry (held behind an `Arc<Mutex<…>>` on a host) is `Send`.
pub type TeardownAction = Box<dyn FnOnce() + Send>;

/// An opaque, monotonically-minted read-session identifier.
///
/// The engine mints these; a concept pairs one with its projection key to form
/// the handle it returns. `0` is a reserved sentinel that is never minted (it
/// signals "open failed, already torn down").
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReadSessionId(pub u64);

/// One open read session: the projection key it surfaces under, plus the ordered
/// list of teardown actions that release everything the open wired.
struct ReadSession {
    projection_key: String,
    /// Teardown steps in **registration** order. [`close`](ReadSessionRegistry::close)
    /// runs them in reverse so the last thing wired is the first released.
    teardown: Vec<TeardownAction>,
}

/// The recipe [`crate::open_read`] hands to [`ReadSessionRegistry::open`]: the
/// projection key the session emits under and the ordered teardown actions that
/// release everything the open registered.
pub struct ReadSessionBuild {
    /// The projection key the opened read's snapshots surface under (opaque).
    pub projection_key: String,
    /// Teardown steps in registration order (run reversed on close).
    pub teardown: Vec<TeardownAction>,
}

/// Registry of live read sessions, keyed by an opaque, monotonically-minted
/// [`ReadSessionId`].
///
/// `open` mints an id and stores the session's projection key + teardown recipe.
/// `close` removes the session and runs its teardown actions **once**; a second
/// close of the same id is a no-op. The registry is the sole owner of the
/// teardown closures, so dropping a session (on close or on registry drop)
/// releases everything they capture (D8). This is the single instance a host
/// keeps so every open read across every concept lands in one leak audit.
pub struct ReadSessionRegistry {
    next_id: AtomicU64,
    sessions: Mutex<std::collections::BTreeMap<ReadSessionId, ReadSession>>,
}

impl Default for ReadSessionRegistry {
    fn default() -> Self {
        Self {
            // Start at 1 so `ReadSessionId(0)` stays reserved as the sentinel.
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

impl ReadSessionRegistry {
    /// Mint a new session id, record the build's projection key + teardown
    /// recipe, and return the id. The caller pairs this id with
    /// `build.projection_key` to form the handle it returns to the app.
    ///
    /// D6 — a poisoned lock means the registry cannot track the session; the
    /// build's teardown actions are run immediately (so nothing the open
    /// registered leaks) and the reserved sentinel `ReadSessionId(0)` is
    /// returned. A higher layer treats `0` as "open failed, already torn down".
    pub fn open(&self, build: ReadSessionBuild) -> ReadSessionId {
        let id = ReadSessionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let ReadSessionBuild {
            projection_key,
            teardown,
        } = build;
        match self.sessions.lock() {
            Ok(mut sessions) => {
                sessions.insert(
                    id,
                    ReadSession {
                        projection_key,
                        teardown,
                    },
                );
                id
            }
            Err(_) => {
                run_teardown(teardown);
                ReadSessionId(0)
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
    pub fn close(&self, id: &ReadSessionId) -> bool {
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
    pub fn is_open(&self, id: &ReadSessionId) -> bool {
        self.sessions
            .lock()
            .map(|sessions| sessions.contains_key(id))
            .unwrap_or(false)
    }

    /// The number of live sessions — the ONE leak audit across every concept
    /// read (proves teardown frees the map entry rather than flipping a flag).
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// The projection key of a live session, or `None` if absent (diagnostic +
    /// the handle-ownership check a concept's close uses before tearing down).
    #[must_use]
    pub fn projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(id).map(|s| s.projection_key.clone()))
    }
}

/// Run teardown actions in reverse registration order — the last resource wired
/// is the first released (nested-acquire / reverse-release discipline, D8).
fn run_teardown(teardown: Vec<TeardownAction>) {
    for action in teardown.into_iter().rev() {
        action();
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
