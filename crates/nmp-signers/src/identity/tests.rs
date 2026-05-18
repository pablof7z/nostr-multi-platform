//! Identity-module unit tests.  Integration tests (cross-module) live under
//! `tests/`.

use std::sync::Arc;

use super::*;
use crate::signers::{LocalKeySigner, Signer};

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

#[test]
fn duplicate_add_rejected() {
    let mut mgr = AccountManager::new();
    let key = LocalKeySigner::generate();
    let key_hex = key.secret_hex();
    let a = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let b = LocalKeySigner::from_secret_hex(&key_hex).unwrap();
    let _id = mgr.add(Arc::new(a)).unwrap();
    let err = mgr.add(Arc::new(b)).unwrap_err();
    assert!(matches!(err, AccountError::AlreadyExists(_)));
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

    let obs = Arc::new(Kind3RewireObserver::new());
    mgr.observe(obs.clone());
    mgr.switch_active(&id).unwrap();
    assert_eq!(obs.pending_count(), 0, "no-op switch must not fire observers");
}

#[test]
fn observer_fires_on_active_change() {
    let mut mgr = AccountManager::new();
    let obs = Arc::new(Kind3RewireObserver::new());
    mgr.observe(obs.clone());

    let id_a = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    let id_b = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();

    mgr.switch_active(&id_a).unwrap();
    mgr.switch_active(&id_b).unwrap();
    mgr.switch_active(&id_a).unwrap();

    let events = obs.drain();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].previous, None);
    assert_eq!(events[0].current, id_a);
    assert_eq!(events[1].previous.as_deref(), Some(id_a.as_str()));
    assert_eq!(events[1].current, id_b);
    assert_eq!(events[2].previous.as_deref(), Some(id_b.as_str()));
    assert_eq!(events[2].current, id_a);
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
