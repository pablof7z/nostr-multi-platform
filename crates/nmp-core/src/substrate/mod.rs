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
//! ## v1 read-model mechanism: declared observed projections
//!
//! The mechanism the kernel drives in v1 is a declared observed projection:
//! a host supplies an [`ObservedProjection`](app_host::ObservedProjection)
//! containing the sink, owner, scope, relay pin, replay shapes, and replay
//! limit before the sink can receive events. The kernel replays matching
//! cached/store rows into that muted sink, then activates scoped future
//! delivery for the declared shapes.
//!
//! This deliberately replaced the former public filterless accepted-event
//! observer. Product read models must not subscribe to every accepted event
//! and self-filter later.
//!
//! Canonical pattern:
//! - the slot + registration helpers: `actor/commands/event_observer.rs`
//! - the kernel fan-out integration: `kernel/event_observer.rs`
//! - a host registering `ObservedProjection` through `ObservedProjectionRegistrar`

mod action;
mod action_context;
pub mod active_observed_projection;
mod app_host;
mod blocked_relays;
mod bounded;
mod capability;
pub mod content_parser;
mod dm_inbox_relays;
mod empty_routing;
pub mod external_event_sink;
mod host_op;
mod host_op_handler;
mod identity;
mod ingest;
mod keyring;
mod payment;
pub mod placeholder;
mod profile_lookup;
mod protocol;
mod raw_event_forwarding;
mod relay_connected;
mod relay_info;
mod relay_intercept;
mod relay_score_store;
mod req_intercept;
mod routing;
mod routing_trace;
mod suppression;
// #1811 — crate-registered full-text search scopes (protocol-aware
// SearchIndexSpec + SearchScopeProvider; compiled into nmp-store's noun-free
// CompiledIndexSpec at composition time).
pub mod search;
// #1804 — input-intent recognizer substrate (noun-free InputScopeRecognizer +
// InputScopeRegistry; orchestrator + generic parsers live in the nmp-intent crate).
pub mod intent;
mod view;
pub(crate) use view::{observed_shape_matches_event, observed_shape_matches_fields};

pub use action::ProtocolDescriptor;
pub use action::{
    ActionId, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ActionResult, RegistrationError,
};
pub use action_context::{
    ActionContext, ActionLocalStore, ActionReadError, ACTION_LOCAL_STORE_MAX_EVENTS,
};
pub use app_host::{
    AppHost, BlockedRelayLookupRegistrar, ConfiguredRelaysChangeRegistrar, CoverageHookRegistrar,
    DmInboxRelayRegistrar, HostCapabilities, IdentityChangeRegistrar, IncrementalApplyError,
    IngestParserRegistrar, KernelReaderRegistrar, ObservedProjection,
    ObservedProjectionCommandHandle, ObservedProjectionRegistrar, ObservedProjectionSessionMap,
    PreferredRelaySource, RelayConnectedHookRegistrar, RelayTextInterceptorRegistrar,
    ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar, SnapshotProjectionRegistrar,
};
pub use blocked_relays::{empty_blocked_relay_lookup, BlockedRelayLookup, EmptyBlockedRelayLookup};
// #1811 — FTS scope registry surface.
pub use search::{
    CacheSearchMode, SearchIndexSpec, SearchPrivacyPolicy, SearchScopeDisposition,
    SearchScopeProvider, SearchScopeRegistrar, SearchScopeRegistry,
};
// #1804 — input-intent recognizer substrate surface.
pub use bounded::{BoundedMessageMap, BoundedRing, MAX_PROJECTION_MESSAGES};
pub use capability::{CapabilityEnvelope, CapabilityModule, CapabilityRequest};
#[cfg(any(test, feature = "test-support"))]
pub use dm_inbox_relays::TestDmInboxRelayCache;
pub use dm_inbox_relays::{
    empty_dm_inbox_relay_lookup, DmInboxRelayLookup, EmptyDmInboxRelayLookup,
};
pub use intent::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentRequest,
    InputIntentTarget, InputScopeDisposition, InputScopeId, InputScopeRecognizer,
    InputScopeRegistrar, InputScopeRegistry, ResolvedInput, ResolvedInputKind, TextSearchTargets,
    INPUT_SCOPE_LEDGER_SEAM,
};
pub use payment::{PaymentIntent, PaymentPort};
pub use profile_lookup::{empty_profile_lookup, EmptyProfileLookup, ProfileLookup, ProfileView};
#[cfg(any(test, feature = "test-support"))]
pub use profile_lookup::{TestKind0Parser, TestProfileCache};
pub use suppression::{empty_suppression_lookup, EmptySuppressionLookup, SuppressionLookup};

pub use content_parser::{ContentParser, NoopContentParser};
#[cfg(any(test, feature = "test-support"))]
pub use empty_routing::TestInMemoryMailboxCache;
pub use empty_routing::{EmptyMailboxCache, EmptyOutboxRouter};
pub use external_event_sink::{
    dispatcher::{
        new_external_event_sink_dispatcher_slot, ExternalEventSinkDispatcher,
        ExternalEventSinkDispatcherSlot,
    },
    ExternalEventSinkPolicy, IngestOutcomeKind, SignedEventFrame, SinkDestination,
};
pub use host_op::{host_op_command, HostOpCommand};
pub use host_op_handler::{new_host_op_handler_slot, HostOpHandler, HostOpHandlerSlot};
pub use ingest::{EventIngestDispatcher, IngestParser};
pub use keyring::{
    KeyringCapability, KeyringIdentityWiring, KeyringRequest, KeyringResult, KeyringStatus,
    MALFORMED_RESULT,
};
pub use nmp_store::{DomainMigration, MigrationTx};
pub use placeholder::{picture_placeholder, Placeholder};
pub use protocol::{
    build_nip44_decrypt_for_account, build_nip44_encrypt_for_account, build_record_action_failure,
    build_record_action_success, build_sign_event_for_account, ActionStageTracker, ErrorSurface,
    HostOpHandlerAccess, KernelClock, LocalSignerAccess, NoopActionStageTracker, NoopErrorSurface,
    NoopHostOpHandlerAccess, NoopKernelClock, NoopLocalSignerAccess, NoopRecipientRelayLookup,
    NoopWalletKernelAccess, NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext,
    ProtocolCommandContextParts, ProtocolCommandError, RecipientRelayLookup, WalletKernelAccess,
    ZapProfileLookup,
};
pub use raw_event_forwarding::{RawEventForwardPolicyContext, RawEventForwardTarget};
pub use relay_connected::{
    fan_relay_connected, fan_relay_connected_hooks, install_relay_connected_hook,
    new_relay_connected_hook_slot, RelayConnectedHook, RelayConnectedHookSlot,
};
pub use relay_info::RelayInfoDoc;
pub use relay_intercept::{
    new_relay_text_interceptor_slot, RelayTextInterceptor, RelayTextInterceptorSlot,
};
#[cfg(feature = "lmdb-backend")]
pub use relay_score_store::LmdbRelayAuthorScoreStore;
pub use relay_score_store::{NoopRelayAuthorScoreStore, RelayAuthorScoreStore, ScoreCell};
pub use req_intercept::{
    new_req_frame_interceptor_slot, ReqFrameContext, ReqFrameInterceptor, ReqFrameInterceptorSlot,
};
pub use routing::{
    canonicalize_relay_url, AppRelayMode, BlockedRelaySet, ClassRoutingPath, Direction, EventClass,
    MailboxCache, OutboxRouter, ParsedRelayList, Pubkey as RoutingPubkey,
    RelayUrl as RoutingRelayUrl, RoutedRelaySet, RoutingContext, RoutingError, RoutingSource,
    SessionKeySet, UserConfiguredCategory,
};
pub use routing_trace::{
    truncate_event_id, LaneOutcome, PublishTrace, RouteAttempt, RoutingLane, RoutingTraceObserver,
    SubscriptionTrace,
};
pub use view::{EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies};

pub use active_observed_projection::ObservedProjectionReconciler;

// NIP-10 / tag codec lives in `crate::tags` (a protocol codec, like nip19 /
// nip21 — not a per-kind decoder, so D0-clean). Re-exported here so the
// per-NIP relation crates that already `use nmp_core::substrate::{...}`
// consume one source.
pub use crate::tags::{
    a_tag, all_tag_values, e_tag, first_tag_value, p_tag, parse_nip10, q_tag, EventRef, Nip10Refs,
};
