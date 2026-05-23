//! V-01 Stage 3 — relay-pool helpers for the wasm32 runtime.
//!
//! Owns the construction of the per-relay [`BrowserRelayDriver`] set and the
//! sink closure that fans the kernel's outbound back to the right driver.
//! Split out of `runtime.rs` so neither file exceeds the LOC ceiling and so
//! the wasm32-only logic does not pollute the protocol-conformance paths
//! that run on native CI.

use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{KernelReducer, OutboundMessage, RelayRole};

use crate::protocol::RelayBootstrapEntry;
use crate::relay_driver::{BrowserRelayDriver, RelaySink};
use crate::runtime::WasmRuntimeError;

/// Build the shared outbound sink — the closure each
/// [`BrowserRelayDriver`] hands to the kernel via
/// [`nmp_core::KernelReducer::handle_relay_frame`]. The sink routes each
/// outbound to the driver whose URL matches the kernel's resolved target.
/// A miss drops the frame: the relay is not in our bootstrap, so fabricating
/// a socket would violate the host's relay-policy declaration.
#[must_use]
pub(crate) fn build_sink(drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>) -> RelaySink {
    Rc::new(move |outbound: Vec<OutboundMessage>| {
        if outbound.is_empty() {
            return;
        }
        let drivers = drivers.borrow();
        for message in outbound {
            if let Some(driver) = drivers.iter().find(|d| d.url() == message.relay_url()) {
                let _ = driver.send_text(message.text());
            }
        }
    })
}

/// Instantiate one [`BrowserRelayDriver`] per bootstrap entry, wiring each
/// one's outbound through `sink`. Returns the populated driver list ready to
/// move into the runtime's relay slot.
///
/// Role assignment: every relay opens as [`RelayRole::Content`] — the
/// diagnostic-lane discriminator the kernel uses for `RelayHealth` rows.
/// Pure-indexer relays in the bootstrap could open a second driver on
/// [`RelayRole::Indexer`]; for Stage 3 the single-content model matches the
/// JS host's existing bootstrap shape (`role: "both"` is the default).
/// Multi-role parsing (split `"both,indexer"` into two drivers) is a
/// post-Stage-3 follow-up tracked in BACKLOG.
pub(crate) fn spawn_drivers(
    bootstrap: &[RelayBootstrapEntry],
    kernel: Rc<RefCell<KernelReducer>>,
    sink: RelaySink,
) -> Result<Vec<Rc<BrowserRelayDriver>>, WasmRuntimeError> {
    let mut drivers = Vec::with_capacity(bootstrap.len());
    for entry in bootstrap {
        let driver = BrowserRelayDriver::new(
            entry.url.clone(),
            RelayRole::Content,
            Rc::clone(&kernel),
            Rc::clone(&sink),
        )
        .map_err(|err| {
            WasmRuntimeError::InvalidConfig(format!(
                "failed to open WebSocket to {}: {err:?}",
                entry.url
            ))
        })?;
        drivers.push(driver);
    }
    Ok(drivers)
}

/// Close every driver in the pool and drop their parked closures. Idempotent.
pub(crate) fn close_drivers(drivers: &Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>) {
    for driver in drivers.borrow().iter() {
        driver.close();
    }
    drivers.borrow_mut().clear();
}
