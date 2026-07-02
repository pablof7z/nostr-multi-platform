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

use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

#[cfg(feature = "native")]
use nmp_network::pool::Pool;

use super::diagnostics::ExternalEventSinkDiagnostics;
#[cfg(feature = "native")]
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
    // Read only by the relay-forwarding worker (native). On wasm the worker is
    // not compiled, so this trait object is intentionally unread there; the
    // cached `kind_filter` is still used by `dispatch`/`all_idle_for_kind`.
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
    pub(super) policy: Arc<dyn ExternalEventSinkPolicy>,
    pub(super) kind_filter: Arc<KindFilter>,
}

// ─── Worker message ───────────────────────────────────────────────────────────

// Fields are read only by the worker thread, which is `native`-gated. On wasm
// the channel is still constructed (the dispatcher type is wasm-safe) but never
// drained, so the fields are intentionally unread there.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub(super) struct DispatchWork {
    pub(super) frame: SignedEventFrame,
    /// Snapshot of policies (with cached filters) matching this frame's kind.
    pub(super) policies: Vec<RegisteredPolicy>,
}

/// Shared runtime handle, bound once via [`ExternalEventSinkDispatcher::bind_runtime`].
///
/// Native-only: it owns the relay `Pool`, which is gated behind nmp-network's
/// `native` feature. On wasm there is no relay transport, so neither this handle
/// nor the worker that consumes it is compiled.
#[cfg(feature = "native")]
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
    /// Consumed only by `bind_runtime` (native); on wasm there is no worker to
    /// bind, so it is held but never taken.
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
    rx: Arc<Mutex<Option<Receiver<DispatchWork>>>>,
    policies: Arc<Mutex<Vec<RegisteredPolicy>>>,
    diagnostics: Arc<ExternalEventSinkDiagnostics>,
    /// `true` once `bind_runtime` has spawned the worker. Guards double bind.
    /// Read only by `bind_runtime` (native).
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
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
    ///
    /// # Kind-filter gate (#1607)
    ///
    /// Policies that return an empty (match-all) [`KindFilter`] are silently
    /// dropped and a `tracing::error!` is emitted. An empty filter was the
    /// raw-event-tap escape hatch that bypassed projection/action guarantees
    /// with no backpressure constraints. Requiring a non-empty explicit kind
    /// list enforces that each consumer declares its intent: this prevents
    /// unbounded cross-subscription fan-out when many consumers register. A
    /// policy that genuinely needs all kinds must enumerate them explicitly via
    /// `KindFilter::from_kinds([0..=u32::MAX])` — the code will not lie for it.
    pub fn set_policies(&self, policies: Vec<Arc<dyn ExternalEventSinkPolicy>>) {
        let registered: Vec<RegisteredPolicy> = policies
            .into_iter()
            .filter_map(|policy| {
                let kind_filter = policy.kind_filter();
                if kind_filter.is_all() {
                    tracing::error!(
                        "ExternalEventSinkPolicy rejected: kind_filter() returned an \
                         empty (match-all) KindFilter. All-kind raw-tap policies are \
                         banned (#1607 D5/D7). Declare explicit kinds via \
                         KindFilter::from_kinds([…])."
                    );
                    return None;
                }
                Some(RegisteredPolicy {
                    policy,
                    kind_filter: Arc::new(kind_filter),
                })
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

    /// Shared production counter handle for kernel `Metrics`.
    ///
    /// The dispatcher owns the counter because it is the writer on bounded
    /// channel overflow; the kernel reads the same atomic so drops are
    /// host-visible on every snapshot instead of test-only diagnostics.
    #[must_use]
    pub(crate) fn channel_overflow_drops_handle(&self) -> Arc<AtomicU64> {
        self.diagnostics.channel_overflow_drops_handle()
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
        let work = DispatchWork {
            frame,
            policies: matching,
        };
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
    ///
    /// Native-only: it takes a relay `Pool` and spawns the forwarding worker.
    /// On wasm there is no relay transport and no worker, so binding does not
    /// exist; the dispatcher still accepts `dispatch`/`all_idle_for_kind` calls
    /// (the channel buffers, nothing drains it) so the kernel ingest chokepoint
    /// compiles unchanged.
    #[cfg(feature = "native")]
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
pub type ExternalEventSinkDispatcherSlot = Arc<Mutex<Option<ExternalEventSinkDispatcher>>>;

#[must_use]
pub fn new_external_event_sink_dispatcher_slot() -> ExternalEventSinkDispatcherSlot {
    Arc::new(Mutex::new(None))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

// The dispatcher tests exercise `bind_runtime` + the relay-forwarding worker,
// both of which are `native`-only. Gate the suite to native so a
// `--no-default-features` (wasm-proxy) check does not try to compile it.
#[cfg(all(test, feature = "native"))]
#[path = "dispatcher/tests.rs"]
mod tests;
