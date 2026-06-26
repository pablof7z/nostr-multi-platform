//! Browser relay handler closure bag — enqueue-only callbacks (#2050).
//!
//! # D4 single-writer invariant
//!
//! `build_handlers` produces a [`BrowserKernelHandlers`] bag whose closures
//! ONLY enqueue [`InboundRelayEvent`]s into the shared `InboundQueue` and call
//! `wake()`. They NEVER borrow or mutate the `KernelReducer`. The reducer is
//! mutated exclusively inside `pump()` via [`super::inbound::drain_inbound`].
//!
//! This is the key difference from `nmp-wasm/src/relay_pool.rs` — where the
//! handler closures called `reducer.borrow_mut()` directly. That pattern breaks
//! D4 (sole-writer) on the browser runtime's owned-by-value architecture.
//!
//! # Wake contract
//!
//! `wake: Rc<dyn Fn()>` is the caller-provided "please schedule a pump" hook.
//! Default: no-op (tests call `pump()` directly). On wasm32 production the
//! host sets it to a function that schedules a 0ms timer which calls `pump()`
//! on the shared runtime handle.

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::BrowserKernelHandlers;
#[cfg(target_arch = "wasm32")]
use nmp_network::role::RelayRole;

#[cfg(target_arch = "wasm32")]
use super::inbound::{InboundQueue, InboundRelayEvent};

/// Build a [`BrowserKernelHandlers`] whose closures enqueue events (never
/// mutate the reducer). Called once from `spawn_bootstrap` on wasm32.
///
/// `inbound` and `wake` are `Rc`-shared so the closures can push events and
/// schedule pumps without holding a mutable borrow on the relay pool.
#[cfg(target_arch = "wasm32")]
pub(crate) fn build_handlers(
    inbound: Rc<InboundQueue>,
    wake: Rc<dyn Fn()>,
) -> BrowserKernelHandlers {
    let on_connected = {
        let inbound = Rc::clone(&inbound);
        let wake = Rc::clone(&wake);
        Rc::new(move |role: RelayRole, url: &str, is_reconnect: bool| {
            inbound.push(InboundRelayEvent::Connected {
                role,
                url: url.to_string(),
                is_reconnect,
            });
            (wake)();
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
            (wake)();
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
            (wake)();
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
            (wake)();
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
            (wake)();
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
            (wake)();
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
