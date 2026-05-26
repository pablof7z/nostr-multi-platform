mod actor;
mod app;
pub mod bunker_hook;
// V6 Stage 1 — Swift `Decodable` emitter input surface. Feature-gated:
// `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas`
// dumps one JSON schema per pilot projection type for `nmp-codegen gen swift`
// to consume. Off by default — shipped artifacts never link `schemars`.
#[cfg(feature = "codegen-schema")]
pub mod codegen_schema;
// Promoted from `mod capability_socket` so `nmp-ffi` can reach
// `dispatch_capability` / `new_capability_callback_slot` /
// `CapabilityCallbackSlot` through `nmp_core::__ffi_internal::*`. The
// socket is the substrate of the capability-callback seam; nothing in it
// names an app or protocol noun.
#[doc(hidden)]
pub mod capability_socket;
// V-33: shared display-string helpers (bech32 abbreviation, avatar tint
// djb2, relative-time bucketing) — canonical home for the cross-surface
// formatting primitives every NIP crate / kernel module / host-app
// projection previously duplicated.
pub mod display;
// Step 11 final — the C-ABI surface that used to live in `mod ffi;` now lives
// in the standalone `nmp-ffi` crate (`docs/architecture/crate-boundaries.md`
// §5 step 11-final). The substrate types the FFI marshals are re-exported
// through the public surface below + the `__ffi_internal` module so the
// extracted crate can name them through normal Rust paths.
//
// `mod ffi;` is gone — `pub use ffi::*` at the bottom of this file is gone
// too — consumers reach the symbols through `nmp_ffi::*` directly.
// ffi_guard: pure catch_unwind wrapper. Not I/O-bound; kept always-on
// because actor/commands/* use it on the native side (also actor is always
// compiled until Phase 1c decoupling). Promoted from `mod ffi_guard` to
// `pub mod ffi_guard` so the extracted `nmp-ffi` crate can reach
// `guard_ffi_callback` through a normal Rust path. The guard is substrate-
// grade (no app or protocol nouns); making it public is a layer-shape
// concession, not a noun leak.
#[doc(hidden)]
pub mod ffi_guard;
// Step 8 phase A — the keepalive FSM moved with the relay worker to
// `nmp-network::keepalive`. It's purely transport-internal; `nmp-core`
// no longer re-exports it.
mod kernel;
mod kernel_action;
mod kernel_reducer;
pub mod nip19;
pub mod nip21;
/// Subscription compiler.
///
/// Step 9 of the crate-boundary migration extracted the implementation into
/// the standalone [`nmp_planner`] crate. This module re-exports the public
/// surface so existing `use nmp_core::planner::*` import sites compile
/// unchanged.
pub mod planner {
    pub use nmp_planner::compiler::{
        CompileContext, EmptyMailboxCache, InMemoryMailboxCache, MailboxCache, MailboxSnapshot,
        SubscriptionCompiler,
    };
    pub use nmp_planner::interest::{
        HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
        NaddrCoord, PTagRouting, Pubkey, RelayHint, RelayUrl,
    };
    pub use nmp_planner::lattice::{merge, MergeOutcome};
    pub use nmp_planner::plan::{
        canonical_filter_hash, CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape,
        UserConfiguredCategory,
    };
    pub use nmp_planner::selection::apply_selection;

    // A small number of in-tree call sites reach into the submodule
    // namespaces directly (`nmp_core::planner::compiler::*`,
    // `nmp_core::planner::interest::*`, etc.). Re-expose those module
    // paths so the migration is a pure compile-only no-op.
    pub use nmp_planner::{compiler, interest, lattice, plan, selection};
}
pub mod publish;
mod relay;
// Step 8 phase A — `relay_protocol` and `relay_worker` moved to
// `nmp-network`. They are re-imported here only through the (gated) actor
// runtime path; the public re-exports below preserve the prior
// `nmp_core::relay_protocol::*` surface (no-op for downstream crates that
// imported through the old path — they should migrate to `nmp_network`).
//
// V-38: the `wallet` module is gone — the NIP-47 wallet runtime + the
// `nmp.wallet.pay_invoice` `ActionModule` moved to `crates/nmp-nip47`. The
// kernel no longer depends on `nmp-nwc`, and `nmp-core` no longer has a
// `wallet` Cargo feature. See `docs/architecture/crate-boundaries.md`
// §5 step 7 for the migration brief.
pub mod remote_signer;
/// Deterministic 64-bit hash helper — the seed for every plan-id,
/// interest-id, and content-addressed projection key.
///
/// Moved into [`nmp_planner::stable_hash`] in step 9 of the crate-boundary
/// migration (the planner is the only foundation crate that *cannot* depend
/// on `nmp-core`). This module is a thin re-export so `use
/// nmp_core::stable_hash::stable_hash64` import sites compile unchanged.
pub mod stable_hash {
    pub use nmp_planner::stable_hash::*;
}
/// Event-storage abstraction.
///
/// Step 9 of the crate-boundary migration extracted the implementation into
/// the standalone [`nmp_store`] crate. This module is a thin re-export so
/// existing `use nmp_core::store::*` import sites compile unchanged. The
/// substrate-side `DomainMigration` / `MigrationTx` types moved with the
/// store; they are re-exported through both `nmp_core::store::*` (via
/// `nmp_store`'s root) and `nmp_core::substrate::*` (legacy path).
pub mod store {
    pub use nmp_store::*;
}
// Step 11 final — shared substrate slot aliases the FFI shell (`nmp-ffi`)
// and the actor runtime (`crate::actor`) both reach into. Used to live in
// `crate::ffi::mod.rs` (private); promoted here so the actor module (a
// crate-private module) can still name them after the FFI extraction.
// `pub` because nmp-ffi reaches them through `nmp_core::slots::*`.
pub mod slots;
pub mod subs;
pub mod substrate;
pub mod tags;
mod update_envelope;
pub mod util;

pub use app::{
    resolve_open_uri, KernelAction, KernelUpdate, KernelViewSpec, OpenUriError, OpenUriRouting,
    VIEW_ADDRESSABLE, VIEW_PROFILE, VIEW_THREAD,
};
pub use bunker_hook::{register_bunker_hook, BunkerHookFn, BunkerHookRequest};
// Step 11 final — `NmpApp` opaque handle + the `nmp_app_*` symbol family
// moved to the standalone `nmp-ffi` crate (`nmp_ffi::NmpApp`). `nmp-core`
// no longer exposes `ffi::*` at all.
pub use kernel::{read_eligible_relay_urls, Kernel, RelayEditRow, RelayEditRowList, RelayEditRowsSlot};
// V-38: NIP crates (`nmp-nip47`) registering per-lane NIP-42 signers need the
// `AuthSignerFn` alias for their `Kernel::set_relay_auth_signer(...)` call.
// Substrate-grade (D0): no protocol nouns — generic Schnorr signer callback.
pub use kernel::AuthSignerFn;
// V-51 phase 4 (validation harness) — the projection's three public types
// reachable from `nmp-testing` and the chirp-repl. `RoutingTraceProjection`
// is the bounded ring-buffer the kernel hands to production composition
// (via `routing_trace()` → `set_routing_substrate` factory →
// `GenericOutboxRouter::with_trace_observer`); `PublishTraceEntry` /
// `SubscriptionTraceEntry` are the entry shapes the `snapshot_*` accessors
// return. See `kernel::routing_trace` module doc.
pub use kernel::routing_trace::{
    PublishTraceEntry, RoutingTraceProjection, SubscriptionTraceEntry,
    DEFAULT_ROUTING_TRACE_CAPACITY,
};
// V-51 phase 2 — JSON DTO renderer. Consumer-side helper: turns a
// projection snapshot into a Swift/wasm-friendly JSON value the FFI symbol
// (`nmp_app_recent_routing_decisions`) and the wasm runtime
// (`recent_routing_decisions`) both ship to their respective hosts.
pub use kernel::routing_trace_dto::{projection_to_json, ROUTING_TRACE_SCHEMA_VERSION};
// V-01 Stage 3 — the wire-transport-agnostic frame enum the kernel ingests.
// Promoted to the public surface so the wasm32 `BrowserRelayDriver` (lives
// in `nmp-network::browser_driver` as of step 8 phase C) can be bridged from
// `web_sys::MessageEvent` / `CloseEvent` through the
// `nmp-wasm::relay_pool::build_handlers` callback bag.
// Substrate-grade (D0): no app/protocol nouns.
pub use kernel::RelayFrame;
pub use kernel_reducer::KernelReducer;
pub use relay::canonical_relay_url;
// V-01 Stage 3 — the per-frame outbound type (`role`, `relay_url`, `text`) the
// kernel produces and any transport (native `relay_worker`, wasm
// `BrowserRelayDriver` — both in `nmp-network` as of step 8 phase C) consumes.
// Fields stay `pub(crate)` so the kernel remains the single writer; external
// callers read via accessors.
pub use relay::{OutboundMessage, RelayRole};
pub use remote_signer::RemoteSignerHandle;
pub use update_envelope::{
    panic_message, wrap_panic, wrap_snapshot, PanicFrame, UpdateEnvelope, WireEnvelope,
    SNAPSHOT_SCHEMA_VERSION,
};

// Stage 4 of NIP-46 wiring: app/FFI composition translates app-neutral
// broker events into actor commands. The `actor` module is crate-private so
// this re-export is the only Rust-side path for adapters that need to push
// `AddRemoteSigner` / `BunkerHandshakeProgress` back to the actor. The enum
// variants themselves are already `pub`.
pub use actor::ActorCommand;
pub use actor::NOSTRCONNECT_DEFAULT_RELAY_URL;

// Step 11 final — every `nmp_app_*` `extern "C"` symbol that used to be
// re-exported from `ffi::` now lives in the standalone `nmp-ffi` crate.
// Consumers that previously named the symbols through `nmp_core::` should
// migrate to `nmp_ffi::*`. The `NmpApp` opaque handle moved with the
// symbols. See `docs/architecture/crate-boundaries.md` §5 step 11-final.
//
// V-38: the `nmp_app_wallet_*` FFI symbols moved to `nmp-ffi::wallet` as
// thin shims routing through `nmp.wallet.{connect,disconnect,pay_invoice}`
// (dispatch_action). The actual wallet runtime lives in `crates/nmp-nip47`.

// T118 / G3 — lifecycle observer wire-shape exposed for integration tests
// (the `LifecycleObserverFn` is a plain `extern "C" fn` shape) and the
// phase-code constants the observer must interpret. The actor module is
// crate-private, so this is the only Rust-side surface for the wire shape.
#[cfg(any(test, feature = "test-support"))]
pub use actor::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};

// T146 — kernel event observer surface exposed to per-app Rust crates
// (`nmp-app-chirp`, future app-specific crates, ...). Apps register typed
// `Arc<dyn KernelEventObserver>`s via [`NmpApp::register_event_observer`].
// The FFI shape (`KernelEventObserverFn` etc.) is the C-ABI channel
// Swift / Kotlin bridges use directly through
// `nmp_app_register_event_observer`.
pub use actor::{
    KernelEventObserver, KernelEventObserverFn, KernelEventObserverId,
};

// Raw signed-event tap surface exposed to per-app Rust crates. Apps
// register typed `Arc<dyn RawEventObserver>`s (with a `KindFilter`) via
// [`NmpApp::register_raw_event_observer`] to receive the verbatim flat
// NIP-01 signed event (`sig` included). The FFI shape
// (`RawEventObserverFn` etc.) is the C-ABI channel Swift / Kotlin bridges
// use directly through `nmp_app_register_raw_event_observer`. Generic
// capability (D0) — no protocol nouns.
pub use actor::{
    KindFilter, RawEventObserver, RawEventObserverFn, RawEventObserverId,
};

// ── Step 11 final — `nmp-ffi` re-export surface ────────────────────────────
//
// The standalone `nmp-ffi` crate (extracted from `nmp-core::ffi`) reaches
// these symbols through `nmp_core::__ffi_internal::*`. The module is
// `#[doc(hidden)]` — no app crate or library consumer should import it; the
// only legitimate consumer is `nmp-ffi`. Adding a new item here is a layer-
// shape concession (the substrate item was previously crate-private), not a
// public API addition.
//
// Why the special module rather than promoting each item to `pub` at the
// crate root: keeps the public surface area visibly identical to before the
// extraction, and gives `cargo doc` users a single place to spot "this is
// an extraction seam, not a real API".
// Gated on `feature = "native"` because the re-exports below pull in
// `run_actor_with_observers` and friends from `crate::actor`, which are
// themselves `#[cfg(feature = "native")]`. The wasm32 build
// (`--no-default-features`) has no actor thread and no FFI shell consuming
// this module.
#[cfg(feature = "native")]
#[doc(hidden)]
pub mod __ffi_internal {
    pub use crate::actor::{
        has_role, new_bunker_handshake_slot, new_event_observer_slot,
        new_lifecycle_observer_slot, new_raw_event_observer_slot, nostrconnect_relay_url,
        register_c_observer, register_c_raw_observer, register_rust_observer,
        register_rust_raw_observer, run_actor_with_observers, unregister_observer,
        unregister_raw_observer, KernelEventObserverRegistration, KernelEventObserverSlot,
        LifecycleObserverFn, LifecycleObserverRegistration, LifecycleObserverSlot,
        RawEventObserverRegistration, RawEventObserverSlot, LIFECYCLE_PHASE_BACKGROUND,
        LIFECYCLE_PHASE_FOREGROUND,
    };
    // V-38: `WalletStatusSlot` / `new_wallet_status_slot` moved to
    // `nmp-nip47`. The host (per-app crate) constructs the slot itself and
    // registers it via `nmp_app_register_snapshot_projection("wallet", …)`.
    pub use crate::app::KernelAction;
    pub use crate::capability_socket::{
        capability_error_envelope, dispatch_capability, new_capability_callback_slot,
        CapabilityCallback, CapabilityCallbackRegistration, CapabilityCallbackSlot,
    };
    pub use crate::kernel::{
        default_registry, is_hex_id, is_hex_pubkey, new_relay_edit_rows_slot,
        new_snapshot_projection_slot, routing_trace, ActionRegistry, LifecyclePhase,
        SnapshotProjectionSlot,
    };
    pub use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
}

/// Test-support facade: gives live-bench binaries access to the actor
/// internals without exposing domain nouns in the stable `nmp-core` API.
///
/// Enable with `features = ["test-support"]` in `Cargo.toml`.  This gate is
/// intentionally `any(test, feature = "test-support")` so `cargo test` always
/// has access without an explicit feature flag.
///
/// V-01 Phase 1c: the facade re-exports `run_actor` and the conformance
/// harness — both live on the native runtime — so the whole module is gated
/// behind `native` as well. Under `--no-default-features` there is no actor
/// thread to spawn and no harness handlers to drive.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
pub mod testing {
    pub use crate::actor::{run_actor, ActorCommand};
    pub use crate::store::{RawEvent, VerifiedEvent};

    /// NIP golden-tag conformance harness — drives the (crate-private) command
    /// handlers against a real `Kernel` + `IdentityRuntime` and returns the
    /// emitted `EVENT` JSON so an integration test can assert per-kind tag
    /// structure. See `tests/nip_tag_conformance.rs`.
    pub use crate::actor::ConformanceHarness;

    use std::sync::mpsc;
    use std::thread;

    /// Spawn the kernel actor on a dedicated thread.
    ///
    /// Returns a command sender and an update receiver.  The caller drives the
    /// actor by sending [`ActorCommand`] values and reads JSON-encoded kernel
    /// snapshots from the update channel.  Dropping the sender or sending
    /// [`ActorCommand::Shutdown`] stops the actor thread.
    pub fn spawn_actor() -> (mpsc::Sender<ActorCommand>, mpsc::Receiver<String>) {
        let (command_tx, command_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        // Hand the actor a clone of the command sender so dispatch arms
        // that spawn workers (currently the LNURL-pay round-trip) can
        // send follow-up `ActorCommand`s back into the loop. The outer
        // returned `command_tx` is the host's primary handle; this clone
        // serves only the actor's internal self-feedback path.
        let actor_command_tx_self = command_tx.clone();
        thread::spawn(move || run_actor(command_rx, actor_command_tx_self, update_tx));
        (command_tx, update_rx)
    }

    /// Build `count` real Schnorr-signed kind-1 events and enqueue them for
    /// ingest via `ActorCommand::IngestPreVerifiedEvents`.
    ///
    /// Uses a single `nostr::Keys::generate()` fixture key so all events share
    /// one pubkey — sufficient for harness pressure tests (S4/S5) where the
    /// goal is emit throughput, not per-author diversity.
    ///
    /// Schnorr sign cost: ~30–50 µs/event.  For S4 (500 events) and S5 (200
    /// events) this is 10–25 ms total — acceptable.  For S3 (100k events) use
    /// `nmp_app_inject_pre_verified_events` which uses `from_raw_unchecked`.
    #[allow(clippy::result_large_err)] // ActorCommand is large by design; boxing here would cascade through test callers
    pub fn inject_signed_events(
        tx: &mpsc::Sender<ActorCommand>,
        base_ts: u64,
        count: u32,
    ) -> Result<(), mpsc::SendError<ActorCommand>> {
        use nostr::{EventBuilder, Keys, Timestamp};

        // Single fixture key: generate once, sign all events with it.
        // The key is not reused across harness runs (Keys::generate() uses OsRng).
        let keys = Keys::generate();
        let events: Vec<VerifiedEvent> = (0..count as u64)
            .filter_map(|i| {
                let content = format!("signed harness event {i}");
                let ts = Timestamp::from(base_ts.saturating_add(i));
                let nostr_event = EventBuilder::text_note(content)
                    .custom_created_at(ts)
                    .sign_with_keys(&keys)
                    .ok()?;
                // Convert nostr::Event to our RawEvent, then verify the full path.
                // try_from_raw re-verifies the signature — confirms the signed event
                // is well-formed before the kernel ingests it.
                let raw = RawEvent {
                    id: nostr_event.id.to_hex(),
                    pubkey: nostr_event.pubkey.to_hex(),
                    created_at: nostr_event.created_at.as_secs(),
                    kind: nostr_event.kind.as_u16() as u32,
                    tags: nostr_event
                        .tags
                        .iter()
                        .map(|t| t.as_slice().to_vec())
                        .collect(),
                    content: nostr_event.content.clone(),
                    sig: nostr_event.sig.to_string(),
                };
                VerifiedEvent::try_from_raw(raw).ok()
            })
            .collect();
        tx.send(ActorCommand::IngestPreVerifiedEvents(events))
    }
}
