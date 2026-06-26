//! Outbound fan-out and driver spawn/close for the browser relay pool (#2050).
//!
//! This module is wasm32-only — native builds have no WebSocket drivers.
//!
//! # Spawn-on-miss (kernel owns socket lifecycle)
//!
//! When the kernel emits an `OutboundMessage` targeting a URL not yet open in
//! the pool, `fan_out_outbound` spawns a driver for it rather than dropping
//! the frame — mirroring the native `relay_worker` pool's `ensure_relay_worker`
//! path. The kernel discovers relays (NIP-65 mailboxes, event-tag hints) and
//! targets them; the transport merely obeys.
//!
//! # Socket budget (#2070)
//!
//! At `MAX_CONCURRENT_SOCKETS` the spawn is refused and
//! `BrowserRuntimeEvent::RelayBudgetExceeded { url }` is emitted (D6-honest —
//! never a silent drop). The frame is still attempted on existing drivers if
//! the URL happens to be in the pool already.
//!
//! # UA note (#2050 O4)
//!
//! Browser `web_sys::WebSocket` cannot set HTTP request headers during the
//! WebSocket handshake (browser security constraint). The configured
//! `relay_user_agent` therefore reaches only the NIP-11 info-document GET
//! (which can set headers). The WS handshake carries the browser's default UA.

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use nmp_core::OutboundMessage;
#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};

#[cfg(target_arch = "wasm32")]
use super::budgets::MAX_CONCURRENT_SOCKETS;
#[cfg(target_arch = "wasm32")]
use crate::BrowserRuntimeEvent;

/// Fan an outbound batch to the matching drivers in `pool`, spawning a driver
/// on demand for any kernel-targeted URL not yet open.
///
/// Returns any budget-exceeded events produced during the fan.
#[cfg(target_arch = "wasm32")]
pub(crate) fn fan_out_outbound(
    pool: &mut Vec<Rc<BrowserRelayDriver>>,
    handlers: &BrowserKernelHandlers,
    outbound: &[OutboundMessage],
) -> Vec<BrowserRuntimeEvent> {
    let mut events = Vec::new();

    for message in outbound {
        let url = message.relay_url();

        let role = message.role();
        let known = pool.iter().any(|d| d.url() == url && d.role() == role);
        if !known {
            if pool.len() >= MAX_CONCURRENT_SOCKETS {
                // Budget exceeded — emit event, never silent.
                events.push(BrowserRuntimeEvent::RelayBudgetExceeded {
                    url: url.to_string(),
                });
                // Still attempt to send on any existing driver for this URL
                // (there are none since !known, so this is a no-op send).
                continue;
            }
            // Spawn-on-miss: the kernel targeted a URL not yet in the pool.
            match BrowserRelayDriver::new(url.to_string(), role, handlers.clone()) {
                Ok(driver) => pool.push(driver),
                // Bad URL (very rare after the first connect). Surface it — the
                // frame cannot be delivered, so do not silently drop it (D6).
                Err(error) => {
                    events.push(BrowserRuntimeEvent::RelaySpawnFailed {
                        url: url.to_string(),
                        reason: format!("{error:?}"),
                    });
                    continue;
                }
            }
        }

        for driver in pool.iter().filter(|d| d.url() == url && d.role() == role) {
            // send_text is synchronous and non-blocking (buffers if not OPEN).
            // A throw (e.g. socket in an illegal state) means the frame did not
            // leave the runtime — surface it rather than swallow it (D6).
            if let Err(error) = driver.send_text(message.text()) {
                events.push(BrowserRuntimeEvent::RelaySendFailed {
                    url: url.to_string(),
                    reason: format!("{error:?}"),
                });
            }
        }
    }

    events
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn_configured_relay(
    pool: &mut Vec<Rc<BrowserRelayDriver>>,
    handlers: &BrowserKernelHandlers,
    url: &str,
    role: &str,
) -> Vec<BrowserRuntimeEvent> {
    let mut events = Vec::new();
    let bootstrap = [(url.to_string(), role.to_string())];
    for plan in super::plan::plan_drivers(&bootstrap) {
        let known = pool
            .iter()
            .any(|d| d.url() == plan.url && d.role() == plan.role);
        if known {
            continue;
        }
        if pool.len() >= MAX_CONCURRENT_SOCKETS {
            events.push(BrowserRuntimeEvent::RelayBudgetExceeded { url: plan.url });
            continue;
        }
        match BrowserRelayDriver::new(plan.url.clone(), plan.role, handlers.clone()) {
            Ok(driver) => pool.push(driver),
            Err(error) => events.push(BrowserRuntimeEvent::RelaySpawnFailed {
                url: plan.url,
                reason: format!("{error:?}"),
            }),
        }
    }
    events
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn close_relay(pool: &mut Vec<Rc<BrowserRelayDriver>>, url: &str) {
    let mut kept = Vec::with_capacity(pool.len());
    for driver in pool.drain(..) {
        if driver.url() == url {
            driver.close();
        } else {
            kept.push(driver);
        }
    }
    *pool = kept;
}

/// Close every driver in the pool and clear it. Idempotent.
#[cfg(target_arch = "wasm32")]
pub(crate) fn close_drivers(pool: &mut Vec<Rc<BrowserRelayDriver>>) {
    for driver in pool.iter() {
        driver.close();
    }
    pool.clear();
}
