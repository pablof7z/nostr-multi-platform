//! Panic-isolated relay-event processing.
//!
//! [`process_relay_event`] wraps [`handle_relay_event`](super::dispatch::handle_relay_event)
//! in a `catch_unwind` so a panic while processing arbitrary network bytes
//! cannot kill the actor loop. Factored out of `run_actor_with_observers` so
//! the same panic-guarded body serves BOTH the bounded relay backlog batch and
//! the single recv'd event from one place (#1264), keeping `actor/mod.rs`
//! within its size budget.

use std::collections::{HashMap, HashSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, RelayRole};
use nmp_network::pool::{Pool, PoolEvent};

use super::dispatch::handle_relay_event;
use super::tick::emit_now;
use super::{CommandSender, RelayControl};

/// Process one relay [`PoolEvent`] under panic isolation.
///
/// Reliability north star: `handle_relay_event` processes arbitrary bytes from
/// the network — it is the highest-risk panic site in the actor. The
/// `catch_unwind` here means a panic in relay frame processing cannot kill the
/// kernel: the actor loop survives, logs the payload, surfaces an error toast,
/// and processes the next event fresh.
///
/// `AssertUnwindSafe` is required because the closure captures `&mut` kernel
/// state (`HashMap`/`Mutex` interiors are not `UnwindSafe`). This is sound
/// here: the actor is single-threaded, so there is no other thread that could
/// observe partially-mutated / poisoned state. Per D1 (best-effort rendering)
/// the kernel tolerates partial state — the invariant we protect is loop
/// survival, not per-event atomicity.
///
/// The command drain in the actor loop is deliberately NOT wrapped: commands
/// are internally generated, so a panic there is a genuine bug that must stay
/// visible.
///
/// V-38: the substrate-generic `RelayTextInterceptorSlot` is passed through so
/// an installed NIP-crate runtime (today `nmp-nip47`) can peek at text frames
/// the kernel would otherwise drop. `nmp-core` no longer names `wallet` / `NWC`
/// at the actor boundary (D0).
#[allow(clippy::too_many_arguments)]
pub(super) fn process_relay_event(
    event: PoolEvent,
    kernel: &mut Kernel,
    relay_text_interceptor: &crate::substrate::RelayTextInterceptorSlot,
    relay_connected_hook: &crate::substrate::RelayConnectedHookSlot,
    command_tx_self: &CommandSender,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    next_relay_generation: &mut u64,
    connected_relays: &mut HashSet<RelayRole>,
    connected_urls: &mut HashSet<CanonicalRelayUrl>,
    update_tx: &Sender<crate::update_envelope::UpdateFrameBytes>,
    last_emit: &mut Instant,
    startup_sent: &mut bool,
    running: bool,
) {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        handle_relay_event(
            event,
            kernel,
            relay_text_interceptor,
            relay_connected_hook,
            command_tx_self,
            relay_controls,
            slot_to_url,
            pool,
            next_relay_generation,
            connected_relays,
            connected_urls,
            update_tx,
            last_emit,
            startup_sent,
            running,
        );
    }));
    if let Err(panic_payload) = result {
        let msg = panic_payload
            .downcast_ref::<&str>()
            .map(std::string::ToString::to_string)
            .or_else(|| panic_payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        kernel.log(format!("actor: relay event handler panicked: {msg}"));
        kernel.set_last_error_toast(Some(
            "relay processing error — continuing".to_string(),
        ));
        // Surface the toast on this tick rather than waiting for the next
        // `flush_due` — mirrors the pending-sign error path.
        emit_now(kernel, running, update_tx, last_emit);
    }
}
