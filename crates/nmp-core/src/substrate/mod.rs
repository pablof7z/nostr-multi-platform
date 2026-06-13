//! Substrate — the per-protocol extension contracts (`ActionModule`,
//! `CapabilityModule`).
//!
//! # Extension mechanism: v1 vs v2
//!
//! The traits in this module are the **v2** extension design — a family of
//! typed, namespace-keyed modules the kernel would discover and drive
//! through a dispatch runtime. That runtime does not exist yet. The per-NIP
//! crates implement these traits and tests invoke their methods directly
//! (static dispatch — `<PublishModule as ActionModule>::plan(...)`), so the
//! trait *contracts* are real and load-bearing. What never shipped is a
//! kernel-side registry that stores `dyn Trait` objects and fans events to
//! them.
//!
//! A previous iteration shipped a `ModuleRegistry` that *looked* like that
//! runtime but only collected `(namespace, family, type_name)` strings —
//! nothing in the kernel, the actor, or codegen ever read them back. It
//! has been removed; it was documentation theater that misled readers
//! about how extension actually works today.
//!
//! Two further v2 traits — `ViewModule` and `IdentityModule` — were removed
//! for the same reason: no `ViewRegistry` or identity-dispatch runtime ever
//! shipped. The per-protocol view types still exist as plain types whose
//! `open` / `on_event_*` / `snapshot` inherent methods are reached via
//! static dispatch; `ViewDependencies` survives as the planner bridge.
//!
//! ## v1 extension mechanism: `KernelEventObserver`
//!
//! The mechanism the kernel *actually* drives in v1 is
//! [`KernelEventObserver`](crate::KernelEventObserver) — a flat raw-event
//! fan-out. Per-app crates register `Arc<dyn KernelEventObserver>`
//! observers; the kernel fans every accepted event (`Inserted | Replaced`)
//! to all registered observers. The modular timeline projection and the
//! MLS group-messaging projection are the canonical live consumers.
//!
//! Canonical pattern:
//! - the slot + registration helpers: `actor/commands/event_observer.rs`
//! - the kernel fan-out integration: `kernel/event_observer.rs`
//! - a per-app crate registering an observer: `nmp-app-chirp/src/ffi.rs`

mod action;
mod app_host;
mod blocked_relays;
mod bounded;
mod suppression;
mod capability;
mod dm_inbox_relays;
mod empty_routing;
mod host_op;
mod host_op_handler;
mod identity;
mod ingest;
mod keyring;
pub mod placeholder;
mod protocol;
mod raw_event_forwarding;
mod relay_connected;
mod relay_info;
mod relay_intercept;
mod relay_score_store;
mod req_intercept;
mod routing;
mod routing_trace;
mod view;

pub use action::{
    ActionContext, ActionId, ActionModule, ActionRegistrar, ActionRejection, ActionResult,
};
pub use app_host::AppHost;
pub use blocked_relays::{empty_blocked_relay_lookup, BlockedRelayLookup, EmptyBlockedRelayLookup};
pub use suppression::{empty_suppression_lookup, EmptySuppressionLookup, SuppressionLookup};
pub use bounded::{BoundedMessageMap, BoundedRing, MAX_PROJECTION_MESSAGES};
pub use capability::{CapabilityEnvelope, CapabilityModule, CapabilityRequest};
#[cfg(any(test, feature = "test-support"))]
pub use dm_inbox_relays::TestDmInboxRelayCache;
pub use dm_inbox_relays::{
    empty_dm_inbox_relay_lookup, DmInboxRelayLookup, EmptyDmInboxRelayLookup,
};
pub use host_op_handler::{new_host_op_handler_slot, HostOpHandler, HostOpHandlerSlot};
// Step 9: the `DomainMigration` / `MigrationTx` value types passed to
// `EventStore::run_migrations` moved with the store (they are consumed only by
// that seam, and keeping them in `nmp-store` lets the store crate compile
// without a back-edge into substrate). Re-exported here so the legacy
// `nmp_core::substrate::{DomainMigration, MigrationTx}` import path is
// unchanged.
pub use identity::{SignedEvent, SigningError, UnsignedEvent};
/// V-78 — NIP crates need to name `SignerOp` to `op.wait()` a parked
/// remote (NIP-46 bunker) sign on an off-actor worker thread (the
/// `nmp-nip57` zap path). Re-exported through the substrate so NIP crates
/// reach it via `nmp_core::substrate::SignerOp` rather than adding a direct
/// `nmp-signer-iface` dependency — every signer surface a NIP crate touches
/// stays funnelled through `nmp_core::substrate`.
/// [`SignerError`] rides along because `SignerOp::Pending` carries a
/// `Receiver<Result<T, SignerError>>`, so any crate constructing or matching
/// on a pending op needs the error name too.
pub use nmp_signer_iface::{SignerError, SignerOp};
pub use ingest::{EventIngestDispatcher, IngestParser};
pub use keyring::{
    KeyringCapability, KeyringIdentityWiring, KeyringRequest, KeyringResult, KeyringStatus,
    MALFORMED_RESULT,
};
pub use nmp_store::{DomainMigration, MigrationTx};
pub use placeholder::{picture_placeholder, Placeholder};
pub use host_op::{host_op_command, HostOpCommand};
pub use protocol::{
    build_nip44_decrypt_for_account, build_nip44_encrypt_for_account, build_sign_event_for_account,
    ActionStageTracker, DmInboxLookup, ErrorSurface, HostOpHandlerAccess, KernelClock,
    LocalSignerAccess, NoopActionStageTracker, NoopErrorSurface, NoopHostOpHandlerAccess,
    NoopKernelClock, NoopLocalSignerAccess, NoopRecipientRelayLookup, NoopWalletKernelAccess,
    NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts,
    ProtocolCommandError, RecipientRelayLookup, WalletKernelAccess, ZapProfileLookup,
};
pub use raw_event_forwarding::{
    RawEventForwardPolicy, RawEventForwardPolicyContext, RawEventForwardTarget,
};
pub use relay_connected::{
    fan_relay_connected, install_relay_connected_hook, new_relay_connected_hook_slot,
    RelayConnectedHook, RelayConnectedHookSlot,
};
pub use relay_info::RelayInfoDoc;
pub use relay_intercept::{
    new_relay_text_interceptor_slot, RelayTextInterceptor, RelayTextInterceptorSlot,
};
pub use req_intercept::{
    new_req_frame_interceptor_slot, ReqFrameContext, ReqFrameInterceptor, ReqFrameInterceptorSlot,
};
#[cfg(any(test, feature = "test-support"))]
pub use empty_routing::TestInMemoryMailboxCache;
pub use empty_routing::{EmptyMailboxCache, EmptyOutboxRouter};
#[cfg(feature = "lmdb-backend")]
pub use relay_score_store::LmdbRelayAuthorScoreStore;
pub use relay_score_store::{NoopRelayAuthorScoreStore, RelayAuthorScoreStore, ScoreCell};
pub use routing::{
    AppRelayMode, BlockedRelaySet, ClassRoutingPath, Direction, EventClass, MailboxCache,
    OutboxRouter, ParsedRelayList, Pubkey as RoutingPubkey, RelayUrl as RoutingRelayUrl,
    RoutedRelaySet, RoutingContext, RoutingError, RoutingSource, SessionKeySet,
    UserConfiguredCategory,
};
pub use routing_trace::{
    truncate_event_id, LaneOutcome, PublishTrace, RouteAttempt, RoutingLane,
    RoutingTraceObserver, SubscriptionTrace,
};
pub use view::{EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies};

// NIP-10 / tag codec lives in `crate::tags` (a protocol codec, like nip19 /
// nip21 — not a per-kind decoder, so D0-clean). Re-exported here so the
// per-NIP relation crates that already `use nmp_core::substrate::{...}`
// consume one source.
pub use crate::tags::{
    a_tag, all_tag_values, e_tag, first_tag_value, p_tag, parse_nip10, q_tag, EventRef, Nip10Refs,
};
