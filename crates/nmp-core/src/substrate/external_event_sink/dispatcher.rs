//! `ExternalEventSinkDispatcher` — bounded channel + single worker thread.
//!
//! The dispatcher is the single choke-point that receives a [`SignedEventFrame`]
//! from the kernel's ingest path and fans it out to every registered
//! [`ExternalEventSinkPolicy`] (in-process relay forwarding) on a dedicated
//! background thread, keeping the actor thread clear of I/O work.
//!
//! ## Construction vs. runtime binding
//!
//! The dispatcher is split so policy registration is deterministic from the
//! moment the app object exists:
//!
//! * [`ExternalEventSinkDispatcher::new`] builds the app-owned **registry**
//!   (policies, seq counter, diagnostics, the inbound channel). No `Pool` is
//!   required, so the FFI layer can publish it into its slot at construction
//!   and policies can register immediately.
//! * [`ExternalEventSinkDispatcher::bind_runtime`] is called once from the
//!   actor loop with the live `Pool`; it spawns the worker thread. Frames
//!   dispatched before binding buffer in the bounded channel.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use nmp_network::pool::Pool;

use super::diagnostics::ExternalEventSinkDiagnostics;
use super::worker::run_worker;
use super::{ExternalEventSinkPolicy, IngestOutcomeKind, SignedEventFrame};
use crate::actor::KindFilter;
use crate::store::InsertOutcome;

/// Bound on the dispatcher's inbound frame channel.  Same rationale as the old
/// `C_FANOUT_CHANNEL_BOUND`.
const DISPATCH_CHANNEL_BOUND: usize = 1024;

// ─── Registered policy (kind filter cached once) ──────────────────────────────

/// A registered relay-forwarding policy plus its `KindFilter`, precomputed once
/// at registration. The hot path must NEVER call `policy.kind_filter()` per
/// event — `IndexerRepublishPolicy::kind_filter` rebuilds a ~10k-entry
/// `BTreeSet` every call.
#[derive(Clone)]
pub(super) struct RegisteredPolicy {
    pub(super) policy: Arc<dyn ExternalEventSinkPolicy>,
    pub(super) kind_filter: Arc<KindFilter>,
}

// ─── Worker message ───────────────────────────────────────────────────────────

pub(super) struct DispatchWork {
    pub(super) frame: SignedEventFrame,
    /// Snapshot of policies (with cached filters) matching this frame's kind.
    pub(super) policies: Vec<RegisteredPolicy>,
}

/// Shared runtime handle, bound once via [`ExternalEventSinkDispatcher::bind_runtime`].
pub(super) struct Runtime {
    pub(super) pool: Pool,
}

// ─── ExternalEventSinkDispatcher ─────────────────────────────────────────────

/// Singleton dispatcher. App-owned registry created at construction; the worker
/// is spawned on [`bind_runtime`](Self::bind_runtime).
///
/// Clone is cheap (all fields are `Arc`).
#[derive(Clone)]
pub struct ExternalEventSinkDispatcher {
    tx: SyncSender<DispatchWork>,
    /// Inbound receiver, held until `bind_runtime` moves it onto the worker
    /// thread. Dropped with the dispatcher if it is never bound (no leak).
    rx: Arc<Mutex<Option<Receiver<DispatchWork>>>>,
    policies: Arc<Mutex<Vec<RegisteredPolicy>>>,
    diagnostics: Arc<ExternalEventSinkDiagnostics>,
    /// `true` once `bind_runtime` has spawned the worker. Guards double bind.
    bound: Arc<Mutex<bool>>,
}

impl Default for ExternalEventSinkDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalEventSinkDispatcher {
    /// Construct the app-owned registry. Does NOT spawn the worker — call
    /// [`bind_runtime`](Self::bind_runtime) once the actor's `Pool` is
    /// available.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = sync_channel::<DispatchWork>(DISPATCH_CHANNEL_BOUND);
        Self {
            tx,
            rx: Arc::new(Mutex::new(Some(rx))),
            policies: Arc::new(Mutex::new(Vec::new())),
            diagnostics: Arc::new(ExternalEventSinkDiagnostics::default()),
            bound: Arc::new(Mutex::new(false)),
        }
    }

    /// Replace the registered policy list. Each policy's `KindFilter` is
    /// precomputed ONCE here (hot path never recomputes — fixes the per-event
    /// ~10k-entry `BTreeSet` rebuild in `IndexerRepublishPolicy::kind_filter`).
    pub fn set_policies(&self, policies: Vec<Arc<dyn ExternalEventSinkPolicy>>) {
        let registered: Vec<RegisteredPolicy> = policies
            .into_iter()
            .map(|policy| {
                let kind_filter = Arc::new(policy.kind_filter());
                RegisteredPolicy { policy, kind_filter }
            })
            .collect();
        if let Ok(mut guard) = self.policies.lock() {
            *guard = registered;
        }
    }

    /// `true` when no registered policy matches `kind`.
    ///
    /// Hot-path gate used in the ingest chokepoint: a `true` return means the
    /// frame is never built (zero serialization). Uses the CACHED per-policy
    /// kind filters — never calls `policy.kind_filter()`.
    #[must_use]
    pub fn all_idle_for_kind(&self, kind: u32) -> bool {
        self.policies
            .lock()
            .map(|guard| !guard.iter().any(|p| p.kind_filter.matches(kind)))
            .unwrap_or(true)
    }

    /// Snapshot of diagnostic counters (panic isolation, channel drops).
    #[must_use]
    pub fn diagnostics(&self) -> super::diagnostics::DiagnosticsSnapshot {
        self.diagnostics.snapshot()
    }

    /// Gate: should the dispatcher emit a frame for this `outcome`?
    ///
    /// Preserves the DUPLICATE-inclusive invariant
    /// (Inserted | Replaced | Duplicate | Ephemeral).
    #[must_use]
    pub fn should_emit(outcome: &InsertOutcome) -> bool {
        IngestOutcomeKind::from_insert_outcome(outcome).is_some()
    }

    /// Non-blocking enqueue. The worker resolves destinations and delivers on a
    /// background thread.
    ///
    /// Returns `false` if no policy matches, the channel is full, or the
    /// policies mutex is poisoned (D6 — best-effort drop on the inbound channel).
    pub fn dispatch(&self, frame: SignedEventFrame) -> bool {
        let kind = frame.raw.kind;
        let matching: Vec<RegisteredPolicy> = match self.policies.lock() {
            Ok(guard) => guard
                .iter()
                .filter(|p| p.kind_filter.matches(kind))
                .cloned()
                .collect(),
            Err(_) => return false,
        };
        if matching.is_empty() {
            return false;
        }
        let work = DispatchWork { frame, policies: matching };
        match self.tx.try_send(work) {
            Ok(()) => true,
            Err(_) => {
                self.diagnostics.inc_channel_overflow_drops();
                false
            }
        }
    }

    /// Bind the live `Pool` and spawn the worker thread.
    ///
    /// Called once from the actor loop. Frames dispatched before this point sit
    /// in the bounded channel and are processed as soon as the worker starts.
    pub fn bind_runtime(&self, pool: Pool) {
        {
            let mut bound = match self.bound.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if *bound {
                return; // already bound — Reset rebinds policies, not the worker
            }
            *bound = true;
        }
        let rx = match self.rx.lock().ok().and_then(|mut g| g.take()) {
            Some(rx) => rx,
            None => return, // no receiver (already bound or constructed oddly)
        };
        let diagnostics = Arc::clone(&self.diagnostics);
        let runtime = Runtime { pool };
        let _worker = std::thread::Builder::new()
            .name("nmp-ext-sink-dispatch".into())
            .spawn(move || {
                run_worker(rx, Some(&runtime), &diagnostics);
            })
            .expect("spawn external-event-sink dispatch thread"); // doctrine-allow: D6 — spawned once at actor init; OS-level spawn failure at startup is unrecoverable
    }
}

// ─── Slot type ────────────────────────────────────────────────────────────────

/// Typed slot for the singleton dispatcher.
///
/// Populated at app construction, then bound to the runtime when the actor
/// spawns.
pub type ExternalEventSinkDispatcherSlot =
    Arc<Mutex<Option<ExternalEventSinkDispatcher>>>;

#[must_use]
pub fn new_external_event_sink_dispatcher_slot() -> ExternalEventSinkDispatcherSlot {
    Arc::new(Mutex::new(None))
}


// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "dispatcher/tests.rs"]
mod tests;
