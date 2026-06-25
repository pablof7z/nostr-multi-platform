//! `nmp-defaults` — the framework's **default composition** for an NMP-based
//! Nostr app (the Bevy-`DefaultPlugins` / Spring-Boot-starter pattern).
//!
//! Per **ADR-0046** ("composition is a library, not a generator") this crate is
//! NOT a template and NOT a scaffold to copy — it is a runtime library you
//! depend on and call. Step 10 of `docs/architecture/crate-boundaries.md` §5.
//! Closes **V-48**: "No composition-root crate — second-app developer must read
//! 403 LOC of Chirp to understand registration".
//!
//! (Renamed from `nmp-app-template` by ADR-0046; the old name lied — a
//! "template" implies copy-and-edit, but every real consumer *calls*
//! `register_defaults` as a library, exactly as Chirp and the external
//! podcast-player do.)
//!
//! # What this crate is
//!
//! A single function — [`register_defaults`] — that, given a freshly
//! constructed host implementing [`AppHost`], wires every registration a
//! generic Nostr app needs to participate in the standard NMP composition:
//!
//! 1. **Action modules** for the common NIPs:
//!    * `nmp.follow` / `nmp.unfollow` — [`nmp_nip02`]
//!    * `nmp.nip25.react` / `nmp.nip25.unreact` — [`nmp_nip25`]
//!    * `nmp.nip17.send` / `nmp.nip17.publish_relay_list` — [`nmp_nip17`]
//!    * `nmp.nip57.zap` — [`nmp_nip57`]
//!    * `nmp.nip65.publish_relay_list` — [`nmp_router`]
//!    * `nmp.nip51.add_bookmark` / `nmp.nip51.remove_bookmark` — [`nmp_nip51`]
//! 2. **Ingest parsers** for the kinds NMP knows how to decode into
//!    substrate caches:
//!    * kind:10050 → [`nmp_nip17::DmRelayCache`] (wired inside
//!      `nmp_nip17::register_actions` alongside the action modules — the
//!      same call installs the substrate `DmInboxRelayLookup`).
//!    * kind:10002 → [`nmp_router::InMemoryMailboxCache`] (wired below
//!      against the same shared cache the routing factory hands to the
//!      kernel — the `Kind10002Parser` is the cache's single writer).
//! 3. **Production routing substrate** — a factory closure that returns
//!    `(Arc<GenericOutboxRouter>, Arc<InMemoryMailboxCache>)` is installed
//!    via [`AppHost::set_routing_substrate`]. The kernel re-invokes the
//!    factory on `Reset` so the production routing survives a state wipe.
//! 4. **Production publish resolver** — a factory closure that returns
//!    `Arc<Nip65OutboxResolver>` is installed via
//!    [`AppHost::set_publish_resolver_factory`] (spec §271, 2026-05-25).
//!    The kernel re-invokes the factory on `Reset` so the production
//!    resolver survives a state wipe. Mirrors the routing factory — both
//!    deliberately live in `nmp-router` (Layer 2) so `nmp-core` (Layer 3)
//!    stays NIP-neutral (D0).
//! 5. **Indexer republish policy** — a factory closure returns
//!    [`nmp_router::IndexerRepublishPolicy`] through the generic raw-event
//!    forwarding seam. `nmp-core` owns only observer dispatch + pool send.
//! 6. **D2 coverage + NIP-77 hooks** — a [`CoverageGate`]-based hook trims
//!    oversized relay plans, and [`nmp_nip77::NegentropySyncRuntime`] replaces
//!    eligible large one-shot author×kind REQs with NIP-77 negentropy.
//! 7. **Canonical runtime controllers** — see [`runtimes`] — for WOT
//!    bootstrap, the NIP-17 DM-inbox subscription/projection, and the NIP-57
//!    self-zap-receipts subscription. These are pure host-side
//!    reconcilers; the kernel ships zero WOT/DM/zap nouns (D0).
//!
//! # What this crate is NOT
//!
//! * It does not register any app-specific projection (Chirp's
//!   `ModularTimelineProjection`, group-chat projection, Marmot, etc.).
//!   App-core composition crates call `register_defaults` once, then wire
//!   those app-specific registrations themselves.
//! * It does not own a C-ABI surface. The `nmp_app_*` FFI lives in
//!   `nmp-ffi` (and per-app `nmp_app_<app>_*` shells live in the app
//!   crate). This crate is pure Rust composition.
//! * It does not call `nmp_app_start`. The caller drives
//!   lifecycle.
//!
//! # Usage
//!
//! ```ignore
//! use nmp_defaults::{NmpAppBuilder, RunConfig};
//!
//! // 1. Construct the builder in the shell.
//! let mut builder = NmpAppBuilder::new();
//!
//! // 2. Call the app-core composition root. It calls `register_defaults`
//! //    exactly once, then registers app-specific projections/actions.
//! my_app_core::register(&mut builder);
//!
//! // 3. Drive the lifecycle.
//! let app = builder
//!     .in_memory()
//!     .consume_all_builtin_projections()
//!     .start(RunConfig::default());
//!
//! // 4. Tear down through the shell's normal FFI/runtime owner.
//! ```
//!
//! # Ordering contract
//!
//! `register_defaults` MUST be called **before** `nmp_app_start`.
//! All registrations need to be visible to the kernel when the first event
//! arrives — late wiring is dropped silently per `D6`.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost
//! [`AppHost::set_routing_substrate`]: nmp_core::substrate::AppHost::set_routing_substrate
//! [`AppHost::set_publish_resolver_factory`]: nmp_core::substrate::AppHost::set_publish_resolver_factory
//! [`AppHost::set_external_event_sink_policy_factory`]: nmp_core::substrate::AppHost::set_external_event_sink_policy_factory
//! [`AppHost::set_coverage_hook`]: nmp_core::substrate::AppHost::set_coverage_hook
//! [`CoverageGate`]: nmp_coverage_gate::CoverageGate

use std::sync::Arc;

use nmp_core::substrate::{
    register_observer_projection, AppHost, EventObserverRegistrar, SnapshotProjectionRegistrar,
};

pub mod action_payloads;
#[cfg(feature = "native")]
pub mod builder;
#[cfg(feature = "native")]
pub mod op_feed_defaults;
#[cfg(feature = "native")]
pub(crate) mod relay_config;
#[cfg(feature = "native")]
pub mod relay_info_probe;
pub mod runtimes;
pub mod search_defaults;
pub mod tiers;
pub mod topic_articles;

#[cfg(feature = "native")]
pub use builder::{NmpAppBuilder, ProjectionsDeclared, RunConfig, StorageSet, Unstarted};
#[cfg(feature = "native")]
pub use op_feed_defaults::{
    compile_feed_params, register_op_feed_defaults, register_op_feed_defaults_with_mute,
    OpFeedDefaults,
};
#[cfg(feature = "native")]
pub use relay_info_probe::{nmp_app_probe_relay_info, RelayInfoProbeCallback};
pub use runtimes::{
    register_bookmark_runtime, register_comment_runtime, register_mute_runtime,
    register_search_relay_runtime, register_search_relay_runtime_with,
};
pub use search_defaults::{effective_search_relays, SearchDefaults};
pub use tiers::{register_substrate, NmpDefaults};

/// Runtime read handles installed by [`register_defaults_with_handles`].
///
/// Most apps can ignore these and use [`register_defaults`]. App crates that
/// own product projections can keep the handles they need without re-registering
/// runtime observers or duplicating graph state.
#[derive(Clone, Default)]
pub struct NmpDefaultRuntimeHandles {
    /// Web-of-trust bootstrap/scoring runtime, present when
    /// [`NmpDefaults::social`] is true and observer installation succeeds.
    pub wot: Option<Arc<nmp_wot::WotBootstrapRuntime>>,
    /// Active-account mute-list runtime, present when
    /// [`NmpDefaults::social`] is true.
    pub mute: Option<Arc<nmp_nip51::MuteListProjection>>,
    /// Active-account search-relay-list runtime (kind:10007), present when
    /// [`NmpDefaults::social`] is true.
    ///
    /// Pass this to [`effective_search_relays`] at search time to resolve
    /// the effective relay list (user's kind:10007 list, else the
    /// [`SearchDefaults`] fallback, which may be empty).
    pub search_relays: Option<Arc<nmp_nip51::SearchRelayListProjection>>,
}

/// Wire the canonical NMP composition into `app`.
///
/// **Call this exactly once**, before `nmp_app_start`. It is NOT idempotent:
///
/// * **Action namespaces — last-writer-wins (replace, not reject).** The
///   action registry is a `HashMap` keyed on namespace; a second registration
///   of the same namespace *replaces* the first
///   (`nmp_core::kernel::action_registry`: "A second registration of the same
///   namespace replaces the first", a bare `HashMap::insert`). It does **not**
///   reject the duplicate.
/// * **Ingest parsers / event observers — additive.** A second call
///   double-registers them (e.g. the kind:10002 parser and the long-form
///   observer would each be installed twice).
/// * **Single-slot factories / hooks — last-writer-wins.** The
///   routing-substrate factory, publish-resolver factory, raw-event-forward
///   policy factory, and coverage hook are overwritten on each call.
///
/// Because parsers/observers are additive, calling `register_defaults` twice
/// is a latent bug, not a no-op — callers must invoke it once. (A
/// diagnostic/idempotence latch is tracked separately.)
///
/// See the crate-level doc for the full list of registrations and the
/// rationale for each.
///
/// # Ordering
///
/// MUST run before `nmp_app_start`. The kernel reads the
/// ingest-parser dispatcher, the routing-substrate factory, and the
/// coverage hook during its first compile/dispatch tick.
///
/// # `app` borrow
///
/// Most NIP-crate `register_actions` calls take `&mut AppHost` (the action
/// registry is a `&mut`-only surface — registrations happen at init, never
/// concurrently with `dispatch_action`). The substrate-routing factory +
/// coverage-hook installation paths take `&AppHost` (shared); the unique
/// borrow on the action-registry side is released before they run.
///
/// # Tiers
///
/// `register_defaults` is the convenience entry point: it delegates to
/// [`register_defaults_with`] with [`NmpDefaults::default()`], which in turn
/// calls [`register_substrate`] (the always-on correctness floor) and then
/// layers the social-feature defaults on top. A consumer that wants the
/// routable substrate *without* the social bundle calls
/// [`register_substrate`] directly; one that wants to toggle individual
/// social features or override the coverage policy / bootstrap relay calls
/// [`register_defaults_with`].
pub fn register_defaults(app: &mut impl AppHost) {
    let _ = register_defaults_with_handles(app, NmpDefaults::default());
}

/// Wire the canonical NMP composition and return runtime read handles.
///
/// This is the entry point for app-core crates that need to consume a default
/// runtime directly while preserving the same one-time registration semantics as
/// [`register_defaults`].
pub fn register_defaults_with_handles(
    app: &mut impl AppHost,
    defaults: NmpDefaults,
) -> NmpDefaultRuntimeHandles {
    register_defaults_inner(app, defaults)
}

/// Wire the canonical NMP composition into `app`, parameterised by `defaults`.
///
/// Always calls [`register_substrate`] (the correctness floor is NOT
/// toggleable — it is correctness, not preference), then layers the
/// social-feature defaults selected by the `social` / `dms` / `zaps` /
/// `longform` toggles on top, wires NIP-50 search fallback relays ONLY from
/// the app-supplied [`SearchDefaults`], and finally wires the `nostrconnect`
/// bootstrap relay ONLY when the leaf app supplied one (`Some(url)`).
///
/// `register_defaults(app)` ≡ `register_defaults_with(app,
/// NmpDefaults::default())` — the default-constructed config enables every
/// social feature and uses `CoverageGate::default()`, but owns NO operator
/// policy: relay-bearing fields default empty/`None` (#1493/#1924), so the
/// no-arg path wires no relay URL at all.
///
/// See [`NmpDefaults`] for the full field set and each field's default.
pub fn register_defaults_with(app: &mut impl AppHost, defaults: NmpDefaults) {
    let _ = register_defaults_with_handles(app, defaults);
}

fn register_defaults_inner(
    app: &mut impl AppHost,
    defaults: NmpDefaults,
) -> NmpDefaultRuntimeHandles {
    let NmpDefaults {
        coverage_gate,
        nostrconnect_bootstrap_relay,
        nostrconnect_perms,
        search_defaults,
        social,
        dms,
        zaps,
        longform,
        client_identity,
        attach_client_tag,
    } = defaults;
    let mut handles = NmpDefaultRuntimeHandles::default();

    // ── Substrate tier (always on — correctness, not preference) ─────────
    //
    // Routing factory + kind:10002 parser (shared `Arc<InMemoryMailboxCache>`),
    // mailbox-cache reader, publish-resolver factory, raw-event forward policy,
    // and the `CoverageGate` coverage hook + NIP-77 negentropy runtime (one
    // shared `coverage_gate` feeds both). See [`register_substrate`].
    register_substrate(app, coverage_gate);

    // ── NIP-50 public full-text search scopes (#1811) ────────────────────
    //
    // Register the crate-owned `nip50.profiles` / `nip50.notes` /
    // `nip50.longform` `SearchScopeProvider`s into the shared FTS scope
    // registry. Search is a generic public-query capability (NIP-50 is
    // transport infrastructure, not a social preference), so all three are
    // wired in the default bundle regardless of the social toggles — an
    // unqueried scope merely indexes events that already pass the kind filter,
    // and registration is additive/yielding (ADR-0049). The scopes are all
    // `PublicIndexable`, so private kinds ([4,13,14,15,1059,1060]) are dropped
    // from the compiled spec by construction; `nmp-core` names no FTS noun (the
    // call lives here in the composition crate, never in the kernel — D0).
    //
    // The registry the host accumulates these into is compiled +
    // `install_into(store)` at actor-kernel construction
    // (`nmp-core::actor::config::apply_to_kernel`), so they MUST be registered
    // before `start()` — guaranteed because `register_defaults` is a pre-start
    // config step.
    nmp_nip50::register_search_scopes(app);

    // ── NIP-50 input-scope recognizers (#1804, S7) ────────────────────────
    //
    // Register the three NIP-50 `InputScopeRecognizer`s (profiles / notes /
    // longform) into the shared input-scope registry so the input-intent
    // resolver (`nmp_app_intent_classify`) can produce `TextQuery` candidates
    // for free-text input with no app-level wiring call. Wired here alongside
    // the FTS scope registration in the always-on block (search is not a social
    // preference; free-text user-input classification is generic transport
    // infrastructure). `nmp-core` names no NIP-50 noun (D0).
    nmp_nip50::register_input_scopes(app);

    // ── Social-feature defaults (toggleable) ─────────────────────────────

    if social {
        // NIP-02: kind:3 follow/unfollow.
        nmp_nip02::register_follow_actions(app);
        // NIP-25: public kind:7 reactions and kind:5 unreact deletion.
        // Uses the typed descriptor (#1724 criterion 6).
        nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
        // NIP-84: public kind:9802 highlight publish (`nmp.nip84.publish_highlight`).
        // Yielding default via the typed descriptor.
        nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
        // NIP-29 group input-scope recognizer (#1804, S7).
        //
        // Register the `nip29.groups` `InputScopeRecognizer` so the
        // input-intent resolver can classify NIP-29 URI form
        // (`host'local-id`) and `naddr` references that point to a group.
        // The recognizer is pure/IO-free (claim-detect only; no HTTP, no
        // relay round-trip). NIP-29 is a social/group feature, so it belongs
        // in this block rather than the always-on substrate tier.
        nmp_nip29::register_input_scopes(app);
        // WOT bootstrap reconciler (EnsureInterest/DropInterestOwner book-keeping
        // for the active account; kernel ships zero WOT nouns — D0).
        handles.wot = nmp_wot::register_runtime(app);
        // NIP-51 mute-list observer + projection; the handle lets app-level
        // feed composition reuse the same read model instead of registering a
        // duplicate observer.
        handles.mute = Some(runtimes::register_mute_runtime(app));
        // NIP-51 kind:10003 global bookmarks. Registers the active-account
        // observer and the add/remove bookmark action modules against the same
        // read model, so writes merge the latest observed list instead of
        // overwriting it through PublishRaw.
        let _ = runtimes::register_bookmark_runtime(app);
        // NIP-51 kind:10007 search-relay list. Registers the active-account
        // observer so the SearchRelayListProjection is populated for the
        // signed-in account. The app-provided SearchDefaults backs the
        // higher-order NIP-50 fallback; the default value is empty, so
        // shared NMP composition never picks a public search relay.
        handles.search_relays = Some(runtimes::register_search_relay_runtime_with(
            app,
            search_defaults,
        ));
        // NIP-22 kind:1111 comments. Installs the threaded comment-thread
        // observer and registers the `nmp.nip22.post_comment` action. The
        // `Arc<CommentThreadProjection>` is dropped here (fire-and-forget
        // bundle path); apps that render comment threads call
        // [`register_comment_runtime`] directly to keep the snapshot handle.
        let _ = runtimes::register_comment_runtime(app);
    }

    if dms {
        // NIP-17: kind:14 chat-message DM send + kind:10050 DM-relay-list
        // publish. Critically, this call also installs the substrate
        // `DmInboxRelayLookup` AND registers the `Kind10050Parser` as an
        // `IngestParser` for kind:10050 (V-40).
        nmp_nip17::register_actions(app);
        // NIP-17 DM-inbox runtime (kind:1059 gift-wrap inbox projection +
        // relay-list reconciler).
        runtimes::register_dm_runtime(app);
    }

    if zaps {
        // NIP-57: kind:9734 zap-request build + LNURL fetch + bolt11
        // surfacing. The protocol crate owns the action module and the
        // `FetchLnurlInvoiceCommand` protocol command end-to-end (V-41).
        nmp_nip57::register_actions(app);
        // NIP-57 self-zap-receipts subscription runtime (kind:9735 `#p`).
        runtimes::register_zap_receipts_runtime(app);
    }

    if longform {
        // ── NIP-23 long-form TYPED snapshot projection (A5 root-cause fix) ──
        //
        // The default kind:30023 projection, emitted as a strongly-typed
        // FlatBuffer in the `typed_projections` sidecar (NOT the generic JSON
        // `projections` map — that map is being retired). Apps read resolved
        // `ArticleProjection` entries off the typed `nmp.nip23.articles`
        // payload instead of tapping raw events and re-parsing/re-superseding
        // NIP-23 tags by hand (the recurring A5 pattern found in Chirp /
        // Podcastr / tenex-off).
        register_longform_projection(app);
    }

    // ── Bootstrap relay for client-initiated NIP-46 (V-65 / #1493) ──────
    //
    // Fallback relay for `nostrconnect://` handshakes when the app has no
    // configured write relay. This is an operator-chosen relay URL, so NMP
    // (including this composition library) supplies NONE by default (#1493).
    // Only wire the slot when the leaf app explicitly provided one; otherwise
    // leave it unset and let the handshake resolve from configured write relays
    // (and fail-closed if there are none).
    //
    // A per-app crate may also set this after calling `register_defaults` by
    // invoking `AppHost::set_nostrconnect_bootstrap_relay` (last-writer-wins,
    // like every other pre-start slot).
    if let Some(relay) = nostrconnect_bootstrap_relay {
        app.set_nostrconnect_bootstrap_relay(relay);
    }

    // ── NIP-46 perm request for client-initiated nostrconnect (#1493 P9) ────
    //
    // Which event kinds an app asks the signer to sign is leaf-app PRODUCT
    // policy, so NMP (including this composition library) supplies NONE by
    // default (#1493). Only wire the slot when the leaf app explicitly provided
    // a perm set; otherwise leave it unset and let the handshake omit the
    // `&perms=` parameter entirely.
    //
    // A per-app crate may also set this after calling `register_defaults` by
    // invoking `AppHost::set_nostrconnect_perms` (last-writer-wins, like every
    // other pre-start slot) — this is how Chirp wires its policy from
    // `nmp-chirp-config`.
    if let Some(perms) = nostrconnect_perms {
        app.set_nostrconnect_perms(perms);
    }

    // ── Client identity → UA (Flow A, always) + NIP-89 client tag (Flow B, opt-in) ──
    //
    // The UA is derived whenever an identity is declared (privacy-neutral
    // transport header). The public NIP-89 `client` tag is opt-in via
    // `attach_client_tag` (framework default false). `set_outbound_public_tags`
    // takes opaque `Vec<Vec<String>>` — nmp-core never sees the NIP-89 noun (D0).
    if let Some(identity) = client_identity {
        app.set_relay_user_agent(identity.user_agent());
        if attach_client_tag {
            app.set_outbound_public_tags(vec![identity.client_tag()]);
        }
    }

    handles
}

/// Wire the default NIP-23 long-form (kind:30023) **typed** snapshot projection
/// into `app`.
///
/// Constructs one [`nmp_content::LongformProjection`] and registers it twice
/// against the same `Arc`: as a [`KernelEventObserver`](nmp_core::KernelEventObserver)
/// (ingest — accumulates the resolved `ArticleProjection` for every kind:30023
/// the kernel surfaces) AND as the typed snapshot projection under
/// [`LONGFORM_PROJECTION_KEY`](nmp_content::LONGFORM_PROJECTION_KEY) (output —
/// the `NL23` FlatBuffer carried in every frame's `typed_projections` sidecar).
/// Mirrors `nmp_wot::register_runtime`.
///
/// **Typed-only.** It registers exclusively through
/// [`AppHost::register_typed_snapshot_projection`]; it does NOT write into the
/// generic JSON `projections` map (that map is being retired). A host decodes
/// the `nmp.nip23.articles` payload with the generated `NL23` accessors.
///
/// D5-scoped: an observer only sees events from open subscriptions (an open
/// `topic_articles` `#t` feed or an event `resolve_ref(naddr-key)` document), so the
/// snapshot only ever carries the articles whose subscriptions are open.
/// Supersession is resolved by the kernel store before the observer fires
/// (`Inserted | Replaced` only), so the projection keeps the winning event with
/// no `created_at` comparison of its own.
///
/// Called by [`register_defaults`]; `pub` so an app opting out of the wholesale
/// defaults can still wire just this projection.
pub fn register_longform_projection(
    app: &(impl EventObserverRegistrar + SnapshotProjectionRegistrar),
) {
    use nmp_content::LongformProjection;
    use nmp_core::KernelEventObserver;

    let projection = Arc::new(LongformProjection::new());
    // register_observer_projection handles the D6 slot-poisoned guard (#1724 criterion 3):
    // if the observer slot is poisoned it returns None and skips the projection registration.
    let projection_for_closure = Arc::clone(&projection);
    register_observer_projection(
        app,
        Arc::clone(&projection) as Arc<dyn KernelEventObserver>,
        nmp_content::LONGFORM_PROJECTION_KEY,
        move || Some(projection_for_closure.typed_projection()),
    );
}
