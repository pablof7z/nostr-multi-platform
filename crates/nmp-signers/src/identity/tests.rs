//! Identity-module unit tests.  Integration tests (cross-module) live under
//! `tests/`.

use std::sync::{Arc, Mutex};

use super::*;
use crate::signers::{LocalKeySigner, Signer};

/// Minimal `ActiveChangeObserver` probe for asserting `AccountManager`
/// transition semantics. Replaces the deleted `Kind3RewireObserver` (which was
/// dead production scaffolding — active-account subscription rebuilds are
/// handled directly by the `SwitchActive` actor command in `nmp-core`).
#[derive(Debug, Default)]
struct ProbeObserver {
    events: Mutex<Vec<ActiveChangeEvent>>,
}

impl ProbeObserver {
    fn new() -> Self {
        Self::default()
    }

    fn pending_count(&self) -> usize {
        self.events.lock().map(|g| g.len()).unwrap_or(0)
    }

    fn drain(&self) -> Vec<ActiveChangeEvent> {
        match self.events.lock() {
            Ok(mut g) => std::mem::take(&mut *g),
            Err(_) => Vec::new(),
        }
    }
}

impl ActiveChangeObserver for ProbeObserver {
    fn on_active_change(&self, event: &ActiveChangeEvent) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event.clone());
        }
    }
}

#[test]
fn add_and_active_lifecycle() {
    let mut mgr = AccountManager::new();
    let a = LocalKeySigner::generate();
    let id_a = mgr.add(Arc::new(a)).expect("add a");

    assert_eq!(mgr.accounts(), vec![id_a.clone()]);
    assert!(mgr.active().is_none());
    assert!(mgr.signer_active().is_none());

    mgr.switch_active(&id_a).expect("switch");
    assert_eq!(mgr.active().as_deref(), Some(id_a.as_str()));
    assert!(mgr.signer_active().is_some());
}

/// PD-004 (same nsec = same account): adding the same nsec twice yields
/// exactly one account.  `IdentityId == pubkey_hex` is permanent; the
/// applesauce "two accounts for one pubkey" model is rejected.
#[test]
fn adding_same_nsec_twice_yields_exactly_one_account() {
    let mut mgr = AccountManager::new();
    let key = LocalKeySigner::generate();
    let key_hex = key.secret_hex();
    let a = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let b = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let id1 = mgr.add(Arc::new(a)).expect("first add");
    let id2 = mgr.add(Arc::new(b)).expect("second add is an idempotent no-op");

    assert_eq!(id1, id2, "same nsec must map to the same IdentityId");
    assert_eq!(mgr.accounts(), vec![id1.clone()], "exactly one slot");
    assert!(mgr.signer_for(&id1).is_some(), "signer still resolves");
}

/// PD-004: `add_unverified` (restoration path) is equally idempotent — a
/// known pubkey never opens a second slot and keeps the original signer.
#[test]
fn add_unverified_same_pubkey_is_noop_and_mixed_paths_keep_one_slot() {
    let mut mgr = AccountManager::new();
    let key_hex = LocalKeySigner::generate().secret_hex();

    let first = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let id1 = mgr.add(Arc::new(first)).expect("verified add");
    let original = mgr.signer_for(&id1).expect("installed");

    // add_unverified with the same pubkey: no-op, original signer retained.
    let dup = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let id2 = mgr.add_unverified(Arc::new(dup)).expect("idempotent no-op");
    // add (verified path) with the same pubkey again: still one slot.
    let dup2 = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let id3 = mgr.add(Arc::new(dup2)).expect("idempotent no-op");

    assert_eq!(id1, id2);
    assert_eq!(id1, id3);
    assert_eq!(mgr.accounts(), vec![id1.clone()], "AccountManager is the sole writer of account identity (D4): exactly one slot");
    assert!(
        Arc::ptr_eq(&original, &mgr.signer_for(&id1).unwrap()),
        "originally-installed signer must be retained, not replaced"
    );
}

#[test]
fn switch_to_unknown_errors() {
    let mut mgr = AccountManager::new();
    let err = mgr.switch_active(&"unknown".to_string()).unwrap_err();
    assert!(matches!(err, AccountError::NotFound(_)));
}

#[test]
fn switch_to_same_is_noop() {
    let mut mgr = AccountManager::new();
    let id = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    mgr.switch_active(&id).unwrap();

    let obs = Arc::new(ProbeObserver::new());
    mgr.observe(obs.clone());
    mgr.switch_active(&id).unwrap();
    assert_eq!(obs.pending_count(), 0, "no-op switch must not fire observers");
}

#[test]
fn observer_fires_on_active_change() {
    let mut mgr = AccountManager::new();
    let obs = Arc::new(ProbeObserver::new());
    mgr.observe(obs.clone());

    let id_a = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    let id_b = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();

    mgr.switch_active(&id_a).unwrap();
    mgr.switch_active(&id_b).unwrap();
    mgr.switch_active(&id_a).unwrap();

    let events = obs.drain();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].previous, None);
    assert_eq!(events[0].current.as_deref(), Some(id_a.as_str()));
    assert_eq!(events[1].previous.as_deref(), Some(id_a.as_str()));
    assert_eq!(events[1].current.as_deref(), Some(id_b.as_str()));
    assert_eq!(events[2].previous.as_deref(), Some(id_b.as_str()));
    assert_eq!(events[2].current.as_deref(), Some(id_a.as_str()));
}

#[test]
fn remove_active_clears_active() {
    let mut mgr = AccountManager::new();
    let id = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    mgr.switch_active(&id).unwrap();
    mgr.remove(&id).unwrap();
    assert!(mgr.active().is_none());
    assert!(mgr.signer_active().is_none());
    assert!(mgr.accounts().is_empty());
}

#[test]
fn signer_active_returns_correct_for_each_switch() {
    let mut mgr = AccountManager::new();
    let signer_a = LocalKeySigner::generate();
    let pubkey_a = signer_a.pubkey();
    let signer_b = LocalKeySigner::generate();
    let pubkey_b = signer_b.pubkey();
    let id_a = mgr.add(Arc::new(signer_a)).unwrap();
    let id_b = mgr.add(Arc::new(signer_b)).unwrap();

    mgr.switch_active(&id_a).unwrap();
    assert_eq!(mgr.signer_active().unwrap().pubkey(), pubkey_a);
    mgr.switch_active(&id_b).unwrap();
    assert_eq!(mgr.signer_active().unwrap().pubkey(), pubkey_b);
}
