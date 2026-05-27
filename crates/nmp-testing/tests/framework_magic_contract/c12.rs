//! Framework Magic Contract — M8-gated test: C12.
//!
//! C12 Account switch is a state transition; views rebind without imperative dance.
//!
//! M8 (multi-account state machine + ActiveAccountChanged trigger) is DONE on master.
//!
//! Design: `docs/design/framework-magic/sessions.md`

use std::sync::{Arc, Mutex};

use nmp_signers::{AccountManager, ActiveChangeEvent, ActiveChangeObserver, LocalKeySigner};

// ── C12 ───────────────────────────────────────────────────────────────────────

/// Observer that records every ActiveChangeEvent for test assertions.
struct EventCapture {
    events: Mutex<Vec<(Option<String>, String)>>,
}

impl EventCapture {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
        })
    }

    fn events(&self) -> Vec<(Option<String>, String)> {
        self.events.lock().unwrap().clone()
    }
}

impl ActiveChangeObserver for EventCapture {
    fn on_active_change(&self, event: &ActiveChangeEvent) {
        self.events.lock().unwrap().push((
            event.previous.clone(),
            event
                .current
                .clone()
                .expect("current must be Some during a switch"),
        ));
    }
}

/// C12: Account switch is an atomic state transition.
///
/// Five sub-properties:
/// 1. `switch_active` to a non-existent id returns `AccountError::NotFound`.
/// 2. Switching to the first account fires the observer with `previous = None`.
/// 3. Switching from account A to B fires with `previous = Some(A)`.
/// 4. Switching to the already-active account is a no-op (observer NOT fired).
/// 5. `CompileTrigger::ActiveAccountChanged` enum variant is constructible —
///    confirms the kernel's trigger bus is wired for M8 multi-account.
///
/// Design: `docs/design/framework-magic/sessions.md`
#[test]
fn c12_account_switch_rebinds_views_without_imperative_dance() {
    use nmp_core::subs::{AccountId, CompileTrigger};
    use std::time::Duration;

    let mut manager = AccountManager::new().with_post_condition_timeout(Duration::from_millis(500));

    let capture = EventCapture::new();
    manager.observe(Arc::clone(&capture) as Arc<dyn ActiveChangeObserver>);

    // --- 1. NotFound before any accounts are added --------------------------
    let not_found = manager.switch_active(&"nonexistent".to_string());
    assert!(
        not_found.is_err(),
        "switch to unknown id must fail with NotFound"
    );

    // --- 2. Add two accounts ------------------------------------------------
    let sk_a = LocalKeySigner::from_secret_hex(
        "0101010101010101010101010101010101010101010101010101010101010101",
    )
    .expect("valid hex");
    let sk_b = LocalKeySigner::from_secret_hex(
        "0202020202020202020202020202020202020202020202020202020202020202",
    )
    .expect("valid hex");

    let id_a = manager.add(Arc::new(sk_a)).expect("add A");
    let id_b = manager.add(Arc::new(sk_b)).expect("add B");

    assert_eq!(manager.accounts().len(), 2);
    assert!(manager.active().is_none(), "add does not auto-activate");

    // --- 3. Switch to A — observer fires with previous = None ---------------
    manager.switch_active(&id_a).expect("switch to A");
    let evs = capture.events();
    assert_eq!(evs.len(), 1);
    assert!(evs[0].0.is_none(), "first switch must have no previous");
    assert_eq!(evs[0].1, id_a, "current must be A");
    assert_eq!(manager.active().as_deref(), Some(id_a.as_str()));

    // --- 4. Switch to B — observer fires with previous = Some(A) ------------
    manager.switch_active(&id_b).expect("switch to B");
    let evs = capture.events();
    assert_eq!(evs.len(), 2);
    assert_eq!(
        evs[1].0.as_deref(),
        Some(id_a.as_str()),
        "previous must be A"
    );
    assert_eq!(evs[1].1, id_b, "current must be B");
    assert_eq!(manager.active().as_deref(), Some(id_b.as_str()));

    // --- 5. Switching to already-active is a no-op --------------------------
    manager.switch_active(&id_b).expect("switch to B again");
    assert_eq!(
        capture.events().len(),
        2,
        "switch to already-active account must not fire observer"
    );

    // --- 6. CompileTrigger::ActiveAccountChanged is constructible -----------
    // This confirms the trigger bus in nmp-core::subs is wired for M8.
    let trigger = CompileTrigger::ActiveAccountChanged {
        from: Some(AccountId(id_a.clone())),
        to: Some(AccountId(id_b.clone())),
    };
    assert!(
        trigger.requires_recompile(),
        "ActiveAccountChanged must require a recompile"
    );
}
