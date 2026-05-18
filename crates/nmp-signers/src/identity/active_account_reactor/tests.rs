//! Unit tests for `ActiveAccountReactor`.
//!
//! Split from `mod.rs` to keep the implementation file under the 300 LOC
//! soft cap (AGENTS.md). 8 tests cover: observer capture, drain
//! insertion-order + clears, empty drain, initial signin (previous=None),
//! normal switch, removal (current=None), Send+Sync compile gate,
//! Arc-clone drain wiring.

use std::sync::Arc;

use nostr::PublicKey;

use super::{bundle_for, ActiveAccountReactor, ActiveSwitch, ActiveSwitchCommand};
use crate::identity::manager::{ActiveChangeEvent, ActiveChangeObserver};

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
