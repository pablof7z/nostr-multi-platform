//! Browser relay handler closure bag — enqueue-only callbacks (#2050).
//!
//! # D4 single-writer invariant
//!
//! `build_handlers` produces a [`BrowserKernelHandlers`] bag whose closures
//! ONLY enqueue [`InboundRelayEvent`]s into the shared `InboundQueue` and call
//! `wake()`. They NEVER borrow or mutate the `KernelReducer`. The reducer is
//! mutated exclusively inside `pump()` via [`super::inbound::drain_inbound`].
//!
//! This is the key difference from the retired `nmp-wasm` relay-pool
//! implementation, where the handler closures called `reducer.borrow_mut()`
//! directly. That pattern breaks D4 (sole-writer) on the browser runtime's
//! owned-by-value architecture.
//!
//! # Wake contract
//!
//! `wake: WakeCell` is the shared, stable "please schedule a pump" indirection
//! (see [`super::WakeCell`]). The closures clone the *cell* (not the inner
//! closure) at construction and invoke the current closure via
//! [`super::fire_wake`] at callback time, so a host that installs the real wake
//! via `set_wake` *after* `spawn_bootstrap` built these handlers is still
//! observed. On wasm32 production the host sets it to a function that schedules
//! a 0ms timer which calls `pump()` on the shared runtime handle.

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::BrowserKernelHandlers;
#[cfg(target_arch = "wasm32")]
use nmp_network::role::RelayRole;

#[cfg(target_arch = "wasm32")]
use super::inbound::{InboundQueue, InboundRelayEvent};
#[cfg(target_arch = "wasm32")]
use super::{fire_wake, WakeCell};

/// Build a [`BrowserKernelHandlers`] whose closures enqueue events (never
/// mutate the reducer). Called once from `spawn_bootstrap` on wasm32.
///
/// `inbound` is `Rc`-shared and `wake` is the shared [`WakeCell`] so the
/// closures can push events and schedule pumps without holding a mutable borrow
/// on the relay pool, and observe a later `set_wake`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn build_handlers(inbound: Rc<InboundQueue>, wake: WakeCell) -> BrowserKernelHandlers {
    let on_connected = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str, is_reconnect: bool| {
            inbound.push(InboundRelayEvent::Connected {
                role,
                url: url.to_string(),
                is_reconnect,
            });
            fire_wake(&wake);
        }) as Rc<dyn Fn(RelayRole, &str, bool)>
    };

    let on_text = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str, text: String| {
            inbound.push(InboundRelayEvent::Text {
                role,
                url: url.to_string(),
                text,
            });
            fire_wake(&wake);
        }) as Rc<dyn Fn(RelayRole, &str, String)>
    };

    let on_binary = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str, bytes: Vec<u8>| {
            inbound.push(InboundRelayEvent::Binary {
                role,
                url: url.to_string(),
                bytes,
            });
            fire_wake(&wake);
        }) as Rc<dyn Fn(RelayRole, &str, Vec<u8>)>
    };

    let on_close = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str, reason: Option<String>| {
            inbound.push(InboundRelayEvent::Close {
                role,
                url: url.to_string(),
                reason,
            });
            fire_wake(&wake);
        }) as Rc<dyn Fn(RelayRole, &str, Option<String>)>
    };

    let on_closed = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str| {
            inbound.push(InboundRelayEvent::Closed {
                role,
                url: url.to_string(),
            });
            fire_wake(&wake);
        }) as Rc<dyn Fn(RelayRole, &str)>
    };

    let on_failed = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str, error: String| {
            inbound.push(InboundRelayEvent::Failed {
                role,
                url: url.to_string(),
                error,
            });
            fire_wake(&wake);
        }) as Rc<dyn Fn(RelayRole, &str, String)>
    };

    BrowserKernelHandlers {
        on_connected,
        on_text,
        on_binary,
        on_close,
        on_closed,
        on_failed,
    }
}
