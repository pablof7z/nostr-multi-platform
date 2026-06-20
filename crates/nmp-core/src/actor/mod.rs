//! Actor main loop — message routing, command dispatch, relay event handling.
//!
//! Idle-tick timing helpers are in `tick.rs`.
//! Relay lifecycle helpers are in `relay_mgmt.rs`.
//!
//! # Dual-channel priority design
//!
//! Commands (`command_rx`) are checked via `try_recv` at the top of every
//! iteration with a bounded burst budget — low latency, never dropped under
//! relay event flood, while relay events and idle work still progress during
//! sustained command bursts.
//! Relay events go through their own separate channel, read via
//! `recv_timeout(compute_wait(…))`. This replaces the old merged
//! `SyncSender<ActorMsg>` design where a 4096-slot bounded channel could fill
//! with relay events and cause `try_send` to silently drop commands like
//! `CreateAccount` during onboarding.

mod commands;
pub mod kind_filter;
// Tier-1 (closure-path) typed-projection codecs for the actor-owned NIP-46
// built-ins `"bunker_handshake"` / `"nip46_onboarding"`. Native-only: the
// `register_typed` registration site is in `run_actor_with_observers`
// (`#[cfg(feature = "native")]`), the only caller of these builders.
// `pub(crate)` so the decode functions (promoted from `#[cfg(test)]`) can be
// re-exported at the crate root via `typed_projections::decode_*`.
#[cfg(feature = "native")]
pub(crate) mod typed_projections;
// V-01 Phase 1c: the actor *runtime* (dispatch / tick / relay management /
// session persistence) sits on top of the native `relay_worker` and is
// therefore native-only. `ActorCommand` (pure data), the observer slots,
// and `relay_roles` (data — pure URL/role canonicalization) stay
// always-compiled below so `publish/action.rs` and every NIP-crate
// `ActionModule::execute` impl can still name `ActorCommand` without the
// `native` feature.
// V-06 / #960 — NIP-42 async-AUTH drain + obligation execution, extracted from
// the actor main loop to keep `mod.rs` within its size budget. Native-only (uses
// the native signer port + relay pool routing).
#[cfg(feature = "native")]
mod auth_sign;
#[cfg(feature = "native")]
mod capability_worker;
#[cfg(feature = "native")]
mod compat;
#[cfg(feature = "native")]
mod config;
#[cfg(feature = "native")]
mod dispatch;
// ADR-0050 §D1/§D3b signer-port dispatch helpers (cipher verbs + completion
// delivery), split out to keep `dispatch.rs` within budget. Native-only (uses
// the native `ActorContext`).
#[cfg(feature = "native")]
mod fairness;
#[cfg(feature = "native")]
mod signer_port_dispatch;
// ADR-0050 §D3a — the single waking actor inbox. `ActorMail` + `CommandSender`
// are always-compiled (the always-compiled `substrate::protocol` seam hands
// `CommandSender` to workers, and `ActorCommand` itself is always-compiled);
// the relay-side scheduler / sink / `Inbox` are `native`-gated inside.
mod inbox;
// Inbox command/relay lane priority + fairness tests, extracted from `inbox.rs`
// to keep that file under the 500 LOC hard cap (AGENTS.md).
#[cfg(all(test, feature = "native"))]
mod inbox_lane_tests;
// Always-compiled port continuations (named by the always-compiled
// `ActorCommand` sign / cipher verbs; not `native`-gated).
mod continuations;
// Generic raw signed-event forwarding dispatch. Native-only: depends on
// `nmp_network::pool::Pool` for outbound `["EVENT", ...]` frames. Policy
// crates provide target selection through a substrate trait object.
#[cfg(all(test, feature = "native"))]
mod app_managed_signer_tests;
#[cfg(all(test, feature = "native"))]
mod cipher_for_account_tests;
#[cfg(all(test, feature = "native"))]
mod nip42_async_auth_tests;
#[cfg(feature = "native")]
mod outbound;
#[cfg(feature = "native")]
pub(crate) mod pending_sign;
#[cfg(all(test, feature = "native"))]
mod protocol_panic_isolation_tests;
#[cfg(all(test, feature = "native"))]
mod publish_relay_dispatch_tests;
#[cfg(feature = "native")]
pub(crate) mod raw_event_forwarder;
#[cfg(feature = "native")]
mod relay_event_guard;
#[cfg(feature = "native")]
mod relay_idle;
#[cfg(feature = "native")]
mod relay_mgmt;
mod relay_roles;
#[cfg(all(test, feature = "native"))]
mod relay_url_canonical_tests;
#[cfg(all(test, feature = "native"))]
mod send_gate_universal_tests;
#[cfg(feature = "native")]
mod session_persistence;
#[cfg(all(test, feature = "native"))]
mod session_persistence_tests;
#[cfg(all(test, feature = "native"))]
mod sign_event_for_account_tests;
#[cfg(all(test, feature = "native"))]
mod signer_port_test_harness;
#[cfg(all(test, feature = "native"))]
mod tests;
#[cfg(feature = "native")]
mod tick;
#[cfg(all(test, feature = "native"))]
mod v87_d1_startup_tests;
#[cfg(all(test, feature = "native"))]
mod v90_capability_worker_tests;

// V-01 Phase 1c: capability callback and identity runtime are native actor runtime only.
#[cfg(feature = "native")]
use crate::capability_socket::new_capability_callback_slot;
#[cfg(feature = "native")]
use commands::IdentityRuntime;
// V-38: the wallet runtime + status slot moved to `crates/nmp-nip47`.
// `nmp-core` no longer has a `wallet` feature, a `WalletRuntime` use, or any
// `WalletStatusSlot` / `new_wallet_status_slot` / `WalletStatus` re-export.
// `KernelEventObserverSlot` and `notify_observers` are consumed by `kernel/event_observer.rs`
// unconditionally — keep them always-compiled. The slot constructors, registration helpers,
// and lifecycle observer types are only consumed by the native FFI and actor runtime.
pub(crate) use commands::notify_observers;
// `KernelEventObserverSlot` and `register_rust_observer` are `pub`
// unconditionally so `nmp-ffi` and wasm32 composition roots can register
// observers. `new_event_observer_slot_headless` is `pub(crate)` — wasm32-safe
// (no drain thread); used by `KernelReducer::new` on all targets.
pub(crate) use commands::new_event_observer_slot_headless;
#[cfg(feature = "native")]
pub use commands::{
    new_event_observer_slot, new_observer_slot as new_lifecycle_observer_slot, unregister_observer,
    LifecycleObserverSlot,
};
pub use commands::{register_rust_observer, KernelEventObserverSlot};
// `register_c_observer` + `LifecycleObserverRegistration` reach `nmp-ffi`
// through `nmp_core::__ffi_internal::*` so the C-ABI bridge in
// `nmp-ffi/src/event_observer.rs` + `lifecycle.rs` can drive the slot.
#[cfg(feature = "native")]
pub use commands::{register_c_observer, LifecycleObserverRegistration};
// D0: NIP-46 remote signing is an app noun — the bunker-handshake slot is
// re-exported so the `ffi` module can build it, hand one clone to the actor's
// `IdentityRuntime`, and capture the other in the built-in
// `"bunker_handshake"` typed snapshot-projection closure.
// V-01 Phase 1c: bunker types are native actor / FFI only.
#[cfg(feature = "native")]
pub(crate) use commands::BunkerHandshakeSlot;
// `nmp-ffi`'s `nmp_app_new` constructs the bunker-handshake slot before
// handing it to the actor; promoted to `pub` for the extracted crate.
#[cfg(feature = "native")]
pub use commands::new_bunker_handshake_slot;
// ADR-0048 D6: generalised remote-signer health slot (hard-break rename of
// the former `BunkerConnectionStateSlot` — no compat aliases). The DTO itself
// stays `commands`-private; callers drive it only through the actor commands.
#[cfg(feature = "native")]
pub use commands::{new_signer_state_slot, SignerStateSlot};
// `pub` (not `pub(crate)`) so the `lib.rs` test-support re-export reaches
// integration tests outside the crate. The `actor` module itself is
// crate-private (`mod actor;` in `lib.rs`), so external Rust callers still
// see these only via the gated `pub use actor::{...}` in lib.rs. The
// `lib.rs` re-export fires in two places: the test-only top-level
// (`#[cfg(any(test, feature = "test-support"))]`) and `__ffi_internal`
// (`#[cfg(feature = "native")]`). Mirror the union of those gates so the
// `pub use` is unused only in a build that consumes neither — wasm32-only
// (`--no-default-features`) without test-support.
#[cfg(any(test, feature = "test-support", feature = "native"))]
pub use commands::{LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};
// T146 — re-export the kernel event observer types so external Rust callers
// (per-app crates such as `nmp-app-chirp`) can implement and register
// `KernelEventObserver`s through the gated `pub use actor::{...}` in
// `lib.rs`. The FFI shape (`KernelEventObserverFn` /
// `KernelEventObserverRegistration` / `KernelEventObserverId`) is also
// surfaced so Swift / Kotlin bindings can use the C-ABI channel.
// `KernelEventObserver` / `KernelEventObserverFn` / `KernelEventObserverId`
// are re-exported unconditionally from `lib.rs` (the typed observer surface
// for per-app Rust crates and the FFI wire-shape). `KernelEventObserverRegistration`
// only reaches the outside world through `lib.rs::__ffi_internal`, which is
// `#[cfg(feature = "native")]`; gate the registration type re-export to match.
#[cfg(feature = "native")]
pub use commands::KernelEventObserverRegistration;
pub use commands::{KernelEventObserver, KernelEventObserverFn, KernelEventObserverId};
// `KindFilter` lives in `kind_filter.rs` (extracted so `external_event_sink`
// can use it without the raw-observer module). Re-exported unconditionally
// from `lib.rs` (used by per-app Rust crates and external_event_sink).
pub use kind_filter::KindFilter;
// NIP golden-tag conformance harness — re-exported up the (crate-private)
// `actor` chain so the gated `pub use actor::ConformanceHarness` in `lib.rs`
// reaches the `tests/nip_tag_conformance.rs` integration test. Gated on
// `test-support` so it never appears in a production build.
// V-01 Phase 1c: the harness sits on the native publish helpers, so the
// `commands` mod gates its re-export the same way; mirror the gate here.
#[cfg(all(any(test, feature = "test-support"), feature = "native"))]
pub use commands::ConformanceHarness;
// V-01 Phase 1c: every import below sits on the native actor runtime
// (`dispatch` / `fairness` / `pending_sign` / `relay_mgmt` / `tick` /
// `relay_worker`). They go away with the rest of the runtime when
// `--no-default-features` is set. `ActorCommand` (the enum below) and the
// observer types remain always-compiled — only the loop that *consumes*
// them is gated.
#[cfg(feature = "native")]
use capability_worker::spawn_capability_worker;
#[cfg(feature = "native")]
pub use config::{ActorChannels, ActorConfig, ActorConfigSources, ActorRuntimeSlots};
#[cfg(feature = "native")]
use dispatch::{dispatch_command, ActorContext};
#[cfg(feature = "native")]
use pending_sign::{resolve_parked_op, AuthObligation, ParkedOp, PublishObligation};

use crate::kernel::LifecyclePhase;

use crate::app::KernelAction;

// ADR-0050 §D3a — always-compiled inbox transport types. `CommandSender` is the
// single command-send seam handed to host code, protocol/capability workers,
// the broker adapter, and the actor's self-feedback path; `ActorMail` is what
// the unified inbox carries. Both name no protocol concept (D0).
pub use inbox::{ActorMail, CommandSendError, CommandSender};
// ADR-0050 §D1 — always-compiled port continuations named by the (always-
// compiled) `ActorCommand` sign / cipher verbs.
pub use continuations::{CipherContinuation, SignContinuation};
// Native-only relay-lane scheduler + receiver wrapper. (`RelayMailSink` is
// constructed via `CommandSender::relay_sink()`, never named here.)
#[cfg(feature = "native")]
use inbox::{CommandLaneDrain, Inbox, LoopStep, MailScheduler};

#[cfg(feature = "native")]
use relay_idle::{sweep_temporary_idle_relays, TEMPORARY_RELAY_IDLE_GRACE};
#[cfg(feature = "native")]
use relay_mgmt::{
    claim_send_gate, close_relays, maybe_send_startup, route_dispatch_outbound, send_all_outbound,
};
#[cfg(feature = "native")]
use tick::{compute_wait, emit_now, flush_due};

#[cfg(feature = "native")]
#[cfg(feature = "native")]
use crate::relay::RelayRole;
#[cfg(feature = "native")]
use crate::relay::{CanonicalRelayUrl, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
#[cfg(feature = "native")]
// Step 8 phase F — actor cut-over to the push-model `Pool` API. The legacy
// `nmp_network::relay_worker::{RelayCommand, RelayEvent, spawn_relay_worker}`
// entry points are no longer named here; with no out-of-crate consumers
// remaining the `relay_worker` module is `pub(crate)` inside `nmp-network`
// (the `pool::Pool` translator wraps it internally). Every per-URL socket
// the actor talks to is now owned by a process-wide `Pool`; the actor
// holds a `RelayHandle` per URL in `RelayControl` and consumes `PoolEvent`s
// on the dedicated relay-event channel below.
#[cfg(feature = "native")]
#[cfg(feature = "native")]
use nmp_network::pool::{Pool, PoolConfig, RelayHandle};
use std::collections::HashMap;
#[cfg(feature = "native")]
use std::collections::HashSet;
#[cfg(feature = "native")]
use std::sync::atomic::Ordering;
#[cfg(feature = "native")]
#[cfg(feature = "native")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "native")]
use std::time::{Duration, Instant};

/// #1069 — interval between bounded GC passes on the actor idle tick.
///
/// `gc.md` §3: "Every 60 seconds." Gated with an `Instant`-based `last_gc`
/// local in `run_actor_with_observers` — a pure performance-timing read (like
/// `last_emit` / `TEMPORARY_RELAY_IDLE_GRACE`), never a business-logic clock,
/// so it stays D9-clean. The *event* time fed to `gc_step` is still the kernel
/// clock (`Kernel::run_gc_step` → `now_secs`); this gate only paces how often
/// the pass fires. Piggy-backs the existing ≤250 ms `compute_wait` loop wake —
/// no new sleep loop, no timer thread (D8 / AGENTS.md "no polling").
#[cfg(feature = "native")]
pub(crate) const GC_TICK_INTERVAL: Duration = Duration::from_secs(60);

// `has_role` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::has_role` (the FFI surface filters relay-edit
// rows by role when computing the write-relay slice for the per-app crate's
// MLS / NIP-17 publish path).
pub use relay_roles::has_role;
pub(crate) use relay_roles::{canonical_relay_role, relay_role_options};
// V6 Stage 1 — Swift codegen pilot. `RelayRoleOption` is `pub(crate)` in
// `relay_roles`; re-exported here so `crate::codegen_schema` can hand it
// to `schemars::schema_for!` from the schema-dump binary. The type stays
// crate-private; the re-export is `pub(crate)`, the bin runs inside the
// crate. Gated to the codegen-schema build so non-codegen builds don't
// trip the unused-import lint (no in-crate consumer outside codegen_schema).
#[cfg(feature = "codegen-schema")]
pub(crate) use relay_roles::RelayRoleOption;
// `nostrconnect_relay_url` is consumed by `nmp-ffi` (native only) through
// `nmp_core::__ffi_internal::nostrconnect_relay_url`.
#[cfg(feature = "native")]
pub use relay_roles::nostrconnect_relay_url;

/// Where a signer added via [`ActorCommand::AddSigner`] comes from.
///
/// Replaces the per-source `SignInNsec` / `SignInBunker` / `AddRemoteSigner`
/// command split: the source kind is now a payload of one unified command.
///
/// D0: the `RemoteHandle` arm carries a `Box<dyn RemoteSignerHandle>` whose
/// concrete type lives in `nmp-signers` — `nmp-core` only sees the trait object
/// (defined in [`crate::remote_signer`]); it never imports the broker or signer
/// crate.
#[allow(dead_code)] // live cross-crate constructors in nmp-ffi — per-crate lint false positive
pub enum SignerSource {
    /// Local secret key — a `nsec1…` bech32 or 64-hex string. Resolves
    /// synchronously: the actor parses it and (when `make_active`) activates it
    /// immediately. Carried as [`zeroize::Zeroizing<String>`] so the plaintext
    /// secret is wiped from memory the instant the command is dropped — the
    /// in-flight window between FFI ingest and key parsing is minimized.
    LocalNsec(zeroize::Zeroizing<String>),
    /// Local secret key for an app-managed signer slot. The actor registers it
    /// in the signer roster, persists it as app-managed local material, and
    /// keeps it hidden from user account projections and active-account
    /// switching.
    AppManagedLocalNsec(zeroize::Zeroizing<String>),
    /// NIP-46 `bunker://` URI. Triggers an asynchronous broker handshake: the
    /// actor seeds the `bunker_handshake` projection, stashes `make_active`, and
    /// delegates the connect/get_public_key dance to the registered broker. The
    /// broker reports completion by sending back an `AddSigner` carrying a
    /// [`SignerSource::RemoteHandle`].
    BunkerUri(String),
    /// A fully-handshaken remote signer handle. The broker adapter constructs
    /// this after a NIP-46 handshake completes and sends it back to the actor,
    /// which inserts it into `IdentityRuntime.remote_signers` and applies
    /// `make_active` (the value the originating `BunkerUri` command stashed).
    RemoteHandle(Box<dyn crate::RemoteSignerHandle>),
}

impl std::fmt::Debug for SignerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret: `LocalNsec` redacts its payload. `Box<dyn
        // RemoteSignerHandle>` is not `Debug`, so `RemoteHandle` prints only its
        // discriminant + the handle's pubkey.
        match self {
            SignerSource::LocalNsec(_) => f.write_str("LocalNsec(<redacted>)"),
            SignerSource::AppManagedLocalNsec(_) => f.write_str("AppManagedLocalNsec(<redacted>)"),
            SignerSource::BunkerUri(uri) => f.debug_tuple("BunkerUri").field(uri).finish(),
            SignerSource::RemoteHandle(handle) => f
                .debug_tuple("RemoteHandle")
                .field(&handle.pubkey_hex())
                .finish(),
        }
    }
}

/// Actor command variants.  The `actor` module is private (`mod actor`, not
/// `pub mod actor`), so this `pub` is only reachable from outside the crate
/// through the `testing` re-export gate.  In normal (non-test-support) builds
/// nothing re-exports these items, so they remain effectively crate-private.
#[derive(Debug)]
pub enum ActorCommand {
    Start {
        visible_limit: usize,
        emit_hz: u32,
        /// App-declared initial relay configuration. Seeded into `configured_relays`
        /// before the session restore runs. Empty for C-ABI callers that seed via
        /// pre-start `add_relay` calls instead.
        initial_relays: Vec<(String, String)>,
    },
    Configure {
        visible_limit: usize,
        emit_hz: u32,
    },
    // V-68 / V-112 (ADR-0042): OpenAuthor{kinds}, OpenThread{kinds} deleted.
    // Apps register a per-app FlatFeed (nmp_app_chirp_open_author_feed etc.)
    // and call OpenInterest for kernel admission — D0-clean since primary-kind
    // policy lives in the app, not in nmp-core.
    /// Sign an unsigned event using the named account's signer and park the
    /// result in the `signed_events` snapshot projection keyed by
    /// `correlation_id`. The caller polls the projection to retrieve the
    /// signed event JSON. Works for both local nsec (resolves immediately) and
    /// NIP-46 bunker (resolves asynchronously via a parked `ParkedOp` with the
    /// `SignedEventsProjection` sink).
    ///
    /// Unlike every other sign path in the actor, this NEVER publishes — the
    /// signed event is handed straight back to the host through the projection
    /// so the host can attach it to an out-of-band transport (e.g. a Blossom
    /// upload `Authorization: Nostr …` header). This closes the D13 gap where
    /// a host that needed a signed auth event had to read raw private key bytes
    /// across the FFI boundary, which is impossible for NIP-46 bunker users.
    ///
    /// `unsigned_json` is a JSON object with fields:
    ///   `{ "kind": u64, "content": str, "tags": [[str, ...], ...], "created_at": u64 }`
    /// The `created_at` field is advisory — the actor re-stamps it from the
    /// kernel clock (D7) so the host never owns wall-clock time.
    ///
    /// `account_pubkey` is the hex pubkey of the registered signer to use.
    /// Pass the empty string `""` to use the active account.
    SignEventForReturn {
        account_pubkey: String,
        unsigned_json: String,
        correlation_id: String,
    },
    /// Generic, backend-transparent sign-account port for `ProtocolCommand`
    /// workers (ADR-0043 Decision 2). Sign `unsigned` with the named account
    /// (`signer_pubkey = Some(hex)`) or the active account (`None`), then invoke
    /// `continuation` with the resolved [`SignedEvent`] (or an error string).
    ///
    /// Local-vs-bunker is invisible to the caller: the dispatch arm routes
    /// through `sign_active_nonblocking` / `sign_with_account_nonblocking` (the
    /// same functions the publish path uses, which look across BOTH local keys
    /// and remote signers). A local key resolves `Ready` and the continuation
    /// runs inline on the actor thread; a NIP-46 bunker resolves `Pending` and
    /// is parked in [`crate::actor::pending_sign::ParkedOp`] with the
    /// `SignContinuation` sink — the idle-loop drain invokes the continuation when the
    /// broker turns the request around (or on timeout, with an `Err`). Either
    /// way the worker code path is identical.
    ///
    /// The continuation runs on the actor thread and MUST only enqueue further
    /// work (e.g. spawn an HTTP worker via the cloned `Sender<ActorCommand>`) —
    /// never block (D8). It never receives raw key bytes — only a `SignedEvent`
    /// (D13).
    ///
    /// `signer_pubkey` matches the publish-path field byte-for-byte
    /// (`PublishUnsignedEvent` / `PublishUnsignedEventToRelays`): `None` = active
    /// account, `Some(pubkey)` = a named roster key.
    ///
    // V-78 reconcile (done): `nmp-nip57`'s `FetchLnurlInvoiceCommand` consumes
    // this port to sign the kind:9734 zap request (active account →
    // `signer_pubkey: None`), so a NIP-46 bunker can zap through the SAME seam
    // as a local nsec. One signing seam, both backends; the redundant
    // `ProtocolCommandContext::sign_active_nonblocking` method it used to call
    // is gone.
    SignEventForAccount {
        /// The unsigned event to sign. `created_at` should already be stamped
        /// by the caller from the kernel clock (D7).
        unsigned: crate::substrate::UnsignedEvent,
        /// `None` = active account; `Some(hex)` = a named roster key.
        signer_pubkey: Option<String>,
        /// Invoked with the resolved sign outcome — inline (local) or from the
        /// idle-loop drain (bunker / timeout).
        continuation: SignContinuation,
    },
    /// Backend-transparent NIP-44 ENCRYPT-account port — the cipher sibling of
    /// [`Self::SignEventForAccount`] (ADR-0050 §D1). Encrypt `plaintext` to
    /// `peer_pubkey` with the named (`Some(hex)`) or active (`None`) account,
    /// then invoke `continuation` with the ciphertext (or error). Local accounts
    /// run `nostr::nips::nip44` inside the identity runtime (D13); remote
    /// accounts route through `RemoteSignerHandle::nip44_encrypt` and park under
    /// the `CipherContinuation` sink. The continuation runs on the actor thread,
    /// only enqueues work (D8), and receives only ciphertext (D13). D0:
    /// `nip44_*` is a crypto capability (present since ADR-0026), not an app noun.
    Nip44EncryptForAccount {
        /// Recipient pubkey (lowercase hex) the plaintext is encrypted to.
        peer_pubkey: String,
        /// The plaintext to encrypt.
        plaintext: String,
        /// `None` = active account; `Some(hex)` = a named roster key.
        signer_pubkey: Option<String>,
        /// Invoked with the resolved ciphertext (or an error string).
        continuation: CipherContinuation,
    },
    /// Backend-transparent NIP-44 DECRYPT-account port — the inbound twin of
    /// [`Self::Nip44EncryptForAccount`] (ADR-0050 §D1). Same contract; decrypts
    /// `ciphertext` from `peer_pubkey` to plaintext.
    Nip44DecryptForAccount {
        /// Sender pubkey (lowercase hex) the ciphertext was encrypted from.
        peer_pubkey: String,
        /// The ciphertext to decrypt.
        ciphertext: String,
        /// `None` = active account; `Some(hex)` = a named roster key.
        signer_pubkey: Option<String>,
        /// Invoked with the resolved plaintext (or an error string).
        continuation: CipherContinuation,
    },
    /// Deliver an inbound remote-signer response for correlation-keyed dispatch
    /// (ADR-0050 §D3b) — the actor-mailbox completion path for steady-state
    /// replies. Both backends route here instead of resolving the parked op on a
    /// foreign thread: NIP-46 via the broker's opaque completion sink (installed
    /// by nmp-ffi; broker stays `nmp-core`-free, D0), NIP-55 via
    /// `external_signer.rs::deliver` (handshake path unaffected). The arm fans
    /// the JSON to every remote handle (each drops non-matching ids — the trait
    /// contract). Because the send lands on the single waking inbox (§D3a), the
    /// completion wakes the actor and the SAME iteration drains the parked-op
    /// queue — no ≤250ms tick dependence; the pending-map mutation is on the
    /// actor thread (D4 single-writer).
    DeliverSignerResponse {
        /// The already-decoded signer response (NIP-46: decrypted RPC body;
        /// NIP-55: serialized `ExternalSignerResponse`), passed verbatim to each
        /// handle's `deliver_response`.
        response_json: String,
    },
    /// Unified sign-in command. Adds a signer to the actor-local identity store
    /// from one of the [`SignerSource`] variants and, when `make_active` is set,
    /// binds it as the active signer + retargets the timeline.
    ///
    /// This is the single entry point that replaces the old `SignInNsec`,
    /// `SignInBunker`, and `AddRemoteSigner` variants:
    ///
    /// * [`SignerSource::LocalNsec`] — parse the secret synchronously and (when
    ///   `make_active`) activate immediately.
    /// * [`SignerSource::BunkerUri`] — shape-validate the `bunker://` URI, seed
    ///   the `bunker_handshake` projection with `"connecting"`, stash
    ///   `make_active` for the async round-trip, and delegate the handshake to
    ///   the registered broker. D0: the broker app/FFI adapter translates the
    ///   app-neutral broker event back into `AddSigner { RemoteHandle, .. }`;
    ///   `nmp-core` never imports the broker or signer crate.
    /// * [`SignerSource::RemoteHandle`] — register a fully-handshaken remote
    ///   signer (e.g. completed NIP-46 bunker handshake) and apply `make_active`
    ///   immediately. The broker adapter sends this after the handshake
    ///   completes, threading through the `make_active` value the `BunkerUri`
    ///   command originally stashed.
    ///
    /// Has live cross-crate callers in `nmp-ffi` (the C-ABI sign-in shims and
    /// the broker adapter); `#[allow(dead_code)]` only suppresses rustc's
    /// per-crate dead-code lint, which cannot see the cross-crate constructors.
    #[allow(dead_code)]
    AddSigner {
        source: SignerSource,
        make_active: bool,
    },
    /// Create a new keypair, publish a kind:0 metadata event and a kind:10002
    /// relay-list event, then register the identity and make it active.
    ///
    /// `profile` is a map of key/value pairs that is JSON-serialised into the
    /// kind:0 `content` field.  `relays` is a list of `(url, role)` tuples
    /// where `role` is `"read"`, `"write"`, `"both"`, `"indexer"`, or a
    /// comma-separated composite such as `"both,indexer"`. `mls` requests
    /// account-scoped MLS setup in app composition crates.
    ///
    /// `initial_follows` is the app-supplied set of hex pubkeys the fresh
    /// account auto-follows (kind:3 contacts prepopulate + cold-start publish).
    /// NMP no longer hardcodes any default follow set — auto-follow is operator
    /// policy that originates in the leaf app (#1493). An empty vector means the
    /// account starts with no contacts and no cold-start kind:3 is published.
    CreateAccount {
        profile: HashMap<String, String>,
        relays: Vec<(String, String)>,
        initial_follows: Vec<String>,
        mls: bool,
        /// Whether to make the newly created account the active account.
        /// `true` for the standard onboarding flow; `false` for creating
        /// an agent/secondary account without disturbing the active session.
        make_active: bool,
    },
    /// T66a identity — switch the active account (synchronous re-bind +
    /// timeline retarget, mirrors `AccountManager::switch_active` semantics).
    SwitchActive {
        identity_id: String,
    },
    /// T66a identity — remove an account; clears the active slot if it was
    /// the active one.
    RemoveAccount {
        identity_id: String,
    },
    /// Broker adapter → actor: progress event for the bunker handshake UI.
    /// Actor stores the latest into a kernel snapshot field; the adapter is
    /// the sole writer. Stage `"idle"` clears the projection. Has a live
    /// production caller in the app/FFI broker adapter; `#[allow(dead_code)]`
    /// only suppresses rustc's per-crate lint, which cannot see it.
    #[allow(dead_code)]
    // live cross-crate caller in nmp-ffi — per-crate lint false positive
    BunkerHandshakeProgress {
        /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`.
        stage: String,
        /// Optional human-readable status (e.g. relay URL, error reason).
        message: Option<String>,
    },
    /// V-14 step b — relay-layer connection state update for the NIP-46 bunker
    /// session. Emitted by the `nmp-ffi` broker adapter when the broker emits
    /// `BrokerEvent::ConnectionStateChanged`. The actor writes it to the shared
    /// `SignerStateSlot` (ADR-0048 D6 — the unified remote-signer health slot);
    /// the built-in `"signer_state"` snapshot projection reads the slot on
    /// every tick.
    ///
    /// D4: the actor is the sole writer of the slot — the broker callback routes
    /// through this command (not directly to the slot) so the write happens on
    /// the actor thread.
    #[allow(dead_code)]
    // live cross-crate caller in nmp-ffi — per-crate lint false positive
    BunkerConnectionStateChanged {
        /// `"connected"` | `"reconnecting"` | `"failed"`.
        state: String,
        /// Optional human-readable reason (error message on reconnect/failed).
        reason: Option<String>,
    },
    /// ADR-0048 D6 — NIP-55 external-signer health update for the unified
    /// `signer_state` projection. Emitted by the `nmp-ffi` NIP-55 driver when
    /// the host capability bridge reports an outcome that affects long-lived
    /// signer health (awaiting approval, ready, rejected, unavailable).
    ///
    /// D4: the actor is the sole writer of the `SignerStateSlot` — the driver
    /// routes through this command (not directly to the slot) so the write
    /// happens on the actor thread. Mirrors `BunkerConnectionStateChanged`.
    #[allow(dead_code)]
    // live cross-crate caller in nmp-ffi — per-crate lint false positive
    Nip55SignerStateChanged {
        /// `"ready"` | `"awaiting_approval"` | `"unavailable"` | `"failed"`.
        state: String,
        /// Optional human-readable reason (rejection/unavailable detail).
        reason: Option<String>,
    },
    /// Sign-and-publish an arbitrary event kind for the active account.
    /// The actor fills `pubkey` from the active signer, stamps `created_at`
    /// (D7), signs, and routes through the NIP-65 outbox per `target`.
    /// Dispatched by `PublishAction::PublishRaw` via `dispatch_action`.
    ///
    /// Both local-keys and remote (NIP-46) signer accounts are supported —
    /// the dispatch arm delegates to the existing `publish_unsigned_event` /
    /// `publish_unsigned_event_to_relays` helpers, which already park bunker
    /// signs as a `ParkedOp` with the `Publish` sink (D8 — actor never blocks).
    PublishRawEvent {
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        target: crate::publish::PublishTarget,
        /// When `Some(pubkey)`, the actor signs with the account whose pubkey
        /// matches — looked up across BOTH local keys and remote signers — via
        /// `sign_with_account_nonblocking`, instead of the active account. This
        /// is the `PublishAction::PublishRaw` signer selector: it lets an agent
        /// / per-podcast key publish without ever becoming the active account.
        /// `None` preserves the legacy behaviour: sign with the active account.
        signer_pubkey: Option<String>,
        correlation_id: Option<String>,
    },
    /// T66a publish — sign a kind:0 profile metadata event with the active
    /// account and emit it to the NIP-65 outbox-resolved write relays (D3).
    ///
    /// `fields` is the flat string map the host supplied; the actor serializes
    /// it into the kind:0 `content`, stamps `created_at` from `kernel.now_secs()`
    /// (the host never hand-rolls the timestamp), and signs. Sibling of
    /// [`ActorCommand::PublishRawEvent`] — same sign-and-publish path, kind:0
    /// instead of an arbitrary kind.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from `nmp_app_dispatch_action` (`PublishAction::PublishProfile`).
    /// Threading it through makes the publish engine report it in
    /// `action_results` so the host spinner keyed on the dispatch return
    /// value can be cleared. `None` for non-dispatch callers.
    PublishProfile {
        fields: serde_json::Map<String, serde_json::Value>,
        correlation_id: Option<String>,
    },
    /// Generic, kind-agnostic publish — take an `UnsignedEvent` already built
    /// by any protocol-crate builder (`nmp_nip23::Article`, `nmp_nip01::Note`,
    /// `nmp_relations::Reaction`, …), sign with the active account's keys,
    /// and route through the NIP-65 outbox resolver (D3). The kernel does
    /// not inspect the kind — that's the protocol crate's concern (D0).
    ///
    /// Stepping stone toward per-protocol-crate `ActionModule` impls
    /// (`kind-wrappers.md` §8 Phase 1); deprecates kind-by-kind as those land.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from an `ActionModule::execute` call. Threading it lets the
    /// publish engine report THAT id in `action_results` (via
    /// `correlation_id_override`) so the host spinner closes on the id it
    /// received from `dispatch_action`, not on the signed event's id.
    /// `None` for callers that are not action-dispatched (e.g. direct
    /// `NmpApp::` Rust API calls, conformance tests).
    PublishUnsignedEvent {
        event: crate::substrate::UnsignedEvent,
        correlation_id: Option<String>,
        /// When `Some(pubkey)`, the actor signs with the account whose pubkey
        /// matches — looked up across BOTH local keys and remote signers — via
        /// `sign_with_account_nonblocking`, instead of the active account. This
        /// lets a non-active account publish without first switching active.
        /// `None` preserves the legacy behaviour: sign with the active account
        /// (and fail closed when no account is active).
        signer_pubkey: Option<String>,
    },
    /// Publish an unsigned event to an explicit relay set, bypassing the
    /// NIP-65 outbox resolver. Used by action executors that target a
    /// specific relay pin (e.g. NIP-29 group relays). D4: only the actor
    /// signs and publishes. D8: no blocking — relay dispatch is async.
    ///
    /// Sibling to [`ActorCommand::PublishUnsignedEvent`] (which routes via the
    /// NIP-65 outbox) and [`ActorCommand::PublishSignedEvent`] (which carries
    /// an already-signed event). This variant SIGNS with the active account
    /// like the unsigned sibling, but ROUTES to exactly `relays` like the
    /// signed sibling's `Explicit` mode — the combination a host-pinned group
    /// action needs. A NIP-29 join request must reach the group's own host
    /// relay, never the author's kind:10002 outbox.
    ///
    /// Like the unsigned sibling, the event's `pubkey` is derived from the
    /// active identity at sign time; the caller's `event.pubkey` is ignored.
    /// Empty or malformed `relays` fail closed in the publish handler. Callers
    /// that want NIP-65 outbox routing must use [`ActorCommand::PublishUnsignedEvent`]
    /// so `Auto` and `Explicit` never share the same empty-vector encoding.
    PublishUnsignedEventToRelays {
        event: crate::substrate::UnsignedEvent,
        relays: Vec<crate::publish::RelayUrl>,
        /// Registry-minted `correlation_id` from `dispatch_action`, when this
        /// command originates from an `ActionModule::execute` call. Threading
        /// it lets the publish engine report THAT id in `action_results`
        /// (via `correlation_id_override`) so the host spinner closes on the
        /// id it received from `dispatch_action`, not on the signed event's id.
        /// `None` for callers that are not action-dispatched (e.g. direct
        /// `NmpApp::` Rust API calls).
        correlation_id: Option<String>,
        /// When `Some(pubkey)`, the actor signs with the account whose pubkey
        /// matches — looked up across BOTH local keys and remote signers — via
        /// `sign_with_account_nonblocking`, instead of the active account.
        /// `None` preserves the legacy behaviour (sign with the active account).
        signer_pubkey: Option<String>,
    },
    /// Generic publish of an **already-signed** event. The kernel verifies
    /// the Schnorr signature + event-id hash, then routes the event verbatim
    /// through the same planner / NIP-65 outbox / relay-pin path the unsigned
    /// command uses — the signer is never consulted (no re-signing). Unlike
    /// [`ActorCommand::PublishUnsignedEvent`], this does not require an active
    /// account: the signature already exists and routing keys off the event's
    /// own pubkey. Generic capability (D0); externally-signed group events are
    /// the first consumer but the kernel has no protocol nouns.
    ///
    /// `target` selects the D3 routing mode without erasing intent:
    /// `Auto` asks the kernel to resolve via NIP-65, while
    /// `Explicit { relays }` dispatches to exactly those relays and fails
    /// closed when the set is empty or malformed.
    ///
    /// `correlation_id` is the registry-minted action id when this publish
    /// originates from `nmp_app_dispatch_action`'s `PublishAction::Publish`
    /// path. Threading it makes the publish engine report THAT id in
    /// `action_results` (via `correlation_id_override`) — explicit symmetry
    /// with the `PublishRaw` path. `None` for non-dispatch callers
    /// (`NmpApp::publish_signed_explicit` — Marmot's MLS / gift-wrap seam,
    /// which replaced the deleted `nmp_app_publish_signed_event*` symbols
    /// with this typed Rust API — and conformance harnesses); the engine
    /// then falls back to the publish handle (== event id), preserving
    /// prior behaviour. The pre-signed `Publish` round-trip already happened
    /// to work by coincidence (`preferred_action_id` returns `event.id`, the
    /// `None`-fallback also reports `event.id`); this field upgrades that
    /// coincidence into an explicit guarantee a host can rely on.
    PublishSignedEvent {
        raw: crate::store::RawEvent,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    },
    // V-39: `SendGiftWrappedDm` variant deleted — the equivalent path now
    // dispatches `ActorCommand::Protocol(Box::new(
    // nmp_nip17::SendGiftWrappedDmCommand { ... }))`, which runs in
    // `nmp-nip17` and reaches the publish engine through the substrate
    // [`crate::substrate::ProtocolCommandContext::send`] follow-up channel
    // (it emits a `PublishSignedEvent` follow-up per envelope).
    /// User intent from the outbox UI: retry a still-pending publish now.
    RetryPublish {
        handle: String,
    },
    /// User intent from the outbox UI: cancel a still-pending publish.
    CancelPublish {
        handle: String,
    },
    /// T66a publish — append `pubkey` to the active account's kind:3 follow
    /// set and re-publish it.
    Follow {
        pubkey: String,
        /// Registry-minted action id when this Follow originates from
        /// `nmp_app_dispatch_action` (`nmp.follow`). See `React` for the
        /// spinner round-trip contract.
        correlation_id: Option<String>,
    },
    /// T66a publish — remove `pubkey` from the kind:3 follow set.
    Unfollow {
        pubkey: String,
        /// Registry-minted action id when this Unfollow originates from
        /// `nmp_app_dispatch_action` (`nmp.unfollow`). See `React` for the
        /// spinner round-trip contract.
        correlation_id: Option<String>,
    },
    /// T66a relay edit — add a relay row (role: `read` | `write` | `both`).
    AddRelay {
        url: String,
        role: String,
    },
    /// T66a relay edit — remove a relay row.
    RemoveRelay {
        url: String,
    },
    /// (Re)open the contact-feed subscription for the active account.
    ///
    /// `kinds` is the compiled acquisition kind set the follow-set REQ should
    /// carry. D0: `nmp-core` does not know which primary kinds or wrapper
    /// policy belong to the host's app concept; the caller supplies the
    /// compiled set so the substrate carries no app-specific social knowledge.
    /// The actor folds it into the kernel via
    /// `Kernel::set_follow_feed_kinds`, which re-registers the active account's
    /// follow-feed M2 interests under the new kind set. An empty set is
    /// equivalent to `CloseContactFeed` — it withdraws all follow-feed interests.
    OpenContactFeed {
        kinds: std::collections::BTreeSet<u32>,
    },
    /// Tear down the contact-feed subscription opened by `OpenContactFeed`.
    ///
    /// Calls `Kernel::set_follow_feed_kinds(BTreeSet::new())`, which clears the
    /// stored kinds and withdraws all follow-feed M2 interests from the lifecycle
    /// registry. The unconditional `FollowListChanged` trigger propagates to
    /// `drain_lifecycle_tick`, which emits CLOSE frames for any live REQs.
    /// D6: no active account (or no prior open) is a silent no-op.
    CloseContactFeed,
    /// Refcounted profile (kind:0) claim. `force` (F-TTL) bypasses the TTL
    /// freshness gate so a user-initiated navigation / pull-to-refresh always
    /// re-verifies the cached profile; `force == false` is the lazy, gated
    /// path used by background claims and `.onAppear` list rows.
    ///
    /// `liveness` is the client freshness hint: `CacheOk` (a feed avatar —
    /// serve from cache, OneShot fetch on miss, no live sub) vs `Live` (an
    /// open profile screen — a Tailing kind:0 sub so profile edits arrive
    /// reactively). Mixed claims on one pubkey resolve to Tailing.
    ClaimProfile {
        pubkey: String,
        consumer_id: String,
        force: bool,
        liveness: crate::kernel::ProfileLiveness,
    },
    ReleaseProfile {
        pubkey: String,
        consumer_id: String,
    },
    /// Refcounted event claim — drives the generic `claim_event` kernel
    /// primitive (F-CR-06 / ADR-0034). `uri` is a `nostr:` URI
    /// (nevent/note/naddr); profile URIs are rejected (use `ClaimProfile`).
    /// Symmetric with `ClaimProfile` in shape and dispatch. `force` (F-TTL)
    /// bypasses the TTL freshness gate for addressable (naddr) coordinates;
    /// it is a silent no-op for immutable nevent/note URIs.
    ClaimEvent {
        uri: String,
        consumer_id: String,
        force: bool,
    },
    /// Release a previously claimed event (the same `uri` +
    /// `consumer_id` pair). On the last consumer's release the
    /// `event_claims[primary_id]` row is removed and
    /// `event_claim_requested` is cleared so a re-claim can re-fetch.
    ReleaseEvent {
        uri: String,
        consumer_id: String,
    },
    // V-68 / V-112 (ADR-0042): CloseAuthor / CloseThread deleted.
    // V-38: the three `Wallet{Connect,Disconnect,PayInvoice}` variants moved
    // out. Wallet connect / disconnect / pay_invoice now route through
    // `ActorCommand::Protocol(Box<dyn ProtocolCommand>)` with concrete
    // `WalletConnectCommand` / `WalletDisconnectCommand` /
    // `WalletPayInvoiceCommand` impls in `crates/nmp-nip47/src/protocol.rs`.
    // `nmp-core` no longer has a `wallet` Cargo feature and no longer
    // depends on `nmp-nwc`. D0: nmp-core names no NIP-47 / NWC nouns.
    //
    // V-41: the closed-enum `FetchLnurlInvoice` variant moved to
    // `nmp_nip57::lnurl::FetchLnurlInvoiceCommand` and dispatches through
    // [`ActorCommand::Protocol`]. `nmp-core` no longer carries any zap
    // nouns (D0). The dispatch arm + handler are deleted; the surface a
    // host sees is unchanged (toast + correlation_id closure remain
    // identical).
    /// T118 / G3 — app lifecycle phase transition reported by the host shell
    /// (or any conforming consumer). The actor folds the phase into the
    /// kernel's [`crate::kernel::LifecyclePhase`] state and, on a
    /// meaningful transition (`Background → Foreground`, `Foreground →
    /// Background`, or first phase after boot), fires the registered
    /// lifecycle observer. The observer is what fans the transition out to
    /// the shell's sync-trigger engine (typically on a foreground
    /// transition); nmp-core itself does not name any shell vocabulary (D0).
    /// Idempotent: rapid scene oscillation debounces to a single observer
    /// call per transition.
    LifecycleEvent(LifecyclePhase),
    /// Host acknowledgement of a `correlation_id` in the
    /// `action_stages` snapshot mirror. The actor folds the ack into the
    /// kernel's `ActionStageTracker`, dropping the entry's stage history
    /// so the next tick's snapshot no longer carries it. Idempotent: an
    /// unknown id is a silent no-op (D6).
    ///
    /// Originates from the FFI symbol `nmp_app_ack_action_stage`. The host
    /// calls this after rendering a terminal stage (`Accepted` or
    /// `Failed`) and clearing its UI; until the ack arrives the entry
    /// stays in the snapshot, so a tick the host missed cannot strand
    /// the action's state machine.
    AckActionStage(String),
    /// Record a terminal `Failed` stage for `correlation_id` on
    /// behalf of an executor that panicked (or otherwise failed *after*
    /// the registry minted the correlation id and before any
    /// `ActorCommand` carrying it could be enqueued).
    ///
    /// Without this seam the failure is orphaned: the host received a
    /// `correlation_id` from `nmp_app_dispatch_action`'s error envelope but
    /// has no way to ACK an `action_stages` entry that was never produced.
    /// The actor folds this command into [`Kernel::record_action_failure`]
    /// — same engine the sign-step failure path uses — so a `Failed`
    /// terminal lands in both `action_stages` (the mirror, for the host's
    /// ACK lifecycle) and `action_results` (the drain, for the host's
    /// spinner cleanup).
    ///
    /// Originates from [`crate::ffi::action::dispatch_action_json`] on the
    /// FFI thread when the executor returned an `Err` (including a
    /// `catch_unwind`-converted panic). Idempotent w.r.t. a buggy host
    /// that re-sends — `record_action_failure` records a second `Failed`
    /// stage, which is a benign no-op for the host (it sees the same
    /// terminal twice; the second ACK is a silent no-op).
    RecordActionFailure {
        correlation_id: String,
        reason: String,
    },
    /// Store a fetched relay-information document on the kernel's per-URL
    /// transport row (ADR-0051). Posted by the `nmp-nip11` fetch worker; the
    /// dispatch arm folds the parsed `RelayInfoDoc` via
    /// [`Kernel::set_relay_info`] so the `relay_diagnostics` projection
    /// surfaces it. `nmp-core` names no NIP-11 noun — it carries the
    /// substrate-generic `RelayInfoDoc` (D0); malformed JSON is a no-op (D6).
    SetRelayInfo {
        /// The relay URL the document was fetched for (canonicalised on store).
        relay_url: String,
        /// `RelayInfoDoc` serialised via `RelayInfoDoc::to_json`.
        doc_json: String,
    },
    /// Record a terminal `Accepted` stage for `correlation_id` on
    /// behalf of an off-thread worker whose success outcome is observed
    /// outside the publish engine. The symmetric counterpart to
    /// [`ActorCommand::RecordActionFailure`]: same routing through
    /// [`Kernel::record_action_success`], which writes both the
    /// `action_stages` mirror (so the host's stage observer sees the
    /// terminal) and the `action_results` per-tick drain (so a spinner
    /// keyed on the `correlation_id` clears).
    ///
    /// The motivating consumer is off-band action settlement such as NIP-47
    /// `pay_invoice`: after the kind:23195 wallet response arrives, the
    /// runtime needs to close the original action promise by correlation id.
    /// The same path closes NIP-57 zaps because their LNURL worker dispatches
    /// wallet payment internally instead of asking the host to pay a toasted
    /// invoice.
    ///
    /// Idempotent w.r.t. a buggy worker that re-sends — `record_action_success`
    /// records a second `Accepted` stage, which is a benign no-op for the
    /// host (it sees the same terminal twice; the second ACK is a silent
    /// no-op).
    RecordActionSuccess {
        correlation_id: String,
        /// ADR-0043 Decision 4 — opaque structured result body forwarded
        /// verbatim into the `action_results[correlation_id]` row's `result`
        /// field. `nmp-core` NEVER parses it (D0: no protocol noun in the
        /// substrate). `None` for the NWC pay-invoice path; `Some(json)` for a
        /// protocol crate (e.g. a Blossom blob descriptor) carrying a return
        /// payload.
        result_json: Option<String>,
    },
    Stop,
    Reset,
    Shutdown,
    /// Generic FFI-boundary action (T95). Routed through the
    /// [`dispatch_kernel_action`] reducer; the resolved [`KernelUpdate`] is
    /// serialized and pushed on the update channel. `OpenUri` registers the
    /// resolved interest through the single-writer registry (D4).
    Kernel(KernelAction),
    /// Open-seam command dispatched through the
    /// [`crate::substrate::ProtocolCommand`] trait. NIP crates use this
    /// instead of adding their own variant to `ActorCommand`
    /// (`docs/architecture/crate-boundaries.md` §4.1, step 1.b). Step 1.b
    /// adds the variant + dispatch arm but no NIP code uses it yet; step 4
    /// (V-41 LNURL fetcher) is the first migration onto the seam.
    Protocol(Box<dyn crate::substrate::ProtocolCommand>),
    /// Ingest pre-verified timeline events through the test-support kernel path.
    ///
    /// The caller is responsible for constructing `VerifiedEvent` values; this
    /// command routes each through `kernel::ingest_pre_verified_event` under the
    /// `"diag-firehose-stress"` sub-id. It inserts through the `EventStore`, then
    /// updates the lightweight read-cache directly. No signature re-verification
    /// is performed — the `VerifiedEvent` type is the gate.
    ///
    /// Test-support only (D0: not part of production FFI surface).
    #[cfg(any(test, feature = "test-support"))]
    IngestPreVerifiedEvents(Vec<crate::store::VerifiedEvent>),
    /// Test-support — ingest pre-verified events under a caller-chosen `sub_id`
    /// that does NOT pin the timeline, then ack.
    ///
    /// `IngestPreVerifiedEvents` routes every event under the
    /// `"diag-firehose-stress"` sub-id, which pushes each id into
    /// `self.timeline` (`preverified_support.rs` — the `diag-firehose-` prefix
    /// branch). Timeline membership PINS the event against both RAM-cache and
    /// durable-LRU eviction, so that path can never produce an eviction-eligible
    /// corpus. This variant takes the sub-id explicitly: any sub-id NOT starting
    /// with `diag-firehose-` skips the `timeline.push_back`, leaving the injected
    /// events un-pinned and therefore evictable — the corpus a GC oracle needs.
    ///
    /// Acked: the actor sends `()` on `ack` only after the whole batch is
    /// ingested AND the timeline re-sorted, so the caller can block until the
    /// ingest is SETTLED before triggering GC and measuring deltas. This is the
    /// deterministic replacement for a fixed `sleep` after a fire-and-forget
    /// inject (which returns before the async actor has processed the batch).
    ///
    /// Test-support only (D0: not part of production FFI surface).
    #[cfg(any(test, feature = "test-support"))]
    IngestPreVerifiedEventsForSubId {
        /// Sub-id passed to `Kernel::ingest_pre_verified_event`. Use a value
        /// that does NOT start with `diag-firehose-` (e.g. `gc-oracle-unpinned`)
        /// to keep the injected events un-pinned and eviction-eligible.
        sub_id: String,
        events: Vec<crate::store::VerifiedEvent>,
        /// One-shot ack channel, sent after the batch is ingested + re-sorted.
        ack: std::sync::mpsc::SyncSender<()>,
    },
    /// Force one immediate GC pass outside the 60-second tick interval, then ack.
    ///
    /// Lets test harnesses exercise GC-budget eviction without waiting 60 s for
    /// the wall-clock gate.  The pass is identical to the idle-tick GC path:
    /// `Kernel::run_gc_step()` → RAM eviction + store LRU step (if a budget
    /// ceiling is configured via `nmp_app_configure_gc_budget`).
    ///
    /// Acked: the actor sends `()` on `ack` only after `run_gc_step()` returns,
    /// so the cumulative eviction counters (`PROCESS_RAM_EVENTS_EVICTED`,
    /// `PROCESS_STORE_LRU_EVICTED`) reflect a SETTLED GC pass before the caller
    /// reads them. This is the deterministic replacement for a fixed `sleep`
    /// after a fire-and-forget trigger.
    ///
    /// Test-support only (D0: not part of production FFI surface).
    #[cfg(any(test, feature = "test-support"))]
    TriggerGcStep {
        /// One-shot ack channel, sent after the GC pass completes.
        ack: std::sync::mpsc::SyncSender<()>,
    },
    /// D6 — surface an error toast from the FFI boundary. Used when the FFI
    /// layer detects a malformed argument (e.g. unparseable JSON) and cannot
    /// call `kernel.set_last_error_toast` directly (the FFI only has a channel
    /// sender, not a kernel reference). The actor thread receives this command
    /// and routes it to `kernel.set_last_error_toast` so the error becomes
    /// observable state, never a silent no-op.
    ShowToast {
        message: String,
    },
    /// D6 + issue #1682 — surface a structured error [`UiToken`] from an
    /// off-actor worker thread (e.g. the NIP-17 gift-wrap publish continuation),
    /// which holds only a `CommandSender`, not a kernel reference. The actor
    /// thread routes it to `kernel.set_last_error_token`, writing both the
    /// machine `code` (`last_error_category`) and the English fallback prose
    /// (`last_error_toast`) so the shell can render localized prose.
    ShowErrorToken {
        token: crate::ui_token::UiToken,
    },
    /// Mark the kernel dirty so host-registered snapshot projections re-emit.
    ///
    /// Used when reusable NMP extension state changes outside a typed kernel
    /// field (for example a registered feed viewport expanding older rows).
    MarkChangedSinceEmit,
    // ADR-0052 §D4 (K2 rung 5.4): `DispatchHostOp { action_json,
    // correlation_id }` was DELETED. Host-op dispatch to the host-installed
    // [`crate::substrate::HostOpHandler`] now flows through the single
    // `Protocol` write seam as `crate::substrate::HostOpCommand`
    // (`ActorCommand::Protocol(Box::new(host_op_command(action_json,
    // correlation_id)))`). The host handler is actor start-time config
    // (set via [`crate::NmpApp::set_host_op_handler`]); the command reaches it
    // through the narrow `HostOpHandlerAccess` capability. Both old-arm
    // guarantees — whole-body `catch_unwind` and a persistent handler shared
    // across one-shot commands — are preserved on the `Protocol` seam.
    /// ADR-0040 §3 — re-entry command from the serialized capability-worker
    /// thread (V-90 Site 2). The worker runs `dispatch_capability` off the
    /// actor thread and posts this command with the result; the actor applies
    /// it inside a normal tick (D4 single-writer invariant).
    ///
    /// The `account_id` field carries the originating account so the dispatch
    /// arm can verify the account still exists before applying — a result for
    /// a since-removed account is dropped with a D6 trace (never
    /// cross-applied to the now-active account). The drop is benign for
    /// writes (persist/forget): a secret that was never stored or is being
    /// cleaned up leaves no observable damage. The handler emits an error
    /// toast only when a write *failed* for a still-present account.
    #[cfg(feature = "native")]
    CapabilityResultReady {
        /// Originating account id (the `account_id` field from the keyring
        /// request). Used solely for the removed-account guard — the handler
        /// never writes any identity state; writing is the actor's job. D6:
        /// an account that has since been removed means the write was
        /// pre-empted by a switch/remove; the result is silently dropped.
        account_id: String,
        /// `CapabilityEnvelope` JSON returned by the native handler. The
        /// dispatch arm decodes it and emits an error toast when `status`
        /// is not `"ok"` and the account is still present.
        result_json: String,
    },
    /// Register a `LogicalInterest` into the subscription registry and trigger
    /// a recompile. Idempotent: same `InterestId` replaces the previous entry.
    ///
    /// Used by protocol crates (e.g. `nmp-marmot`) to register persistent
    /// relay subscriptions (e.g. kind:1059 `#p <pubkey>`) that should remain
    /// live for the session without Swift/Kotlin involvement (D0). The kernel
    /// will emit the appropriate `REQ` frames to connected relays on the next
    /// compile pass; matching inbound events then flow through the raw-event
    /// tap into the host-app service automatically (D4 / event-driven delivery).
    PushInterest(crate::planner::LogicalInterest),
    /// Withdraw a previously registered logical interest by id and trigger a
    /// recompile. Generic lifecycle counterpart to [`PushInterest`].
    WithdrawInterest(crate::planner::InterestId),
    /// Attach one owner to a logical interest using the registry's
    /// `(owner, key, scope)` identity. Multiple owners sharing the same key
    /// keep one live subscription until the last owner is dropped.
    EnsureInterest {
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    },
    /// Detach one owner from a logical interest registered through
    /// [`EnsureInterest`](Self::EnsureInterest).
    DropInterestOwner(crate::subs::SubIdentity),
    /// ADR-0058 §10, step 3a — register (or replace-by-id) a pull cursor in the
    /// non-durable cursor registry. Fire-and-forget. The host mints `cursor_id`
    /// (`0` is invalid). Arms an immediate pull wake when `after_seq` is behind
    /// the store head. A *new* registration past `MAX_PULL_CURSORS` is a loud
    /// no-op (logged, never panics).
    RegisterPullCursor {
        cursor_id: u64,
        consumer_id: String,
        scope: crate::kernel::pull::PullScope,
        mode: crate::kernel::pull_cursor::PullCursorMode,
        after_seq: u64,
        limits: crate::kernel::pull::PullLimits,
    },
    /// ADR-0058 §10, step 3a — monotonically advance a registered cursor's
    /// `after_seq` (`max(old, new)`). Fire-and-forget. Re-arms an immediate
    /// pull wake when the cursor is still behind the store head. Unknown id is
    /// a no-op.
    AdvancePullCursor {
        cursor_id: u64,
        after_seq: u64,
    },
    /// ADR-0058 §10, step 3a — remove a cursor's registry row AND any pending
    /// pull wake entry. Fire-and-forget.
    UnregisterPullCursor {
        cursor_id: u64,
    },
    /// M2 (ADR-0042) — the generic FFI-facing feed-subscription front door that
    /// replaced the bespoke `OpenAuthor` / `OpenThread` / `OpenFirehoseTag`
    /// variants. The host passes a verbatim NIP-01 REQ filter; the dispatch arm
    /// parses it into an `InterestShape` (`InterestShape::from_filter_json`),
    /// builds a `SubIdentity` (`owner = consumer_id`, `key = InterestShape`
    /// hash, scope from the param), and runs the same
    /// `registry_mut().ensure_sub` + `CompileTrigger` body as
    /// [`EnsureInterest`](Self::EnsureInterest). Lifecycle is always `Tailing`.
    ///
    /// D0: `nmp-core` carries the filter as opaque shape data. Declared feed
    /// seams accept app-owned primary kinds; protocol adapters derive wrapper
    /// acquisition outside the substrate. The
    /// `InterestShape` hash gives deterministic dedup: two call sites passing
    /// the same filter (regardless of JSON key/element ordering) map to the same
    /// slot.
    OpenInterest {
        /// Verbatim NIP-01 REQ filter JSON, e.g.
        /// `{"kinds":[1],"#t":["nostr"]}`.
        filter_json: String,
        /// Refcount owner key — deduplicates the live subscription across call
        /// sites that register the same filter.
        consumer_id: String,
        /// `0` = `InterestScope::ActiveAccount` (re-route on account switch),
        /// `1` = `InterestScope::Global` (account-agnostic).
        scope: u32,
    },
    /// M2 (ADR-0042) — detach one owner from an interest registered via
    /// [`OpenInterest`](Self::OpenInterest). Drops the live subscription when
    /// the last owner leaves (mirrors [`DropInterestOwner`](Self::DropInterestOwner)).
    CloseInterest {
        filter_json: String,
        consumer_id: String,
        scope: u32,
    },
    /// Test-support synchronisation primitive (V-105). When the actor dequeues
    /// this command it sends `()` on the `ack` channel, proving all prior
    /// commands have been dispatched. Tests that need to wait for the actor to
    /// reach a known state send this after enqueuing the commands they care
    /// about and then block on the ack receiver — deterministic, no blind
    /// `recv_timeout` polling.
    ///
    /// Only compiled when `any(test, feature = "test-support")` so the variant
    /// never appears in production builds.
    #[cfg(any(test, feature = "test-support"))]
    Barrier {
        /// One-shot ack channel. The actor sends `()` here immediately after
        /// processing this command. The sender is `SyncSender` so it can be
        /// `send`-ed without blocking from any thread (the actor never holds a
        /// borrow on the channel after the `send` call).
        ack: std::sync::mpsc::SyncSender<()>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// V-01 Phase 1c: the actor runtime — per-URL relay handles, the public
// entry points (`run_actor*`), and every loop / dispatch helper below —
// sits on top of the native `relay_worker`. Gated behind `native` so the
// crate compiles without the WebSocket transport. Everything above (the
// `ActorCommand` enum, observer types, `relay_roles`) stays always-compiled.
// ─────────────────────────────────────────────────────────────────────────────

/// One per-URL relay-worker handle. T105: `relay_url` (NOT `role`) is the
/// pool key — every resolved write/read relay gets its own socket. `role`
/// is retained so the actor can route diagnostic-bucket updates back to
/// the kernel's lane-keyed `RelayHealth` rows until per-URL health lands (M11).
///
/// Phase F: `handle` is the generational [`RelayHandle`] handed back by
/// [`Pool::ensure_open_with_role`]; outbound frames go through
/// `pool.send(handle, WireFrame::Text(..))` and shutdown is `pool.close(handle)`.
/// The per-actor `generation` counter is unrelated to `handle.generation()`
/// (the pool's slot generation) — it's a strictly-monotonic stamp the actor
/// uses to drop in-flight events from prior `ensure_open` rounds (the pool's
/// translator already drops events whose slot-generation is stale; the
/// actor-side check is belt-and-braces for the same observable behaviour
/// the pre-Pool design exposed via the `RelayEvent.generation()` field).
#[cfg(feature = "native")]
pub(super) struct RelayControl {
    /// Strictly-monotonic per-actor stamp assigned at `ensure_relay_worker`
    /// time. Phase F: no longer the worker-side generation (the pool owns
    /// that as `handle.generation()`); kept as a diagnostic field for the
    /// FFI surface and tests that still check spawn-order monotonicity.
    #[allow(dead_code)]
    pub(super) generation: u64,
    #[allow(dead_code)] // Diagnostic lane label; per-URL health is M11.
    pub(super) role: RelayRole,
    #[allow(dead_code)] // The URL this worker dials — the routing key in the pool.
    pub(super) relay_url: String,
    pub(super) handle: RelayHandle,
    pub(super) connection_kind: RelayConnectionKind,
    pub(super) idle_since: Option<Instant>,
}

#[cfg(feature = "native")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RelayConnectionKind {
    Persistent,
    Temporary,
}

#[cfg(feature = "native")]
use outbound::wire_frames_to_outbound;

#[cfg(all(feature = "native", any(test, feature = "test-support")))]
pub use compat::run_actor;
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use compat::run_actor_with_lifecycle_observer;

/// T118 / G3 + T146 — actor entry point that accepts BOTH the lifecycle
/// observer slot and the kernel event observer slot. The FFI
/// (`ffi/lifecycle.rs::nmp_app_set_lifecycle_callback`,
/// `ffi/event_observer.rs::nmp_app_register_event_observer`) shares the SAME
/// `Arc<Mutex<…>>` instances so registrations from outside the actor are
/// visible without crossing the FFI on each event.
///
/// Single-inbox priority design (ADR-0050 §D3a): `inbox_rx` carries both
/// commands and relay events as [`ActorMail`]. Each iteration drains the
/// command lane via `try_recv` first (budgeted, stashing any relay mail seen
/// along the way), then makes the loop's single blocking `recv_timeout` — so a
/// command send wakes a relay-blocked actor instead of waiting out the 250 ms
/// idle cap. Command-lane priority and the [`COMMAND_DRAIN_BUDGET`] fairness
/// budget are preserved exactly; relay events still surface at emit-hz cadence
/// when the command lane is not saturated.
#[cfg(feature = "native")]
pub fn run_actor_with_observers(
    channels: ActorChannels,
    config: ActorConfig,
    runtime: ActorRuntimeSlots,
) {
    let ActorChannels {
        inbox_rx,
        command_tx_self,
        update_tx,
    } = channels;
    let ActorRuntimeSlots {
        lifecycle_observer,
        event_observers,
        snapshot_projections,
        bunker_handshake,
        signer_state,
        bunker_hook,
        external_signer_hook,
        configured_relays,
        mls_local_nsec,
        active_local_keys,
        capability_callback,
        queue_depth,
        routing_trace,
        active_account,
        event_store,
        pull_cursor_registry,
        external_event_sink_dispatcher: dispatcher_slot,
    } = runtime;
    // Dual-channel design: relay events get their own dedicated channel.
    // No merged SyncSender<ActorMsg>, no forwarder threads, no drops.
    //
    // Phase F: the channel item is now [`PoolEvent`] (push-model surface from
    // `nmp_network::pool`). The `Pool` is constructed eagerly here — it owns
    // every per-URL worker thread and the worker→pool translator thread that
    // rewrites `RelayEvent` into `PoolEvent`. Default `PoolConfig` (production
    // keepalive constants, `RelayRole::Content` default lane) matches the
    // pre-Pool actor behaviour bit-for-bit; per-URL role attribution still
    // flows through `Pool::ensure_open_with_role` from `ensure_relay_worker`.
    // ADR-0050 §D3a — the pool delivers relay events through a
    // `RelayMailSink` that wraps each `PoolEvent` into `ActorMail::Relay` and
    // pushes it onto the SAME inbox `inbox_rx` receives commands on. There is
    // no longer a separate `relay_rx`: relay traffic and commands share one
    // waking channel, so a command send wakes a relay-blocked actor.
    let inbox = Inbox::new(inbox_rx);
    let pool = Pool::new(PoolConfig::default(), command_tx_self.relay_sink());

    // The lane scheduler (ADR-0050 §D3a). It owns the relay backlog so any
    // relay mail stashed while draining the command lane each iteration is
    // replayed in order.
    let mut scheduler = MailScheduler::new();

    // The actor owns the only live kernel. FFI/app configuration was snapped at
    // `nmp_app_start` into `config`; runtime-observable handles stay in
    // `runtime` so registrations and publish-back slots preserve identity.
    let mut kernel =
        config.kernel_with_account_slot(DEFAULT_VISIBLE_LIMIT, Arc::clone(&active_account));
    if let Ok(mut guard) = routing_trace.lock() {
        *guard = Some(kernel.routing_trace());
    }
    if let Ok(mut guard) = event_store.lock() {
        *guard = Some(kernel.event_store_handle());
    }
    // ADR-0058 step 3b — publish the kernel's pull-cursor registry handle so the
    // synchronous FFI `pull_page` path can snapshot a registration. Re-published
    // on `Reset` (see dispatch.rs) the same way the event-store handle is.
    if let Ok(mut guard) = pull_cursor_registry.lock() {
        *guard = Some(kernel.pull_cursor_registry_handle());
    }
    config.apply_to_kernel(&mut kernel);
    // G-S4 — bind the actor command-channel depth counter so it surfaces on
    // the diagnostic snapshot (`Metrics::actor_queue_depth`). `NmpApp::send_cmd`
    // increments it; this loop decrements per dequeued command (both recv
    // sites below). Survives `Reset` the same way the drop counter does —
    // re-bound there so the counter stays visible across a kernel rebuild.
    kernel.set_queue_depth_handle(Arc::clone(&queue_depth));
    // T146 — bind the shared kernel event observer slot. The kernel calls
    // `notify_event_observers` after every `EventStore::insert` returning
    // `Inserted | Replaced` (see `kernel/ingest/timeline.rs`). Per-app
    // crates (e.g. `nmp-app-chirp`) clone this slot via
    // `NmpApp::register_event_observer` to register typed observers.
    // Survives `Reset` the same way the drop counter does.
    kernel.set_event_observers_handle(Arc::clone(&event_observers));
    // The ExternalEventSinkDispatcher replaces the raw-event-forwarder +
    // pool-send inline path.  The dispatcher owns a bounded channel + worker
    // thread (off the actor thread).  Policies are set via
    // `register_raw_event_forward_policies_from_factory` below and re-installed
    // after every `Reset`.
    //
    // Instance-identity fix: the dispatcher exists from app construction
    // (zero-arg `new()`), so the FFI layer may already have published an
    // instance into `dispatcher_slot` before this actor thread spawned. Adopt
    // that published instance if present so the actor and any FFI handle share
    // one dispatcher. Only if the slot is empty (non-FFI test harnesses) do we
    // create + publish one.
    let external_event_sink_dispatcher = {
        let existing = dispatcher_slot.lock().ok().and_then(|guard| guard.clone());
        match existing {
            Some(d) => d,
            None => {
                let d = crate::substrate::ExternalEventSinkDispatcher::new();
                if let Ok(mut guard) = dispatcher_slot.lock() {
                    *guard = Some(d.clone());
                }
                d
            }
        }
    };
    // Bind the live Pool and spawn the worker thread. Any frames that arrived
    // before this point are retained on the bounded channel and processed as
    // soon as the worker starts.
    external_event_sink_dispatcher.bind_runtime(pool.clone());
    // Bind the dispatcher to the kernel so `persistence.rs` can dispatch
    // frames from the single all-kinds ingest chokepoint.
    kernel.set_external_event_sink_dispatcher(external_event_sink_dispatcher.clone());
    // Raw signed-event forwarding policies are installed through a
    // substrate factory.  The actor contributes only the live kernel
    // handles; target selection and dedup live in the injected policy crate.
    raw_event_forwarder::register_raw_event_forward_policies_from_factory(
        &kernel,
        &external_event_sink_dispatcher,
        config.external_event_sink_policy.clone(),
    );
    // Bind the shared snapshot-projection slot. The kernel runs every
    // host-registered projection closure in `make_update` and appends the
    // result to `KernelSnapshot::projections`. Per-app crates register
    // through the C-ABI `nmp_app_register_snapshot_projection`, which mutates
    // the same `Arc<Mutex<…>>`. Survives `Reset` the same way the other
    // shared handles do so host projections stay live across a kernel
    // rebuild.
    kernel.set_snapshot_projection_handle(Arc::clone(&snapshot_projections));
    // D0 — register the built-in `"bunker_handshake"` snapshot projection.
    // NIP-46 remote signing is an app noun, so handshake state is NOT a typed
    // `KernelSnapshot` field — it is projected under
    // `projections["bunker_handshake"]` exactly like a host-registered
    // namespace. The closure reads the shared bunker-handshake slot the
    // actor's `IdentityRuntime` writes; it runs on every snapshot tick (D8:
    // cheap, non-blocking — a single lock-and-clone). When no handshake is in
    // flight the slot holds `None` and the closure contributes JSON `null`,
    // preserving the "key present, value null when idle" semantic the host
    // sign-in flow decodes. Registered here (the actor wiring site) rather than
    // on the FFI surface so every actor consumer — FFI or test — gets it.
    {
        // Typed sidecar (ADR-0037).
        let typed_slot = Arc::clone(&bunker_handshake);
        if let Ok(mut registry) = snapshot_projections.lock() {
            registry.register_typed("bunker_handshake", move || {
                typed_projections::bunker_handshake_typed(&typed_slot)
            });
        }
    }
    // D0 — second built-in NIP-46 projection: `"nip46_onboarding"`. Where
    // `"bunker_handshake"` carries the raw broker progress (stage string +
    // message), this projection carries the *typed* onboarding read model
    // shells render directly — the static signer-app probe table, the typed
    // `stage_kind`, and pre-computed `is_in_flight` / `is_failed` /
    // `is_terminal_success` / `can_cancel` flags. The closure reads the same
    // shared bunker-handshake slot the previous projection serializes, plus a
    // Rust-owned static signer-app list (no platform-shell ownership of
    // protocol-knowledge tables). Always present (never JSON null) so the host
    // can read `signer_apps` even when no handshake is in flight.
    //
    // ADR-0055 R6-S2: the typed sidecar now uses `TypedProjectionEmissionState`
    // to omit an unchanged `nip46_onboarding` frame when the host has declared
    // incremental-apply capability (exact byte equality, monotonic rev, freeze
    // guard on the same FrameIdentity the feed uses — one implementation, shared).
    {
        // Typed sidecar (ADR-0037).
        let typed_slot = Arc::clone(&bunker_handshake);
        // R6-S2: read the frame-identity and capability handles from the
        // registry (one lock) before the registration lock below.
        let (
            nip46_onboarding_incremental_apply,
            nip46_onboarding_frame_session_id,
            nip46_onboarding_frame_snapshot_epoch,
        ) = if let Ok(reg) = snapshot_projections.lock() {
            let cap = reg.incremental_apply_handle();
            let (sid, epoch) = reg.frame_identity_handles();
            (cap, sid, epoch)
        } else {
            use std::sync::atomic::AtomicU64;
            (
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            )
        };
        let nip46_onboarding_emission_state = Arc::new(Mutex::new(
            crate::projection_emission::TypedProjectionEmissionState::new(
                nip46_onboarding_incremental_apply,
            ),
        ));
        if let Ok(mut registry) = snapshot_projections.lock() {
            let emission_state = Arc::clone(&nip46_onboarding_emission_state);
            let frame_session_id = Arc::clone(&nip46_onboarding_frame_session_id);
            let frame_snapshot_epoch = Arc::clone(&nip46_onboarding_frame_snapshot_epoch);
            registry.register_typed("nip46_onboarding", move || {
                // Build the typed payload (always Some for nip46_onboarding).
                let typed_data = typed_projections::nip46_onboarding_typed(&typed_slot)?;
                // R6-S2: apply byte-equality omit (same mechanism as feed R6-S1).
                let identity = crate::projection_emission::FrameIdentity {
                    session_id: frame_session_id.load(Ordering::Acquire),
                    snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
                };
                let Ok(mut state) = emission_state.lock() else {
                    // Poisoned mutex — degrade to always-emit (D6: safe fallback).
                    return Some(typed_data);
                };
                let emit_decision = state.should_emit(typed_data.payload.clone(), identity);
                drop(state);
                match emit_decision {
                    None => None,
                    Some((payload, projection_rev)) => {
                        Some(crate::update_envelope::TypedProjectionData {
                            payload,
                            projection_rev,
                            ..typed_data
                        })
                    }
                }
            });
        }
    }
    // ADR-0048 D6 — generalised remote-signer health projection: `"signer_state"`.
    // Replaces the NIP-46-only `"bunker_connection_state"` (V-14 step b) with a
    // unified surface keyed by `signer_kind` (`"nip46"` | `"nip55"`). Both
    // signers write into the same slot via `IdentityRuntime::set_signer_state`.
    // `None` (no active remote signer session) → JSON `null`.
    // D0: remote-signer health is an app noun, not a typed `KernelSnapshot` field.
    {
        // Typed sidecar (ADR-0037).
        let typed_slot = Arc::clone(&signer_state);
        if let Ok(mut registry) = snapshot_projections.lock() {
            registry.register_typed("signer_state", move || {
                typed_projections::signer_state_typed(&typed_slot)
            });
        }
    }
    // Bind the shared relay-edit rows handle so external Rust callers
    // (e.g. a per-app dispatch crate) can read the user's current
    // relay list without crossing FFI. Survives `Reset` the same way as
    // the other shared handles.
    kernel.set_app_relay_slot(Arc::clone(&configured_relays));
    // D4: the identity runtime is the sole writer of the shared
    // bunker-handshake slot. The built-in `"bunker_handshake"` snapshot
    // projection registered above reads the same `Arc<Mutex<…>>` clone on
    // every tick. Same for `signer_state` (ADR-0048 D6).
    let mut identity = IdentityRuntime::new(bunker_handshake, signer_state);
    // ADR-0052 §D3 — bind the per-app signer hook slots so the FFI broker /
    // NIP-55 driver install into the SAME slots this runtime reads.
    identity.set_signer_hook_slots(bunker_hook, external_signer_hook);
    // V-38: the wallet runtime moved to `nmp-nip47`. The actor no longer
    // owns it; the substrate relay-text interceptor slot
    // (`relay_text_interceptor`) is the only seam the actor calls for NIP-47
    // NWC behavior.
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    // D1 / offline-first §3 — emit one empty-but-valid snapshot from the real
    // kernel before the actor processes any command. Because the same kernel
    // later handles `Start`, a snapshot-first host sees strict rev monotonicity
    // naturally: the initial `running=false` frame is rev=1 and the first
    // `running=true` Start frame is rev=2.
    emit_now(&mut kernel, running, &update_tx, &mut last_emit);

    // T105: URL-keyed transport pool. One socket per resolved relay URL;
    // workers spawn on demand as OutboundMessages flow with new relay_urls.
    // Keyed by `CanonicalRelayUrl` so the canonicalization invariant is
    // compiler-enforced — a raw `&str` cannot index the pool.
    let mut relay_controls: HashMap<CanonicalRelayUrl, RelayControl> = HashMap::new();
    // Phase F: reverse lookup from a `RelayHandle.slot()` back to the
    // canonical pool key. Inbound `PoolEvent`s carry the handle but not the
    // URL on every variant (`Opened` carries it; `Frame`/`Closed`/`Failed`
    // do not), so we maintain this side-map alongside `relay_controls` so
    // the event dispatcher can resolve `slot → (url, role)` without an
    // O(n) scan. Inserted by `ensure_relay_worker`, removed by
    // `shutdown_relay_worker` / `close_relays`.
    let mut slot_to_url: HashMap<u32, CanonicalRelayUrl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls: HashSet<CanonicalRelayUrl> = HashSet::new(); // T116/G1 reconnect-replay discriminator.
    let mut next_relay_generation = 1;
    // #1069 — wall-clock gate for the bounded GC pass. Initialised to "now" so
    // the first pass fires one `GC_TICK_INTERVAL` after the actor starts, not
    // on the cold-start burst (the store is empty then anyway). An `Instant`
    // (performance-timing) read, never the business clock — D9-clean.
    let mut last_gc = Instant::now();
    let mut startup_sent = false;
    // The single unified parked-op queue (ADR-0050 §D2). `dispatch_command`
    // pushes a `ParkedOp` whenever a remote (NIP-46 / NIP-55) signer goes
    // `Pending` — publish, sign-and-return, the generic sign port, and the
    // cipher port (§D1) all land here and are drained in ONE `retain_mut` below.
    // Lives outside the loop so parked ops survive across ticks.
    let mut parked_ops: Vec<ParkedOp> = Vec::new();
    let mut queued_publish_outbound = Vec::new();
    let mut first_command = None;

    // ADR-0040 §3 — spawn the serialized capability-worker thread (V-90 Site 2).
    // The worker owns the Receiver; the actor holds `capability_work_tx` and
    // hands borrows of it to `ActorContext` on each dispatch. Dropping
    // `capability_work_tx` on actor teardown closes the channel and the worker
    // exits its blocking `recv` loop cleanly (D8).
    let capability_work_tx =
        spawn_capability_worker(Arc::clone(&capability_callback), command_tx_self.clone());

    loop {
        // ── Priority lane: commands ──────────────────────────────────────
        // Drain a bounded burst of pending commands before touching relay
        // events. Commands still get first service on every iteration, but the
        // budget prevents a sustained command stream from starving relay
        // events, subscription ticks, publish retries, and parked sign ops.
        // Single drain (issue #1231 follow-up #3): `MailScheduler::
        // drain_command_lane` is now the *only* implementation of the
        // command-priority + fairness + relay-backlog contract. It replays the
        // command held from the prior blocking wait, drains up to
        // `COMMAND_DRAIN_BUDGET` commands, stashes any relay mail it sees
        // (honoring the #1264 RELAY_BACKLOG_CAP backpressure: once the backlog
        // is full it STOPS pulling relay mail forward, leaving it in the
        // bounded mpsc channel so pressure builds at the pool translator
        // rather than silently dropping the oldest staged event), and returns
        // the commands as a `Vec` so the `&mut kernel` / `&mut identity`
        // per-command dispatch (which a closure boundary cannot express,
        // hence the prior inline copy) runs here, after the drain returns.
        let CommandLaneDrain {
            commands,
            drain: command_drain,
            disconnected: inbox_disconnected,
        } = scheduler.drain_command_lane(&inbox, first_command.take());
        for command in commands {
            {
                {
                    // G-S4 — straddle counter: one command has left the channel
                    // through `drain_command_lane`. Mirror `NmpApp::send_cmd`'s
                    // `fetch_add(1)` so the depth tracks occupancy.
                    // `saturating_sub` guards the (benign) race where the actor
                    // drains a command sent through `actor_sender`, which
                    // bypasses the increment. `Relaxed` — observability, not
                    // synchronization.
                    queue_depth
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                            Some(d.saturating_sub(1))
                        })
                        .ok();
                    // Bundle the actor's mutable runtime state into a borrowed
                    // `ActorContext` for the duration of this one dispatch.
                    // Built fresh per command and dropped immediately after, so
                    // every other call site in this loop keeps using the
                    // original locals untouched (no loop-lifetime borrow).
                    //
                    // Fix A (universal latent-bug fix): `relays_ready` is the
                    // SINGLE claim/open send-gate, computed here once per dispatch
                    // and fed to every consumer (claim_event / claim_profile /
                    // open_author / open_thread / open_firehose /
                    // sign_in_nsec→retarget / session restore). `claim_send_gate`
                    // returns true as soon as ANY bootstrap lane is connected; the
                    // prior `all`-lane gate parked every claim forever when one
                    // lane (e.g. the Indexer) never opened its socket. See
                    // `relay_mgmt::claim_send_gate` for the full rationale and the
                    // proof that hosts connecting all lanes (iOS/TUI) are
                    // behavior-preserved.
                    let relays_ready = claim_send_gate(&connected_relays);
                    let mut ctx = ActorContext {
                        kernel: &mut kernel,
                        identity: &mut identity,
                        relay_controls: &mut relay_controls,
                        slot_to_url: &mut slot_to_url,
                        pool: &pool,
                        connected_relays: &mut connected_relays,
                        connected_urls: &mut connected_urls,
                        update_tx: &update_tx,
                        last_emit: &mut last_emit,
                        next_relay_generation: &mut next_relay_generation,
                        running: &mut running,
                        emit_hz: &mut emit_hz,
                        startup_sent: &mut startup_sent,
                        relays_ready,
                        lifecycle_observer: &lifecycle_observer,
                        mls_local_nsec: &mls_local_nsec,
                        active_local_keys: &active_local_keys,
                        capability_callback: &capability_callback,
                        parked_ops: &mut parked_ops,
                        command_tx_self: &command_tx_self,
                        capability_work_tx: &capability_work_tx,
                        config: &config,
                        routing_trace_slot: &routing_trace,
                        event_store_slot: &event_store,
                        pull_cursor_registry_slot: &pull_cursor_registry,
                        active_account_slot: &active_account,
                        external_event_sink_dispatcher: &external_event_sink_dispatcher,
                    };
                    let outbound = dispatch_command(command, &mut ctx);
                    let Some(outbound) = outbound else {
                        return; // Shutdown
                    };
                    route_dispatch_outbound(
                        running,
                        &mut queued_publish_outbound,
                        &mut relay_controls,
                        &mut slot_to_url,
                        &pool,
                        &mut kernel,
                        &mut next_relay_generation,
                        outbound,
                    );
                    if running
                        && maybe_send_startup(
                            running,
                            &mut startup_sent,
                            &connected_relays,
                            &mut relay_controls,
                            &mut slot_to_url,
                            &pool,
                            &mut kernel,
                            &mut next_relay_generation,
                        )
                    {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
        }
        // Inbox closed (every `CommandSender` clone dropped) → tear down. This
        // is the merged-inbox equivalent of the old `command_rx`
        // `Disconnected` arm: relay traffic alone can never disconnect the
        // inbox (the actor holds the relay sink), so a disconnect means all
        // command senders are gone.
        if inbox_disconnected {
            close_relays(
                &mut relay_controls,
                &mut slot_to_url,
                &pool,
                &mut connected_relays,
                &mut kernel,
            );
            connected_urls.clear();
            return;
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        // This `recv_timeout` is the loop's SINGLE blocking point (D8): a
        // backlog relay event (stashed while draining commands) is served
        // first with zero wait; otherwise we block on the unified inbox, so a
        // command send wakes us here too. A command
        // received during the wait is replayed as `first_command` so the next
        // iteration dispatches it on the priority lane (no added latency).
        //
        // Phase F: the inbound item is `PoolEvent` (push-model). Stale-event
        // filtering moved into `handle_relay_event` itself — the helper
        // resolves `RelayHandle.slot()` → `(url, role)` via the
        // `slot_to_url` side-map and the `relay_controls` entry, dropping
        // any handle whose generation no longer matches the slot's current
        // generation. The pool's translator already drops events with a
        // stale slot-generation, so this is belt-and-braces.
        // Relay events are processed under panic isolation — see
        // `relay_event_guard::process_relay_event`. `handle_relay_event`
        // parses arbitrary network bytes (the highest-risk panic site in the
        // actor); the guard's `catch_unwind` keeps a panic from killing the
        // loop (D1: partial state tolerated, loop survival is the invariant).
        // The same guarded helper serves BOTH the bounded backlog batch and
        // the single recv'd event below (#1264).
        //
        // A small local macro forwards the actor's ~13 loop locals into the
        // helper from both call sites without re-listing them (a closure would
        // have to mutably re-borrow them per batch element).
        macro_rules! process_relay_event {
            ($event:expr) => {
                relay_event_guard::process_relay_event(
                    $event,
                    &mut kernel,
                    &config.relay_text_interceptors,
                    &config.relay_connected_hooks,
                    &command_tx_self,
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut next_relay_generation,
                    &mut connected_relays,
                    &mut connected_urls,
                    &update_tx,
                    &mut last_emit,
                    &mut startup_sent,
                    running,
                )
            };
        }

        // #1264: serve a BOUNDED batch of staged backlog events this iteration
        // (up to RELAY_BACKLOG_DRAIN_BATCH) so the backlog drains faster than a
        // sustained relay flood fills it — then ALWAYS fall through to the
        // single blocking `recv_timeout` below. A non-empty backlog therefore no
        // longer bypasses the one wait per iteration (D8), which kills the
        // busy-spin that previously pinned the CPU under flood.
        for event in scheduler.drain_backlog_batch() {
            process_relay_event!(event);
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        // This `recv_timeout` is the loop's SINGLE blocking point (D8). A
        // command received during the wait is replayed as `first_command` so
        // the next iteration dispatches it on the priority lane (no added
        // latency).
        //
        // #1264: when backlog work remains (the batch did not exhaust it) we
        // pass a ZERO wait so the loop keeps draining promptly — but we STILL
        // call `recv_timeout`, so the single blocking point is reached every
        // iteration (no busy-spin / no D8 violation: a zero-timeout `recv` is
        // the one wait, it simply returns immediately when nothing is queued).
        //
        // Phase F: the inbound item is `PoolEvent` (push-model). Stale-event
        // filtering moved into `handle_relay_event` itself — the helper
        // resolves `RelayHandle.slot()` → `(url, role)` via the `slot_to_url`
        // side-map and the `relay_controls` entry, dropping any handle whose
        // generation no longer matches the slot's current generation. The
        // pool's translator already drops events with a stale slot-generation,
        // so this is belt-and-braces.
        let wait = if scheduler.has_backlog() {
            std::time::Duration::ZERO
        } else {
            command_drain.relay_wait(compute_wait(&kernel, running, last_emit, emit_hz))
        };
        match scheduler.next_after_drain(&inbox, wait) {
            LoopStep::Command(command) => {
                // Woken by a command during the blocking wait — replay it on
                // next iteration's priority lane (zero added latency).
                first_command = Some(command);
            }
            LoopStep::Shutdown => {
                close_relays(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut connected_relays,
                    &mut kernel,
                );
                connected_urls.clear();
                return;
            }
            LoopStep::Idle => {
                // Timeout (normal idle tick) — fall through to idle work.
            }
            LoopStep::Relay(event) => {
                process_relay_event!(event);
            }
        }

        // ── Idle work (runs on every iteration after relay poll) ─────────
        // Flush any time-gated view requests (e.g. contacts_deadline) and
        // run the M2 planner tick only while the actor is running. Before
        // Start these would spawn relay workers (via send_all_outbound) and
        // trigger relay-lifecycle events that emit spurious snapshots on the
        // update channel even though no consumer is listening — the root
        // cause of the S2 retention leak (T114b / s2-retention-audit.md).
        // The publish engine tick below already carries the same running gate
        // for the same reason. Pending profile claims, deferred view
        // requests, and lifecycle triggers all survive in kernel state until
        // Start flushes them through spawn_missing_relays + the first
        // running-gated idle tick.

        // V-64: drive wall-clock-gated sweeps (e.g. NIP-47 pending-payment
        // TTL expiry) even when no relay frame arrives. The interceptor's
        // default `on_idle_tick` is a no-op; the nmp-nip47 impl uses this
        // hook to close expired pay_invoice correlations via
        // `record_action_failure`. No running gate — sweeps must fire even
        // before Start so that entries enqueued during connection setup are
        // not orphaned if the relay never connects.
        {
            for interceptor in &config.relay_text_interceptors {
                let extra = interceptor.on_idle_tick(&mut kernel);
                if !extra.is_empty() {
                    send_all_outbound(
                        &mut relay_controls,
                        &mut slot_to_url,
                        &pool,
                        &mut kernel,
                        &mut next_relay_generation,
                        extra,
                    );
                }
            }
        }

        if running {
            let pending = kernel.pending_view_requests();
            if !pending.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    pending,
                );
            }
        }
        // T142 — M2 planner tick: drain the subscription lifecycle's trigger
        // inbox. Per D8, an empty inbox is a zero-cost no-op (single
        // `is_empty()` check — no allocation, no compile pass). When
        // triggers are queued (e.g. FollowListChanged A11, Nip65Arrived A1)
        // this produces REQ/CLOSE WireFrames that are converted to
        // OutboundMessages and sent to the relay pool. Placed after M1
        // `pending_view_requests()` to ensure M1 CLOSE frames are enqueued
        // before M2 opens new subs (spec §3.1 placement rationale).
        if running {
            let wire_frames = kernel.drain_lifecycle_tick();
            if !wire_frames.is_empty() {
                let outbound = wire_frames_to_outbound(wire_frames, &mut kernel);
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    outbound,
                );
            }
        }
        // W6 — claim-expansion idle tick: advance the per-claim Phase 1/2/3
        // state machine once per actor idle iteration. Per D8, an empty
        // `pending_claims` map is a zero-cost no-op (single `is_empty()` check
        // in `poll_claim_expansion`, no allocation, no iteration). When claims
        // are pending, the state machine applies budget checks and promotes
        // Phase-1 claims to Phase 2 by enqueuing a `CompileTrigger::ViewOpened`
        // via `advance_to_phase2`; the resulting REQ frames surface on the NEXT
        // iteration's `drain_lifecycle_tick` call above. Per D4, this is the
        // sole writer of `pending_claims` — actor single-writer invariant.
        // `poll_claim_expansion` always returns `Vec::new()` today (W5 contract);
        // the `if !msgs.is_empty()` guard is forward-compatible with W7+ where
        // the controller may route fallback REQs as direct OutboundMessages.
        if running {
            let expansion_msgs = kernel.poll_claim_expansion(Instant::now());
            if !expansion_msgs.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    expansion_msgs,
                );
            }
        }
        kernel.flush_relay_scores_if_dirty();
        // T127: actor-tick for the publish engine. The 250ms idle poll
        // in `compute_wait` (`tick.rs`) already paces this; no
        // additional throttle (the engine's own pending_retries gate
        // skips dispatch work when nothing is due). D8 — when
        // `in_flight` is empty the tick is heap-free:
        //   - `PublishEngine::tick` collects `Vec<PublishHandle>`
        //     from an empty iterator (Rust's `FromIterator for Vec`
        //     special-cases empty → `Vec::new()`, no allocation),
        //   - `QueueDispatcher::drain` swaps in `Vec::new()` via
        //     `mem::take` (no allocation when the queue was empty),
        //   - the kernel returns `drained.into_iter().map(..).collect()`
        //     which is also heap-free for an empty source.
        // Closes Residual 1 from T117 — transient retries fire even
        // on a quiet socket (no inbound traffic).
        if running {
            let retry_frames = kernel.tick_publish_engine_for_now();
            if !retry_frames.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    retry_frames,
                );
            }
        }
        if running {
            sweep_temporary_idle_relays(
                &mut relay_controls,
                &mut slot_to_url,
                &mut connected_urls,
                &pool,
                &mut kernel,
                Instant::now(),
                TEMPORARY_RELAY_IDLE_GRACE,
            );
        }
        // #1069 — bounded GC pass on the actor idle tick (audit Finding 1:
        // `gc_step` was never called in production, so on-device store growth,
        // NIP-40 expiry, and LRU eviction were all dead). Mirrors the T127
        // publish tick above: piggy-backs the existing ≤250 ms `compute_wait`
        // loop wake with a wall-clock gate so it fires at most once per
        // `GC_TICK_INTERVAL` (60 s, `gc.md` §3) — no new sleep loop, no timer
        // thread (D8 / "no polling"). When the gate has not elapsed this is a
        // single `Instant::elapsed()` compare — heap-free, no false wakeups.
        //
        // `Kernel::run_gc_step` derives `now_secs` from the injected kernel
        // clock (D7/D9 — deterministic under replay/`FixedClock`); the store's
        // own `gc.rs` budget loops bound the worst-case latency to ~50 ms so the
        // mailbox is never blocked (`gc.md` §3, §8).
        if running && last_gc.elapsed() >= GC_TICK_INTERVAL {
            kernel.run_gc_step();
            last_gc = Instant::now();
        }
        // ADR-0045 §5 — chunked continuation for store-cache serves. Drains
        // ONE aggregate per-tick budget chunk (`cache_serve_tick_budget`,
        // 2× the visible window) across ALL pending serves, resuming
        // partially-completed interests via their per-query cursor. Like the
        // gc tick above this piggybacks the existing ≤250 ms `compute_wait`
        // wake — no new sleep loop, no timer thread (D8 / "no polling").
        // An empty queue costs one bool check. Runs BEFORE the `flush_due`
        // emit below, so served events land in this tick's snapshot (D1).
        if running && (kernel.has_pending_cache_serves() || kernel.has_cache_serve_wakeups()) {
            kernel.run_cache_serve_step();
        }
        // ── V-06 / #960: drain kernel-emitted NIP-42 AUTH signs ──────────
        // `handle_message` enqueues an AUTH kind:22242 for any relay lane whose
        // active account is a REMOTE signer; route each through the async signer
        // port (park under the `Auth` sink) — see `auth_sign::drain_pending_auth_signs`.
        auth_sign::drain_pending_auth_signs(
            &mut kernel,
            &identity,
            &mut parked_ops,
            &mut auth_sign::RouteCtx {
                running,
                queued_publish_outbound: &mut queued_publish_outbound,
                relay_controls: &mut relay_controls,
                slot_to_url: &mut slot_to_url,
                pool: &pool,
                next_relay_generation: &mut next_relay_generation,
            },
        );
        // ── Poll the unified parked-op queue (ADR-0050 §D2) ──────────────
        // ONE `retain_mut` over ONE `Vec<ParkedOp>` replaces the two former
        // drains (the inline publish block + `resolve_pending_sign_return`). Each
        // op is polled once per tick (D8 — `SignerOp::poll` is non-blocking; the
        // deadline is the wall-clock gate). Projection / continuation sinks
        // resolve against the kernel in `resolve_parked_op`; the `Publish` sink
        // hands back a `PublishObligation` (the loop owns relay routing).
        // Obligations are collected during the retain and run after it so the
        // drain's `&mut kernel` borrow never overlaps `route_dispatch_outbound`.
        // Empty `parked_ops` is a heap-free zero-item retain.
        if !parked_ops.is_empty() {
            let mut publish_obligations: Vec<PublishObligation> = Vec::new();
            let mut auth_obligations: Vec<AuthObligation> = Vec::new();
            let mut any_changed = false;
            parked_ops.retain_mut(|parked| {
                let outcome = resolve_parked_op(parked, &mut kernel);
                if let Some(obligation) = outcome.publish {
                    publish_obligations.push(obligation);
                }
                if let Some(obligation) = outcome.auth {
                    auth_obligations.push(obligation);
                }
                any_changed |= outcome.changed;
                outcome.keep
            });
            // V-06 / #960: execute the NIP-42 AUTH obligations the `Auth` sink
            // handed back (re-enter `dispatch_signed_auth` / `fail_auth_sign` and
            // route outbound) — see `auth_sign::run_auth_obligations`. Runs here
            // after the retain so the drain's `&mut kernel` borrow has ended.
            auth_sign::run_auth_obligations(
                &mut kernel,
                auth_obligations,
                &mut auth_sign::RouteCtx {
                    running,
                    queued_publish_outbound: &mut queued_publish_outbound,
                    relay_controls: &mut relay_controls,
                    slot_to_url: &mut slot_to_url,
                    pool: &pool,
                    next_relay_generation: &mut next_relay_generation,
                },
            );
            // Execute the publish obligations the `Publish` sink handed back,
            // preserving ALL prior terminal behaviours exactly: a resolved sign
            // routes via the parked `target` + `correlation_id_override`; a
            // failure / timeout surfaces the toast and (for a dispatched action)
            // records the `"failed"` verdict so the host spinner clears (D6).
            for obligation in publish_obligations {
                match obligation {
                    PublishObligation::Publish {
                        signed,
                        p_tags,
                        target,
                        correlation_id_override,
                    } => {
                        let outbound = kernel.publish_signed_to_with_correlation(
                            &signed,
                            &p_tags,
                            target,
                            correlation_id_override,
                        );
                        route_dispatch_outbound(
                            running,
                            &mut queued_publish_outbound,
                            &mut relay_controls,
                            &mut slot_to_url,
                            &pool,
                            &mut kernel,
                            &mut next_relay_generation,
                            outbound,
                        );
                    }
                    PublishObligation::Failed {
                        toast,
                        correlation_id_override,
                    } => {
                        kernel.set_last_error_toast(Some(toast.clone()));
                        // Recorded BEFORE `emit_now` (below) so this tick's
                        // snapshot drains it; `None` (a `react` / `follow` park)
                        // is a no-op — nothing is waiting on an id.
                        if let Some(id) = correlation_id_override {
                            kernel.record_action_failure(id, toast);
                        }
                    }
                }
            }
            // Surface the changes immediately rather than waiting up to one
            // periodic flush tick — matches the prior per-op `emit_now`.
            if any_changed && running {
                emit_now(&mut kernel, running, &update_tx, &mut last_emit);
            }
        }
        // Only emit when state actually changed; do not emit on every
        // idle tick (D8: zero false-wakeup allocations after warmup).
        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}
