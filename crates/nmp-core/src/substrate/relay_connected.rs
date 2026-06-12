//! `RelayConnectedHook` — substrate-generic seam for protocol crates that need
//! to react when a relay socket opens.
//!
//! # Why this exists
//!
//! Some protocol-crate work is *lifecycle-driven* — it is triggered by a relay
//! connecting, not by a host command (so it does not fit the command-shaped
//! [`crate::substrate::ProtocolCommand`] seam) and not by an inbound frame (so
//! it does not fit [`crate::substrate::RelayTextInterceptor`]):
//!
//! * `nmp-nip11` fetches a relay's information document
//!   (`application/nostr+json`) the first time the relay connects, subject to a
//!   per-URL TTL, then posts it back via [`crate::ActorCommand::SetRelayInfo`].
//!
//! `RelayConnectedHook` lifts that reaction out of `nmp-core`: on
//! `PoolEvent::Opened` the actor reaches into the host-installed slot and gives
//! each registered hook the freshly-connected URL plus an owned
//! [`CommandSender`] it can hand to a spawned worker (the canonical
//! off-thread-fetch → post-result-back pattern). The sender is the ADR-0050
//! §D3a waking inbox handle, so a worker posting its result genuinely wakes
//! the actor. The trait is substrate-generic; `nmp-core` never names
//! `nmp-nip11` (D0).
//!
//! ## D8 — the hook must not block
//!
//! `on_relay_connected` runs on the actor thread. It MUST only *spawn* work
//! (e.g. `std::thread::spawn` a `ureq` GET) and return immediately — never
//! perform blocking I/O inline. The worker posts its result back through the
//! cloned sender after the actor loop has moved on.

use std::sync::{Arc, Mutex};

use crate::CommandSender;

/// A protocol-crate-owned hook the actor calls when a relay socket opens.
///
/// The hook decides for itself whether to act (e.g. `nmp-nip11` checks its
/// per-URL TTL before spawning a fetch). It is handed the canonical relay URL
/// and a [`CommandSender`] clone for posting follow-up commands back into
/// the actor loop.
///
/// `Send + Sync` so the slot can be a shared `Arc<dyn …>` cloned to the FFI
/// surface.
pub trait RelayConnectedHook: Send + Sync + 'static {
    /// React to `relay_url` having just connected. MUST NOT block (D8): spawn a
    /// worker and return. `command_sender` is an owned clone the worker keeps to
    /// post results (e.g. [`crate::ActorCommand::SetRelayInfo`]) back into the
    /// actor loop — sends through it wake the actor (ADR-0050 §D3a).
    ///
    /// `is_reconnect` is `false` on the first `Opened` for a URL and `true` on
    /// every subsequent reconnect — hooks that only need a one-shot fetch can
    /// ignore reconnects (the TTL gate makes a refetch idempotent regardless).
    fn on_relay_connected(
        &self,
        relay_url: &str,
        is_reconnect: bool,
        command_sender: CommandSender,
    );
}

/// Shared slot holding active [`RelayConnectedHook`]s.
///
/// `Arc<Mutex<Vec<Arc<dyn …>>>>` so the slot is host-mutable during app
/// construction without `&mut self`, and the inner `Arc`s can be cloned out
/// under the lock and invoked outside it (no long-held mutex around hook
/// bodies). Multiple protocol crates can react to the same connect.
pub type RelayConnectedHookSlot = Arc<Mutex<Vec<Arc<dyn RelayConnectedHook>>>>;

/// Construct a fresh, empty [`RelayConnectedHookSlot`].
#[must_use]
pub fn new_relay_connected_hook_slot() -> RelayConnectedHookSlot {
    Arc::new(Mutex::new(Vec::new()))
}

/// Install a hook into the slot. Helper so protocol crates do not reach into
/// the `Mutex` directly.
pub fn install_relay_connected_hook(slot: &RelayConnectedHookSlot, hook: Arc<dyn RelayConnectedHook>) {
    if let Ok(mut hooks) = slot.lock() {
        hooks.push(hook);
    }
}

/// Fan a connect notification to every installed hook. The actor calls this
/// from the `PoolEvent::Opened` arm. Clones the `Arc`s out under the lock and
/// invokes them outside it so a hook body never holds the slot mutex.
pub fn fan_relay_connected(
    slot: &RelayConnectedHookSlot,
    relay_url: &str,
    is_reconnect: bool,
    command_sender: &CommandSender,
) {
    let hooks: Vec<Arc<dyn RelayConnectedHook>> = match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return,
    };
    for hook in hooks {
        // D15: a panicking hook adapter must not unwind the actor's dispatch
        // frame. Each hook gets its own sender clone.
        let sender = command_sender.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hook.on_relay_connected(relay_url, is_reconnect, sender);
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHook {
        calls: Arc<AtomicUsize>,
        last_reconnect: Arc<Mutex<Option<bool>>>,
    }

    impl RelayConnectedHook for CountingHook {
        fn on_relay_connected(
            &self,
            _relay_url: &str,
            is_reconnect: bool,
            _command_sender: CommandSender,
        ) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_reconnect.lock().unwrap() = Some(is_reconnect);
        }
    }

    /// A throwaway waking-inbox sender for tests (receiver kept alive by the
    /// caller via the returned pair).
    fn test_sender() -> (CommandSender, std::sync::mpsc::Receiver<crate::ActorMail>) {
        let (tx, rx) = std::sync::mpsc::channel::<crate::ActorMail>();
        (CommandSender::new(tx), rx)
    }

    #[test]
    fn fan_invokes_every_installed_hook() {
        let slot = new_relay_connected_hook_slot();
        let calls = Arc::new(AtomicUsize::new(0));
        let last = Arc::new(Mutex::new(None));
        install_relay_connected_hook(
            &slot,
            Arc::new(CountingHook {
                calls: calls.clone(),
                last_reconnect: last.clone(),
            }),
        );
        install_relay_connected_hook(
            &slot,
            Arc::new(CountingHook {
                calls: calls.clone(),
                last_reconnect: last.clone(),
            }),
        );

        let (tx, _rx) = test_sender();
        fan_relay_connected(&slot, "wss://relay.example", true, &tx);

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(*last.lock().unwrap(), Some(true));
    }

    #[test]
    fn empty_slot_is_a_noop() {
        let slot = new_relay_connected_hook_slot();
        let (tx, _rx) = test_sender();
        // Must not panic with no hooks installed.
        fan_relay_connected(&slot, "wss://relay.example", false, &tx);
    }

    struct PanickingHook;
    impl RelayConnectedHook for PanickingHook {
        fn on_relay_connected(&self, _u: &str, _r: bool, _s: CommandSender) {
            panic!("hook adapter panicked");
        }
    }

    #[test]
    fn panicking_hook_does_not_unwind_the_caller() {
        let slot = new_relay_connected_hook_slot();
        install_relay_connected_hook(&slot, Arc::new(PanickingHook));
        let (tx, _rx) = test_sender();
        // catch_unwind inside fan_relay_connected must contain the panic.
        fan_relay_connected(&slot, "wss://relay.example", false, &tx);
    }
}
