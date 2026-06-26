//! Inner mutable state held by `BrowserAppBuilder<S>`.
//!
//! Separated from `builder.rs` to keep both files under the 500-LOC ceiling
//! (AGENTS.md file-size rule). The `BrowserBuilderInner` struct accumulates
//! every pre-start registration/configuration call; `start()` consumes it and
//! applies all deferred settings to the `KernelReducer` before handing off to
//! `BrowserRuntime`.
//!
//! # Interior mutability model
//!
//! `BrowserAppBuilder<S>` wraps `Mutex<BrowserBuilderInner>` so that all
//! registrar traits (which take `&self`) can mutate builder state without
//! `&mut self`. On wasm32 the Mutex is always uncontested (single-threaded);
//! on native test builds it provides `Send + Sync`. The `ActionRegistrar`
//! trait (which takes `&mut self`) unlocks the same Mutex.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorMail;
use nmp_core::subs::PlanCoverageHook;
use nmp_core::substrate::InputScopeRegistry;
use nmp_core::substrate::PreferredRelaySource;
use nmp_core::substrate::SearchScopeRegistry;
use nmp_core::substrate::{
    BlockedRelayLookup, ContactsLookup, DmInboxRelayLookup, ExternalEventSinkPolicy, MailboxCache,
    ObservedProjectionSessionMap, OutboxRouter, ProfileLookup, RawEventForwardPolicyContext,
    RelayConnectedHook, RelayTextInterceptor, ReqFrameInterceptor, RoutingTraceObserver,
};
use nmp_core::{
    publish::OutboxResolver,
    slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot},
    ActionRegistry, AppRelaySlot, Clock, KernelReducer,
};

// Type aliases matching AppHost factory shapes.
type RoutingSubstrateFactory = Box<
    dyn Fn(Arc<dyn RoutingTraceObserver>) -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)
        + Send
        + Sync,
>;
type PublishResolverFactory = Box<
    dyn Fn(
            Arc<dyn nmp_store::EventStore>,
            IndexerRelaysSlot,
            LocalWriteRelaysSlot,
            ActiveAccountSlot,
        ) -> Arc<dyn OutboxResolver>
        + Send
        + Sync,
>;
type ExternalEventSinkPolicyFactory = Box<
    dyn Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn ExternalEventSinkPolicy>> + Send + Sync,
>;

/// Everything the builder accumulates before `start()`.
///
/// `&mut` settings (applied to the kernel in `start()`) are stored here until
/// `start()` can consume them under an exclusive borrow of the reducer.
/// `&self` settings go directly to the reducer's Arc<Mutex<...>> slots (which
/// have their own interior mutability).
pub(crate) struct BrowserBuilderInner {
    // ── Kernel (owned until start()) ──────────────────────────────────────────
    pub(crate) reducer: KernelReducer,

    // ── ActionRegistry (extends ActionRegistrar, mut) ─────────────────────────
    pub(crate) action_registry: ActionRegistry,

    // ── HostCapabilities ──────────────────────────────────────────────────────
    /// Sender half of the builder-owned mailbox. Cloned into `CommandSender`s.
    pub(crate) inbox_tx: std::sync::mpsc::Sender<ActorMail>,
    /// Receiver half — moved into `BrowserRuntime` at start().
    pub(crate) inbox_rx: std::sync::mpsc::Receiver<ActorMail>,
    /// Shared relay-list slot (builder holds it, kernel gets a clone at start).
    pub(crate) configured_relays_slot: AppRelaySlot,
    /// Preferred-relay source for NIP-50 search relay resolution. Stored here;
    /// consumed when the browser capability/host surface that resolves preferred
    /// relays lands — seam: browser capability registry (#2049).
    #[allow(dead_code)]
    pub(crate) preferred_relay_source: Option<Arc<dyn PreferredRelaySource>>,

    // ── Registries (held as Arc for install_into at start()) ─────────────────
    pub(crate) search_scope_registry: Arc<SearchScopeRegistry>,
    pub(crate) input_scope_registry: Arc<InputScopeRegistry>,
    pub(crate) observed_projection_sessions: ObservedProjectionSessionMap,

    // ── Deferred &mut-kernel settings (applied in start()) ───────────────────
    pub(crate) coverage_hook: Option<PlanCoverageHook>,
    pub(crate) req_frame_interceptor: Option<Arc<dyn ReqFrameInterceptor>>,
    pub(crate) profile_lookup: Option<Arc<dyn ProfileLookup>>,
    pub(crate) contacts_lookup: Option<Arc<dyn ContactsLookup>>,
    pub(crate) dm_inbox_relay_lookup: Option<Arc<dyn DmInboxRelayLookup>>,
    pub(crate) blocked_relay_lookup: Option<Arc<dyn BlockedRelayLookup>>,
    /// Read-only `MailboxCache` for the NIP-19 `nprofile` encoder. Stored here;
    /// consumed when the browser snapshot/projection/encoding contract wires the
    /// identity encoder — seam: #2051.
    #[allow(dead_code)]
    pub(crate) mailbox_cache_reader: Option<Arc<dyn MailboxCache>>,
    pub(crate) routing_substrate_factory: Option<RoutingSubstrateFactory>,
    pub(crate) publish_resolver_factory: Option<PublishResolverFactory>,
    /// Raw-event forward (external sink) policy factory. Stored here; consumed by
    /// the browser relay transport's outbound-forward path — seam: bounded
    /// transport-only relay adapter (#2050).
    #[allow(dead_code)]
    pub(crate) external_event_sink_policy_factory: Option<ExternalEventSinkPolicyFactory>,
    pub(crate) outbound_public_tags: Vec<Vec<String>>,
    /// NIP-46 `nostrconnect://` bootstrap relay URL. Stored here; consumed by the
    /// browser NIP-46 signer provider — seam: signer-provider registry (#2049).
    #[allow(dead_code)]
    pub(crate) nostrconnect_bootstrap_relay: Option<String>,
    /// NIP-46 requested permissions. Stored here; consumed by the browser NIP-46
    /// signer provider — seam: signer-provider registry (#2049).
    #[allow(dead_code)]
    pub(crate) nostrconnect_perms: Option<String>,
    /// Relay-handshake User-Agent. Stored here; consumed by the browser relay
    /// driver when it opens sockets — seam: relay transport adapter (#2050).
    #[allow(dead_code)]
    pub(crate) relay_user_agent: Option<String>,

    // ── Collections handed to BrowserRuntime at start() ───────────────────────
    pub(crate) relay_text_interceptors: Vec<Arc<dyn RelayTextInterceptor>>,
    pub(crate) relay_connected_hooks: Vec<Arc<dyn RelayConnectedHook>>,
    pub(crate) identity_change_observers: Vec<Box<dyn Fn(Option<String>) + Send + Sync + 'static>>,
    /// Capability/signer providers accumulated via
    /// `BrowserAppBuilder::with_capability_providers`. Moved into the
    /// `CapabilityProviderRegistry` in `from_builder_inner` at `start()`.
    pub(crate) capability_providers: Vec<Arc<dyn nmp_signers::Signer>>,

    // ── Gate-specific fields set by typestate-advancing builder methods ────────
    /// Relay bootstrap list set at `set_relays()` gate; applied at `start()`.
    pub(crate) relay_bootstrap: Vec<(String, String)>,
    /// Run config set at `decide_providers()` gate.
    pub(crate) run_config: Option<crate::builder::BrowserRunConfig>,

    // ── #2076 — clock injection ───────────────────────────────────────────────
    /// Injectable kernel clock. `None` = use the default web-time wall-clock.
    /// Set via `.with_clock(arc)` or `.with_system_clock()`. Applied at
    /// `start()` via `KernelReducer::set_clock`. `Send + Sync` so the builder
    /// itself stays `Send + Sync` (same constraint as the rest of the inner).
    pub(crate) clock: Option<Arc<dyn Clock>>,
}

impl BrowserBuilderInner {
    /// Construct a fresh inner state with an empty `KernelReducer` and wired
    /// mailbox channel + relay slot.
    pub(crate) fn new() -> Self {
        let (inbox_tx, inbox_rx) = std::sync::mpsc::channel::<ActorMail>();
        let configured_relays_slot: AppRelaySlot =
            Arc::new(Mutex::new(nmp_core::AppRelayList::default()));
        Self {
            reducer: KernelReducer::new(),
            action_registry: nmp_core::default_registry(),
            inbox_tx,
            inbox_rx,
            configured_relays_slot,
            preferred_relay_source: None,
            search_scope_registry: Arc::new(SearchScopeRegistry::new()),
            input_scope_registry: Arc::new(InputScopeRegistry::new()),
            observed_projection_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            coverage_hook: None,
            req_frame_interceptor: None,
            profile_lookup: None,
            contacts_lookup: None,
            dm_inbox_relay_lookup: None,
            blocked_relay_lookup: None,
            mailbox_cache_reader: None,
            routing_substrate_factory: None,
            publish_resolver_factory: None,
            external_event_sink_policy_factory: None,
            outbound_public_tags: Vec::new(),
            nostrconnect_bootstrap_relay: None,
            nostrconnect_perms: None,
            relay_user_agent: None,
            relay_text_interceptors: Vec::new(),
            relay_connected_hooks: Vec::new(),
            identity_change_observers: Vec::new(),
            capability_providers: Vec::new(),
            relay_bootstrap: Vec::new(),
            run_config: None,
            clock: None,
        }
    }
}
