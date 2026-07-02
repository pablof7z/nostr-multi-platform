pub mod actor;
mod app;
pub mod bunker_hook;
pub mod external_signer_hook;

// SHARED FlatBuffers `ProfileCard` row type, mounted at the crate root so the
// profile-cluster generated bindings can resolve it.
//
// `profile.fbs` `include`s `profile_card.fbs` and references its `ProfileCard`
// table. `flatc` (no
// `--gen-all`) emits `ProfileCard` ONLY into `profile_card_generated.rs` and
// drops a crate-root `use crate::profile_card_generated::*;` into each per-key
// `*_generated.rs`. That glob only sees items at the *top* of
// `profile_card_generated`, but the generated leaf types are nested under
// `nmp::kernel`. So this wrapper hides the generated `pub mod nmp` inside
// `inner` and flat-re-exports the `nmp::kernel` leaf types at the module root —
// the per-key generated files' glob then resolves `ProfileCard` /
// `ProfileCardArgs` by short name. Mirrors the `op_feed.fbs` →
// `timeline_snapshot.fbs` include precedent in `crates/nmp-nip01/src/lib.rs`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
pub(crate) mod profile_card_generated {
    mod inner {
        #![allow(
            clippy::all,
            dead_code,
            deprecated,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            unsafe_code,
            unused_imports
        )]
        include!("kernel/typed_projections/generated/profile_card_generated.rs");
    }
    pub use inner::nmp::kernel::*;
}

// V-112 (ADR-0076): the shared FlatBuffers `TimelineItem` row cluster
// (`timeline_item.fbs`, `timeline_item_generated.rs`, and the
// `timeline_item_generated` wrapper mod that mirrored
// `profile_card_generated` above) was deleted — its only consumers were the
// retired `author_view.fbs` / `thread_view.fbs` typed projections.

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
// The C-ABI surface that used to live in `mod ffi;` now lives in the
// standalone `nmp-ffi` crate (`docs/architecture/crate-boundaries.md` §10a).
// `mod ffi;` / `pub use ffi::*` are gone; consumers reach
// the symbols through `nmp_ffi::*` directly. The substrate types the FFI
// marshals are re-exported through the public surface below + `__ffi_internal`.
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
/// V-57 P2 — canonical Nostr kind constants for the entire workspace.
/// Single source of truth for the integer kind numbers used on the wire.
/// See [`kinds`] for the migration rationale.
pub mod kinds;
// NIP-19 bech32 entity codec AND the NIP-21 `nostr:` URI surface moved out of
// the kernel substrate into the dependency-light Layer-0 `nmp-nostr-id` crate
// (issue #2515) — the substrate must not own protocol-specific parsers/nouns
// (crate-boundaries.md §3), and the identifier vocabulary belongs at L0 (like
// `nmp-relay-url`) which the kernel may depend on downward. Callers now depend
// on `nmp_nostr_id::*` directly; there is no re-export shim here.
// Subscription compiler — internal path for nmp-core consumers. External
// callers must depend on `nmp-planner` directly (`nmp_planner::*`); the
// `nmp_core::planner` re-export path is deleted (#1608, D0/D3: facades leak
// planner internals into the app-facing surface). Only items nmp-core
// internals actively use are re-exported here.
pub(crate) mod planner {
    pub use nmp_planner::compiler::{MailboxCache, MailboxSnapshot, SubscriptionCompiler};
    pub use nmp_planner::interest::{
        bounded_search_query, HintSource, InterestId, InterestLifecycle, InterestScope,
        InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayHint, RelayUrl,
    };
    // Test-only: `InMemoryMailboxCache` and `PTagRouting` are only referenced
    // in `#[cfg(test)]` modules inside nmp-core; gate them so the production
    // build stays warning-free under `-D warnings`.
    #[cfg(test)]
    pub use nmp_planner::compiler::InMemoryMailboxCache;
    #[cfg(test)]
    pub use nmp_planner::interest::PTagRouting;
    pub use nmp_planner::plan::{
        canonical_filter_hash, CompiledPlan, PlannerError, RelayAttribution, SubShape,
    };
    // W4 — warm-relay score lookup seam + lookup-aware selection.
    pub use nmp_planner::selection::apply_selection_with_lookup;
    // Internal call sites that reach into `interest::EventId` and similar
    // sub-module items that aren't re-exported at the planner crate root.
    pub use nmp_planner::interest;
}
/// V-52 — single-relay browsing via the `nmp.browse_relay` action namespace.
///
/// Manual relay-bypass path (D3 explicit opt-out): builds a
/// [`nmp_planner::interest::LogicalInterest`] with `relay_pin = Some(url)` and
/// dispatches `InterestsCommand::EnsureInterest`. This is not a standard app path —
/// the host must explicitly register [`browse::BrowseRelayModule`] and the
/// caller must supply a validated relay URL. NIP-65 fan-out is suppressed.
pub mod browse;
pub mod publish;
mod relay;
mod transport;
// ADR-0071 / S2 (#1750) — the open write-command byte transport: decode +
// fail-closed gates + the opaque-payload carry. Public so the wasm runtime and
// the native FFI byte doorway both reach the one inbound decode path.
pub use transport::dispatch_envelope;
// V-38: the `wallet` module is gone — the NIP-47 wallet runtime + the
// `nmp.wallet.pay_invoice` `ActionModule` moved to `crates/nmp-nip47`. The
// kernel no longer depends on `nmp-nwc`, and `nmp-core` no longer has a
// `wallet` Cargo feature. See `docs/architecture/crate-boundaries.md`
// §8 for the crate responsibility statement (`nmp-nip47` owns the NIP-47
// NWC wallet runtime and the `PaymentPort` implementation).
// Deterministic 64-bit hash helper — internal path for nmp-core.
// External callers must depend on `nmp-planner` directly and use
// `nmp_planner::stable_hash::stable_hash64` (#1608, compat facade deleted).
pub(crate) mod stable_hash {
    pub use nmp_planner::stable_hash::*;
}
// Event-storage abstraction — internal path for nmp-core.
// External callers must depend on `nmp-store` directly and use
// `nmp_store::*` (#1608, compat facade deleted).
pub(crate) mod store {
    pub use nmp_store::*;
}
pub mod projection_emission; // ADR-0070 R6-S2: byte-equality typed-projection omit helper.
pub mod refs; // ADR-0070 Lane A (#1671) — row-grain delta carrier for keyed reference projections.
              // Step 11 final — shared substrate slot aliases the FFI shell (`nmp-ffi`) and the
              // actor runtime (`crate::actor`) both reach into. Used to live in `crate::ffi::mod.rs`
              // (private); promoted here so the crate-private actor module can still name them after
              // the FFI extraction. `pub` because nmp-ffi reaches them through `nmp_core::slots::*`.
pub mod slots;
pub mod subs;
pub mod substrate;
pub mod tags;
// Target-conditional time shim: `web_time` on wasm32, `std::time` on native.
// Wasm-reachable kernel code imports `Instant`, `SystemTime`, `UNIX_EPOCH`
// from here so `performance.now()` / `Date.now()` back them on wasm32
// (where the `std` implementations abort). See `time.rs` for rationale.
pub mod time;
pub mod ui_token;
mod update_envelope;
pub mod util;

pub use app::{
    resolve_open_uri, KernelAction, KernelUpdate, KernelViewSpec, OpenUriError, OpenUriRouting,
    VIEW_ADDRESSABLE, VIEW_PROFILE, VIEW_THREAD,
};
pub use bunker_hook::{
    install_bunker_hook, new_bunker_hook_slot, BunkerHookFn, BunkerHookRequest, BunkerHookSlot,
};
pub use external_signer_hook::{
    install_external_signer_hook, new_external_signer_hook_slot, ExternalSignerHookFn,
    ExternalSignerHookRequest, ExternalSignerHookSlot,
};
// Step 11 final — `NmpApp` opaque handle + the `nmp_app_*` symbol family
// moved to the standalone `nmp-ffi` crate (`nmp_ffi::NmpApp`). `nmp-core`
// no longer exposes `ffi::*` at all.
pub use kernel::{
    read_eligible_relay_urls, AppRelay, AppRelayList, AppRelaySlot, DependentInterestChild, Kernel,
    ProfileLiveness, KERNEL_BUILTIN_PROJECTION_KEYS,
};
// ADR-0070 Lane D — closed typed `resolve_ref`/`release_ref` surface at the crate root.
pub use kernel::pull::{pull_page_over, PullError, PullLimits, PullScope}; // ADR-0072
pub use kernel::pull_cursor::{InvalidCursorSpec, PullConsumerId, PullCursorHandle};
pub use kernel::pull_cursor::{PullCursorId, PullCursorMode, PullCursorRegistry, PullCursorSpec};
pub use kernel::pull_wake::{decode_pull_wake_batch, PullWakeRow, PULL_WAKE_KEY};
pub use kernel::{record_emitted_feed_authors, EmittedFeedAuthorsSlot}; // ADR-0070 D7 (#1671)
pub use kernel::{
    EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape,
};
// ADR-0069 — the composition ledger (explain-the-composition surface) and its
// record types. Re-exported at the crate root so `nmp-ffi` (the C-ABI host) and
// downstream composition crates can name them without reaching into `kernel`.
pub use kernel::{default_registry, ActionRegistry};
pub use kernel::{
    CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION,
}; // ADR-0071/S3 (#1751/#1008): crate-root registry for the no-`native` WASM path.
   // Injectable kernel wall-clock trait. Re-exported (always) so the `pub`
   // `slots::KernelClockSlot` alias (`Arc<Mutex<Option<Arc<dyn Clock>>>>`) is
   // nameable across crates. Production installs nothing (the kernel keeps its
   // `SystemClock`); only the test-support `MonotonicSecondClock` is constructible
   // downstream.
pub use kernel::Clock;
// Test-support: advanceable kernel clock external e2e tests install through the
// FFI `NmpApp::set_kernel_clock_for_test` seam to stamp strictly-increasing
// `created_at` deterministically (no wall-clock sleep — D8).
#[cfg(any(test, feature = "test-support"))]
pub use kernel::MonotonicSecondClock;
// W2 — relay-author-score types. Re-exported so nmp-testing integration tests
// and downstream crates (W4, W5) can access `ClaimOutcome`, `RelayAuthorScore`,
// and `RelayAuthorScoreMap` without reaching into the private `kernel` module.
pub mod relay_score {
    pub use super::kernel::relay_score::{
        ClaimOutcome, RelayAuthorScore, RelayAuthorScoreMap, DECAY_HALFLIFE_DAYS, WARM_THRESHOLD,
    };
}
// V-38: NIP crates (`nmp-nip47`) registering per-lane NIP-42 signers need the
// `AuthSignerFn` alias for their `Kernel::set_relay_auth_signer(...)` call.
// Substrate-grade (D0): no protocol nouns — generic Schnorr signer callback.
pub use kernel::{wallet_access::KernelWalletAccess, AuthSignerFn}; // KernelWalletAccess: ADR-0072 §D5 wallet/zap adapter
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
// in `nmp-network::browser_driver`) can be bridged from
// `web_sys::MessageEvent` / `CloseEvent` through the
// `nmp-browser-runtime` relay handler callback bag.
// Substrate-grade (D0): no app/protocol nouns.
pub use kernel::{
    kernel_ports::{
        IdentityPort, InterestPort, KernelPorts, ProtocolDispatchPort, PublishPort, PullCursorPort,
        ReferencePort, RelayLifecyclePort, UiPort,
    },
    RelayFrame,
};
pub use kernel_reducer::CommandApplyOutcome; // #2045 PR-A narrow headless interpreter outcome
pub use kernel_reducer::{
    KernelReducer, SignRoundTripCompletion, SignRoundTripOutcome, SignRoundTripRequest,
}; // #1753 S6 wasm signing DTOs
pub use relay::canonical_relay_url;
// V-01 Stage 3 — the per-frame outbound type (`role`, `relay_url`, `text`) the
// kernel produces and any transport (native `relay_worker`, wasm
// `BrowserRelayDriver` — both in `nmp-network`) consumes.
// Fields stay `pub(crate)` so the kernel remains the single writer; external
// callers read via accessors.
pub use relay::OutboundMessage;
pub use update_envelope::{
    decode_snapshot_envelope, decode_snapshot_typed_projections, decode_update_frame, encode_panic,
    encode_snapshot_frame, panic_message, PanicFrame, ProjectionMergeCache, RelayStatusEntry,
    SnapshotEnvelope, TypedProjectionData, UpdateEnvelope, UpdateFrameBytes,
    UpdateFrameDecodeError, WireProjectionState, WireSubscriptionEntry, SNAPSHOT_SCHEMA_VERSION,
};

/// Public decode surface for the kernel-owned (Tier-2) typed-projection
/// sidecar (ADR-0072).
///
/// Pair these per-key decoders with [`decode_snapshot_typed_projections`],
/// which returns the snapshot's [`TypedProjectionData`] entries: look an entry
/// up by `key` (e.g. [`typed_projections::PUBLISH_QUEUE_SCHEMA_ID`]) and pass
/// its `payload` to the matching `decode_*` function to get a typed Rust
/// struct. The Tier-3 envelope fields (rev/running/metrics/relay status)
/// travel separately — read them via [`decode_snapshot_envelope`].
///
/// The module is the documented extension point for the Tier-2 cluster (one
/// `pub use` line per key).
pub mod typed_projections {
    pub use crate::kernel::public_typed_projections::*;
}

// #2074 — signer-state typed-projection codec promoted to the crate root.
//
// `SignerStateModel`, `encode_signer_state`, `decode_signer_state`, and the
// three schema constants are re-exported here unconditionally so browser-runtime
// and external consumers can encode/decode the Tier-1 signer-state typed sidecar
// using pure FlatBuffers without the `__ffi_internal` seam or native-only deps.
//
// On native builds: re-exported from the actor-owned codec (the canonical source).
// On non-native (wasm32/browser) builds: re-exported from the always-compiled
// `signer_state_codec` shim (pure FlatBuffers, no native runtime deps).
#[cfg(feature = "native")]
pub use actor::typed_projections::{
    decode_signer_state, encode_signer_state, SignerStateModel, SIGNER_STATE_FILE_IDENTIFIER,
    SIGNER_STATE_SCHEMA_ID, SIGNER_STATE_SCHEMA_VERSION,
};
#[cfg(not(feature = "native"))]
pub(crate) mod signer_state_codec;
#[cfg(not(feature = "native"))]
pub use signer_state_codec::{
    decode_signer_state, encode_signer_state, SignerStateModel, SIGNER_STATE_FILE_IDENTIFIER,
    SIGNER_STATE_SCHEMA_ID, SIGNER_STATE_SCHEMA_VERSION,
};

// Stage 4 of NIP-46 wiring: app/FFI composition translates app-neutral
// broker events into actor commands. The `actor` module is crate-private so
// this re-export is the only Rust-side path for adapters that need to push
// `AddSigner` / `BunkerHandshakeProgress` back to the actor. The enum
// variants themselves are already `pub`.
//
// `SignerSource` is re-exported alongside so the FFI sign-in shims and the
// broker adapter can name `SignerSource::{LocalNsec, BunkerUri, RemoteHandle}`
// when constructing an `AddSigner` command.
//
// `SignContinuation` is the boxed sign-outcome callback carried by the
// `ActorCommand::SignEventForAccount` port (ADR-0071 Decision 2). Re-exported
// so protocol crates that consume the port through
// `ProtocolCommandContext::sign_event_for_account` (e.g. `nmp-nip57`'s zap
// command) can name it — chiefly in tests that drive the continuation directly.
pub use actor::{CipherContinuation, SignContinuation, SignerSource};
// ADR-0072 §D3a — the unified actor-inbox transport seam. `CommandSender` is
// the single command-send handle passed to relay-connected hooks, DM inbox
// chains, and similar substrate seams that post commands from worker threads.
// `CommandSendError` is `send`'s disconnect error (mpsc-`SendError` parity);
// full-inbox shed-load is counted and reported as `CommandSendStatus`.
// `ActorMail` is the raw inbox discriminant — test-support only (#1608: not
// part of the stable public API; external code that needs a test channel
// should use `nmp_core::testing::spawn_actor()` instead).
#[cfg(any(test, feature = "test-support"))]
pub use actor::ActorMail;
pub use actor::{CommandSendError, CommandSendStatus, CommandSender};

// Every `nmp_app_*` `extern "C"` symbol that used to be re-exported from
// `ffi::` now lives in the standalone `nmp-ffi` crate (see
// `docs/architecture/crate-boundaries.md` §10a). Consumers that previously
// named the symbols through `nmp_core::` should migrate to `nmp_ffi::*`.
// The `NmpApp` opaque handle moved with the symbols.
//
// V-38: the `nmp_app_wallet_*` FFI symbols moved to `nmp-ffi::wallet` as
// thin shims routing through `nmp.wallet.{connect,disconnect,pay_invoice}`
// (dispatch_action). The actual wallet runtime lives in `crates/nmp-nip47`.
// T118 / G3 — lifecycle observer phase-code constants exposed for
// integration tests. The actor module is crate-private, so this is the only
// Rust-side surface for the wire shape.
#[cfg(any(test, feature = "test-support"))]
pub use actor::{LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};

// Scoped observed-projection sink surface exposed to reusable Rust crates.
// Hosts register these through `substrate::ObservedProjectionRegistrar` with a
// declared event shape; there is no public filterless observer registration.
pub use actor::{ObservedProjectionId, ObservedProjectionSink};

// `KindFilter` is the per-registration kind filter used by `external_event_sink`
// and any consumer that needs to match events by kind. Exported from the
// canonical `actor::kind_filter` module.
pub use actor::KindFilter;

// ── Step 11 final — native FFI re-export surface ───────────────────────────
//
// `nmp-uniffi` and `nmp-native-runtime` reach these symbols through
// `nmp_core::__ffi_internal::*`. The module is `#[doc(hidden)]` — no app
// crate or library consumer should import it directly. Adding a new item
// here is a layer-shape concession (the substrate item was previously
// crate-private), not a public API addition.
//
// Why the special module rather than promoting each item to `pub` at the
// crate root: keeps the public surface area visibly identical to before the
// extraction, and gives `cargo doc` users a single place to spot "this is
// an extraction seam, not a real API".
// Gated on `feature = "native"` because the re-exports below pull in
// `run_actor_with_observers` and friends from `crate::actor`, which are
// themselves `#[cfg(feature = "native")]`. The wasm32 build
// (`--no-default-features`) has no actor thread and no native runtime shell
// consuming this module.
#[cfg(feature = "native")]
#[doc(hidden)]
pub mod __ffi_internal {
    pub use crate::actor::{
        has_role, new_bunker_handshake_slot, new_event_observer_slot, new_lifecycle_observer_slot,
        new_signer_state_slot, nostrconnect_relay_url, register_rust_observer_muted,
        run_actor_with_observers, rust_observer_count, unregister_observer, ActorChannels,
        ActorConfigSources, ActorRuntimeSlots, LifecycleObserverSlot, NativeLifecycleObserver,
        ObservedProjectionSinkSlot, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND,
    };
    // `ActorMail` is the raw inbox discriminant used by the bounded actor
    // inbox shared by native runtime construction. Not part of the
    // stable public surface (#1608); exposed only through this sealed seam so
    // the native layer can name the mail type without making `ActorMail` a general
    // API export.
    pub use crate::actor::ActorMail;
    // V-38: `WalletStatusSlot` / `new_wallet_status_slot` moved to `nmp-nip47`.
    // The host (per-app crate) constructs the slot itself and registers the typed
    // `"wallet"` sidecar via `register_typed_snapshot_projection` (ADR-0072).
    pub use crate::app::KernelAction;
    pub use crate::capability_socket::{
        capability_error_envelope, dispatch_capability, new_capability_callback_slot,
        CapabilityCallbackSlot, NativeCapabilityHandler,
    };
    pub use crate::kernel::{
        default_registry, is_hex_id, is_hex_pubkey, new_app_relay_slot,
        new_snapshot_projection_slot, routing_trace, ActionExecuteFailure, ActionFailureKind,
        ActionRegistry, LifecyclePhase, RegistrationError, SnapshotProjectionSlot,
    };
    // ADR-0072: the typed-projection closure type; `nmp-ffi` reaches it
    // through this internal surface to type the
    // `register_typed_snapshot_projection` seam on the `AppHost` trait.
    pub use crate::kernel::snapshot_registry::TypedProjectionFn;
    // Blocker C: `nmp-ffi` reads the admission result to record a truthful
    // composition-ledger disposition (Installed / Replaced / DroppedFull).
    pub use crate::kernel::snapshot_registry::TypedAdmission;
    // Blocker C test support: the D5 cap constant so the over-cap test can
    // fill the registry to exactly the ceiling without hard-coding the value.
    pub use crate::kernel::snapshot_registry::bounds::MAX_SNAPSHOT_PROJECTIONS;
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
pub mod testing;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
