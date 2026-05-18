//! `ActiveAccountReactor` — observer + command-bundle translator for
//! active-account transitions.
//!
//! Lives in `nmp-signers` because `AccountManager` + `ActiveChangeObserver`
//! are defined here; the reactor wraps that observer surface and produces a
//! deterministic command bundle the kernel actor consumes inside one tick.
//!
//! ## Design split (D4 — single writer per fact)
//!
//! The observer is invoked synchronously by `AccountManager::switch_active`
//! on the caller thread. In production the caller IS the actor thread, so
//! the observer must not block. It does the cheapest possible work — drop
//! the event into a `Mutex<Vec<_>>` — and lets the actor drain on its next
//! tick.
//!
//! The translation step (`bundle_for`) is a pure function. The actor calls
//! `drain()` once per tick, then `bundle_for(&event)` for each event, then
//! executes the resulting `ActiveSwitchCommand`s in order:
//!
//! 1. `CloseAccountSubs { author }` — close kind:3, kind:10000, kind:10002
//!    subscriptions scoped to the previous active (and any FollowingTimeline
//!    whose root is that author).
//! 2. `RebindPublishSigner { signer: Option<id> }` — call
//!    `manager.signer_active()` and install on the publish engine.
//! 3. `OpenAccountSubs { author }` — open the equivalent subscriptions for
//!    the new active account.
//! 4. `EmitFullState` — flush one `AppUpdate::FullState` snapshot AFTER all
//!    rebuilds complete (D5 atomicity — observers see consistent state).
//!
//! ## Why a bundle and not separate observer methods
//!
//! Atomicity. If the actor's tick observes "rebind happened" but not "subs
//! closed", a stray inbound event for account-A could be misattributed to
//! account-B's signer context. The bundle is the unit of work the actor
//! commits atomically inside one tick.
//!
//! ## What this module does NOT do
//!
//! - It does NOT execute the bundle. The kernel actor is the executor (D4).
//! - It does NOT touch sockets, the EventStore, or the publish engine. Those
//!   live in `nmp-core` and would create a circular dep.
//! - It does NOT decide which subscriptions are account-scoped. That's a
//!   planner concern. This module only names the closure points.

use std::sync::{Arc, Mutex};

use super::manager::{ActiveChangeEvent, ActiveChangeObserver, IdentityId};

/// Commands the kernel actor executes (in order) for one active-account
/// transition. The reactor produces these; the actor's tick consumes them.
///
/// The actor MUST execute all commands in a single tick — splitting across
/// ticks violates D5 atomicity (intermediate snapshots would be observable
/// with mixed-account state).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActiveSwitchCommand {
    /// Close all account-scoped subscriptions whose author is this id
    /// (kind:3, kind:10000, kind:10002, FollowingTimeline rooted at id).
    CloseAccountSubs {
        /// Author whose account-scoped subs must be torn down. None means
        /// "no previous active" (initial sign-in) and is a no-op.
        author: Option<IdentityId>,
    },
    /// Rebind the publish engine's signer. `signer_id` is the new active id;
    /// `None` means clear (active was removed).
    RebindPublishSigner {
        /// Which identity is now the active signer, or None to clear.
        signer_id: Option<IdentityId>,
    },
    /// Open account-scoped subscriptions for this author (kind:3, kind:10000,
    /// kind:10002, FollowingTimeline). `None` means no new active and is a
    /// no-op.
    OpenAccountSubs {
        /// Author whose account-scoped subs must be opened. None on removal.
        author: Option<IdentityId>,
    },
    /// After all the above complete, emit one `AppUpdate::FullState`. D5.
    EmitFullState,
}

/// One active-account transition captured by the reactor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveSwitch {
    /// Previous active account, if any.
    pub previous: Option<IdentityId>,
    /// New active account, or `None` if the active slot was cleared.
    pub current: Option<IdentityId>,
}

impl From<&ActiveChangeEvent> for ActiveSwitch {
    fn from(ev: &ActiveChangeEvent) -> Self {
        Self {
            previous: ev.previous.clone(),
            current: ev.current.clone(),
        }
    }
}

/// Reactor that captures `ActiveChangeEvent`s into an internal buffer.
///
/// Mirrors the `Kind3RewireObserver` drain pattern intentionally — both run
/// on the actor thread and both have the same hot-path constraint (do not
/// block). The kernel installs ONE reactor + one rewire-observer on the
/// `AccountManager`; the actor drains both each tick.
#[derive(Debug, Default)]
pub struct ActiveAccountReactor {
    inner: Arc<Mutex<Vec<ActiveSwitch>>>,
}

impl ActiveAccountReactor {
    /// Construct an empty reactor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain pending switches in insertion order. Clears the internal buffer.
    /// Called by the actor on each tick.
    pub fn drain(&self) -> Vec<ActiveSwitch> {
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

impl ActiveChangeObserver for ActiveAccountReactor {
    fn on_active_change(&self, event: &ActiveChangeEvent) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.push(ActiveSwitch::from(event));
        }
    }
}

/// Translate one `ActiveSwitch` into the ordered command bundle the actor
/// executes atomically.
///
/// Pure function — no I/O, no allocation beyond the returned `Vec`. The
/// actor calls this for each drained switch and feeds the result into its
/// dispatch loop inside the same tick.
///
/// ## Atomicity contract
///
/// The returned vec is the unit of atomicity. The actor MUST execute all
/// four commands in one tick. The closing of old subs MUST happen before
/// the rebind, which MUST happen before the opening of new subs, which MUST
/// happen before the snapshot. Re-ordering breaks D5.
pub fn bundle_for(switch: &ActiveSwitch) -> Vec<ActiveSwitchCommand> {
    vec![
        ActiveSwitchCommand::CloseAccountSubs {
            author: switch.previous.clone(),
        },
        ActiveSwitchCommand::RebindPublishSigner {
            signer_id: switch.current.clone(),
        },
        ActiveSwitchCommand::OpenAccountSubs {
            author: switch.current.clone(),
        },
        ActiveSwitchCommand::EmitFullState,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::PublicKey;

    fn ev(previous: Option<&str>, current: Option<&str>) -> ActiveChangeEvent {
        ActiveChangeEvent {
            previous: previous.map(String::from),
            current: current.map(String::from),
            current_pubkey: current.map(|hex| {
                // PublicKey::from_hex requires 64-char hex; pad with "0"s.
                let padded = format!("{:0<64}", hex);
                PublicKey::from_hex(&padded).expect("test pubkey")
            }),
        }
    }

    #[test]
    fn observer_captures_event_into_buffer() {
        let reactor = ActiveAccountReactor::new();
        reactor.on_active_change(&ev(None, Some("a")));
        assert_eq!(reactor.pending_count(), 1);
    }

    #[test]
    fn drain_returns_insertion_order_and_clears() {
        let reactor = ActiveAccountReactor::new();
        reactor.on_active_change(&ev(None, Some("a")));
        reactor.on_active_change(&ev(Some("a"), Some("b")));
        reactor.on_active_change(&ev(Some("b"), Some("c")));
        let drained = reactor.drain();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].current.as_deref(), Some("a"));
        assert_eq!(drained[1].current.as_deref(), Some("b"));
        assert_eq!(drained[2].current.as_deref(), Some("c"));
        assert_eq!(reactor.pending_count(), 0);
    }

    #[test]
    fn drain_on_empty_returns_empty() {
        let reactor = ActiveAccountReactor::new();
        assert!(reactor.drain().is_empty());
    }

    #[test]
    fn bundle_for_initial_signin_skips_close() {
        // previous = None, current = Some(a) — initial sign-in case.
        let switch = ActiveSwitch {
            previous: None,
            current: Some("a".to_string()),
        };
        let bundle = bundle_for(&switch);
        assert_eq!(bundle.len(), 4);
        assert_eq!(
            bundle[0],
            ActiveSwitchCommand::CloseAccountSubs { author: None }
        );
        assert_eq!(
            bundle[1],
            ActiveSwitchCommand::RebindPublishSigner {
                signer_id: Some("a".to_string())
            }
        );
        assert_eq!(
            bundle[2],
            ActiveSwitchCommand::OpenAccountSubs {
                author: Some("a".to_string())
            }
        );
        assert_eq!(bundle[3], ActiveSwitchCommand::EmitFullState);
    }

    #[test]
    fn bundle_for_switch_closes_old_opens_new() {
        let switch = ActiveSwitch {
            previous: Some("a".to_string()),
            current: Some("b".to_string()),
        };
        let bundle = bundle_for(&switch);
        // Order MUST be: close → rebind → open → emit (D5 atomicity).
        match &bundle[0] {
            ActiveSwitchCommand::CloseAccountSubs { author } => {
                assert_eq!(author.as_deref(), Some("a"));
            }
            other => panic!("expected CloseAccountSubs first, got {:?}", other),
        }
        match &bundle[1] {
            ActiveSwitchCommand::RebindPublishSigner { signer_id } => {
                assert_eq!(signer_id.as_deref(), Some("b"));
            }
            other => panic!("expected RebindPublishSigner second, got {:?}", other),
        }
        match &bundle[2] {
            ActiveSwitchCommand::OpenAccountSubs { author } => {
                assert_eq!(author.as_deref(), Some("b"));
            }
            other => panic!("expected OpenAccountSubs third, got {:?}", other),
        }
        assert_eq!(bundle[3], ActiveSwitchCommand::EmitFullState);
    }

    #[test]
    fn bundle_for_removal_clears_signer_and_subs() {
        // previous = Some(a), current = None — active account removed.
        let switch = ActiveSwitch {
            previous: Some("a".to_string()),
            current: None,
        };
        let bundle = bundle_for(&switch);
        assert_eq!(
            bundle[0],
            ActiveSwitchCommand::CloseAccountSubs {
                author: Some("a".to_string())
            }
        );
        assert_eq!(
            bundle[1],
            ActiveSwitchCommand::RebindPublishSigner { signer_id: None }
        );
        assert_eq!(
            bundle[2],
            ActiveSwitchCommand::OpenAccountSubs { author: None }
        );
        assert_eq!(bundle[3], ActiveSwitchCommand::EmitFullState);
    }

    #[test]
    fn observer_is_send_sync_for_arc_dyn_storage() {
        // Compile-time check: the AccountManager stores observers as
        // Arc<dyn ActiveChangeObserver>, which requires Send + Sync. If
        // ActiveAccountReactor lost either trait this test fails to compile.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ActiveAccountReactor>();
        let _arc: Arc<dyn ActiveChangeObserver> = Arc::new(ActiveAccountReactor::new());
    }

    #[test]
    fn observer_drains_via_arc_clone_of_inner_buffer() {
        // Real-world wiring: the kernel constructs the reactor, registers
        // an Arc<reactor> with the manager (cloned), and keeps another Arc
        // for draining on the actor tick. Verify drain works through either
        // handle (Arc<Mutex<>> shares state across clones).
        let reactor = Arc::new(ActiveAccountReactor::new());
        let manager_handle: Arc<dyn ActiveChangeObserver> = reactor.clone();
        manager_handle.on_active_change(&ev(None, Some("a")));
        let drained = reactor.drain();
        assert_eq!(drained.len(), 1);
    }
}
