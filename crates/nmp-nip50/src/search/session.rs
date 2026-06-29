//! Typed lifecycle registry for one NIP-50 search read session.
//!
//! The registry is intentionally search-shaped, not reusable across unrelated reads:
//! callers compile a validated [`crate::SearchRequest`] into one search snapshot
//! key, resolved relay pins, and teardown actions that release the machinery
//! they registered. The registry owns replacement and close semantics so hosts
//! do not keep a parallel hand-written `open`/`close` recipe per search surface.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// A single teardown step recorded when a search session opens.
///
/// Boxed `FnOnce` keeps every registered resource single-owner: once close runs,
/// the registry cannot run the same teardown again.
pub type SearchTeardownAction = Box<dyn FnOnce() + Send>;

/// The compiled lifecycle for one search session.
pub struct SearchSessionBuild {
    /// The typed `N50S` projection key surfaced to the host.
    pub projection_key: String,
    /// The resolved relay pins this session opened live demand against.
    pub relays: Vec<String>,
    /// Teardown steps in registration order. Close runs them in reverse.
    pub teardown: Vec<SearchTeardownAction>,
}

struct SearchSession {
    projection_key: String,
    relays: Vec<String>,
    teardown: Vec<SearchTeardownAction>,
}

/// Registry of live NIP-50 search sessions keyed by caller-supplied session id.
#[derive(Default)]
pub struct SearchSessionRegistry {
    sessions: Mutex<BTreeMap<String, SearchSession>>,
}

impl SearchSessionRegistry {
    /// Open or replace `session_id` with `build`.
    ///
    /// Replacing a live id first closes the old session, then records the new
    /// one. If the registry cannot take ownership of the new build, it tears the
    /// build down immediately and reports failure.
    pub fn open(&self, session_id: impl Into<String>, build: SearchSessionBuild) -> bool {
        let session_id = session_id.into();
        self.close(&session_id);

        let SearchSessionBuild {
            projection_key,
            relays,
            teardown,
        } = build;
        match self.sessions.lock() {
            Ok(mut sessions) => {
                sessions.insert(
                    session_id,
                    SearchSession {
                        projection_key,
                        relays,
                        teardown,
                    },
                );
                true
            }
            Err(_) => {
                run_teardown(teardown);
                false
            }
        }
    }

    /// Close `session_id`, running its teardown exactly once.
    pub fn close(&self, session_id: &str) -> bool {
        let session = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(session_id));
        let Some(session) = session else {
            return false;
        };
        run_teardown(session.teardown);
        true
    }

    /// Return the projection key for a live session.
    #[must_use]
    pub fn projection_key(&self, session_id: &str) -> Option<String> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(session_id).map(|s| s.projection_key.clone()))
    }

    /// Return the resolved live relay pins for a session.
    ///
    /// Diagnostic/test surface: this proves empty relay resolution stays
    /// fail-closed and never becomes wildcard demand.
    #[must_use]
    pub fn relays(&self, session_id: &str) -> Vec<String> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(session_id).map(|s| s.relays.clone()))
            .unwrap_or_default()
    }

    /// Count live sessions for contract tests.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }
}

fn run_teardown(teardown: Vec<SearchTeardownAction>) {
    for action in teardown.into_iter().rev() {
        action();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn replacing_a_search_session_runs_old_teardown_once() {
        let registry = SearchSessionRegistry::default();
        let log = Arc::new(Mutex::new(Vec::new()));

        let first_log = Arc::clone(&log);
        assert!(registry.open(
            "s1",
            SearchSessionBuild {
                projection_key: "nmp.nip50.search.s1".to_string(),
                relays: vec!["wss://one/".to_string()],
                teardown: vec![Box::new(move || {
                    first_log.lock().unwrap().push("old");
                })],
            },
        ));

        let second_log = Arc::clone(&log);
        assert!(registry.open(
            "s1",
            SearchSessionBuild {
                projection_key: "nmp.nip50.search.s1".to_string(),
                relays: vec!["wss://two/".to_string()],
                teardown: vec![Box::new(move || {
                    second_log.lock().unwrap().push("new");
                })],
            },
        ));

        assert_eq!(registry.relays("s1"), vec!["wss://two/"]);
        assert_eq!(&*log.lock().unwrap(), &["old"]);
        assert!(registry.close("s1"));
        assert!(!registry.close("s1"));
        assert_eq!(&*log.lock().unwrap(), &["old", "new"]);
    }

    #[test]
    fn close_runs_teardown_in_reverse_registration_order() {
        let registry = SearchSessionRegistry::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::clone(&log);
        let second = Arc::clone(&log);

        assert!(registry.open(
            "s1",
            SearchSessionBuild {
                projection_key: "key".to_string(),
                relays: Vec::new(),
                teardown: vec![
                    Box::new(move || first.lock().unwrap().push("projection")),
                    Box::new(move || second.lock().unwrap().push("live")),
                ],
            },
        ));

        assert!(registry.close("s1"));
        assert_eq!(&*log.lock().unwrap(), &["live", "projection"]);
    }
}
