//! Reusable host-side composition for the NIP-46 actor-lane runtime.
//!
//! [`register_nip46`] is the canonical, app-neutral wiring for the NIP-46
//! actor-relay lane.  It mirrors the shape of `nmp-nip47::register::register_wallet`:
//!
//! 1. Creates a [`Nip46RuntimeHandle`].
//! 2. Installs a [`Nip46Interceptor`] on the relay-text interceptor slot.
//! 3. Installs a [`Nip46ConnectedHook`] on the relay-connected-hook slot.
//! 4. Returns the handle so the caller can store it and start sessions via
//!    [`crate::runtime::init_bunker`] / [`crate::runtime::init_nostrconnect`].
//!
//! ## When to call
//!
//! During the app's **config phase** (before `kernel.start()`), so the actor
//! reads the registered hooks once at kernel construction.
//!
//! ## PR-B2: broker deleted
//!
//! `NmpApp::init_signer_broker` (in `nmp-native-runtime`) calls
//! `register_nip46` and the broker transport (`nmp-signer-broker`) is
//! deleted.  The actor-lane runtime is now the sole NIP-46 transport.
//!
//! ## Layer cleanliness
//!
//! This function depends only on the narrow substrate registrar traits it
//! uses (`RelayTextInterceptorRegistrar + RelayConnectedHookRegistrar`), not
//! on `NmpApp` or any FFI type.  It names no C-ABI symbol; the native-runtime
//! adapter in `nmp-native-runtime` calls it with `app.actor_sender()` as the
//! `command_sender`.

use std::sync::Arc;

use nmp_core::substrate::{RelayConnectedHookRegistrar, RelayTextInterceptorRegistrar};
use nmp_core::CommandSender;

use crate::connected_hook::Nip46ConnectedHook;
use crate::interceptor::Nip46Interceptor;
use crate::runtime::{new_nip46_runtime_handle, Nip46RuntimeHandle};

/// Install the NIP-46 actor-lane runtime on `app`.
///
/// Registers:
/// - A [`Nip46Interceptor`] on the relay-text interceptor slot (inbound
///   kind:24133 decoder + effect translator).
/// - A [`Nip46ConnectedHook`] on the relay-connected-hook slot (REQ replay
///   on (re)connect, deadline arming, connection-state reporting).
///
/// Returns the [`Nip46RuntimeHandle`] so the caller can start sessions.
///
/// # Arguments
///
/// - `app`: any type implementing both `RelayTextInterceptorRegistrar` and
///   `RelayConnectedHookRegistrar` (e.g. `NmpApp` in production, a test
///   double in tests).  Both traits use `&self` interior-mutability (e.g.
///   `NmpApp`'s lock-based registrar impls), so this takes `&impl` rather than
///   `&mut impl` — `NmpApp` is shared behind an `Arc`/UniFFI object handle in
///   `nmp-native-runtime`, so `&mut` access would violate aliasing rules.
/// - `command_sender`: a clone of the actor's waking-inbox sender (obtained
///   from `app.actor_sender()` in the `nmp-native-runtime` adapter).  The
///   interceptor uses it to post actor commands (add_signer,
///   bunker_handshake_progress, …) without holding the kernel mutex.
pub fn register_nip46(
    app: &(impl RelayTextInterceptorRegistrar + RelayConnectedHookRegistrar),
    command_sender: CommandSender,
) -> Nip46RuntimeHandle {
    let handle = new_nip46_runtime_handle();

    // Install the relay-text interceptor.
    app.add_relay_text_interceptor(Arc::new(Nip46Interceptor {
        runtime: Arc::clone(&handle),
        sender: command_sender,
    }));

    // Install the relay-connected hook (REQ replay on reconnect).
    app.add_relay_connected_hook(Arc::new(Nip46ConnectedHook {
        runtime: Arc::clone(&handle),
    }));

    handle
}
