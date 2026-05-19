//! kind:3 (contacts) auto-rewire.
//!
//! When the active account flips, downstream "your follows" subscriptions must
//! rebuild against the new account's follow-set + kind:10002 relay list.  The
//! kernel installs `Kind3RewireObserver` as an `ActiveChangeObserver` on the
//! `AccountManager`.  The observer captures the new active account and stages
//! a rewire request.
//!
//! The actual subscription teardown / rebuild happens in the kernel's planner
//! (it owns the relay pool — D7 capability-vs-policy split); this module only
//! signals.

use std::sync::{Arc, Mutex};

use super::manager::{ActiveChangeEvent, ActiveChangeObserver, IdentityId};

/// One rewire request — emitted on every active-account transition,
/// including removal-of-active (`current = None`, codex review #5 —
/// 9944bed.md).  The kernel-side planner translates each event into the
/// appropriate subscription rebuild or teardown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kind3RewireEvent {
    /// Previous active account, if any.
    pub previous: Option<IdentityId>,
    /// New active account.  `None` means the active slot was cleared (the
    /// active account was removed); downstream code tears down the kind:3
    /// subscription and emits a `FullState` with no active account.
    pub current: Option<IdentityId>,
}

/// Observer that captures rewire events into an internal buffer.  The kernel
/// drains the buffer on each actor tick.
#[derive(Debug, Default)]
pub struct Kind3RewireObserver {
    inner: Arc<Mutex<Vec<Kind3RewireEvent>>>,
}

impl Kind3RewireObserver {
    /// Construct an empty observer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Drain pending rewire events.  Returns insertion-order; clears the
    /// buffer.
    pub fn drain(&self) -> Vec<Kind3RewireEvent> {
        match self.inner.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => Vec::new(),
        }
    }

    /// Peek pending count without draining (test convenience).
    pub fn pending_count(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }
}

impl ActiveChangeObserver for Kind3RewireObserver {
    fn on_active_change(&self, event: &ActiveChangeEvent) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.push(Kind3RewireEvent {
                previous: event.previous.clone(),
                current: event.current.clone(),
            });
        }
    }
}
