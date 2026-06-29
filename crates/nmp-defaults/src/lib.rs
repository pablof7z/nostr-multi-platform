//! `nmp-defaults` — reusable NMP substrate and protocol composition installers.
//!
//! Per **ADR-0069**, production app roots compose named substrate, protocol,
//! app, capability, and policy pieces explicitly. This crate remains a runtime
//! library, not a template or generated scaffold to copy. See
//! `docs/architecture/crate-boundaries.md` §10.
//!
//! (Renamed from `nmp-app-template` by ADR-0046; the old name lied — a
//! "template" implies copy-and-edit. Reusable composition belongs in library
//! installers that app roots call deliberately.)
//!
//! # What this crate is
//!
//! The primary production installer is [`register_substrate`], the correctness
//! floor every NMP app stands on. The legacy convenience preset
//! [`register_defaults`] remains as tutorial/test/migration compatibility while
//! downstream apps migrate to explicit feature composition.
//!
//! The compatibility preset wires:
//!
//! 1. **Action modules** for the common NIPs:
//!    * `nmp.follow` / `nmp.unfollow` — [`nmp_nip02`]
//!    * `nmp.nip18.repost` / `nmp.nip18.quote_repost` — [`nmp_nip18`]
//!    * `nmp.nip25.react` / `nmp.nip25.unreact` — [`nmp_nip25`]
//!    * `nmp.nip17.send` / `nmp.nip17.publish_relay_list` — [`nmp_nip17`]
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
//!    bootstrap and the NIP-17 DM-inbox subscription/projection. These are pure
//!    host-side reconcilers; the kernel ships zero WOT/DM nouns (D0).
//!
//! # What this crate is NOT
//!
//! * It does not register any app-specific projection (Chirp's
//!   `ModularTimelineProjection`, group-chat projection, Marmot, etc.).
//!   App-core composition crates install substrate/protocol/app features in
//!   their own roots.
//! * It does not own a C-ABI surface. Generic `nmp_app_*` symbols live in the
//!   C ABI wrapper crate, and per-app `nmp_app_<app>_*` shells live in the app
//!   crate. This crate is pure Rust composition.
//! * It does not call `nmp_app_start`. The caller drives
//!   lifecycle.
//!
//! # Usage
//!
//! ```ignore
//! use nmp_core::substrate::AppHost;
//!
//! fn compose_app(host: &mut impl AppHost) {
//!     // 1. Install the shared substrate floor exactly once.
//!     nmp_defaults::register_substrate(
//!         host,
//!         nmp_defaults::NmpDefaults::default().coverage_gate,
//!     );
//!
//!     // 2. Register selected protocol features and app-specific modules.
//!     my_app_core::register(host);
//! }
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
    AppHost, ObservedProjection, ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
pub mod action_payloads;
mod composition;
pub mod op_pointer_source;
pub mod runtimes;
pub mod search_defaults;
pub mod tiers;
pub mod topic_articles;
mod topic_articles_wire;

pub use composition::{
    register_dm_protocol_defaults, register_nip50_protocol_defaults,
    register_social_protocol_defaults,
};
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
    /// NMP-owned NIP-65 (kind:10002) mailbox cache read handle (#2085).
    ///
    /// Always present after [`register_defaults_with_handles`] — the substrate
    /// tier unconditionally installs it, so unlike the social handles above this
    /// is never `None` in practice; it is `Option` only so the struct can keep
    /// `#[derive(Default)]` (a trait-object `Arc` has no `Default`).
    ///
    /// This is the **same** `Arc` instance wired into the kind:10002 parser
    /// writer, the routing-substrate factory, and the NIP-19 encoder reader, so
    /// reads observe exactly what the parser ingests. App-core crates (e.g.
    /// Highlighter's relay-import preview) read one author's importable relay
    /// list via [`snapshot`](nmp_core::substrate::MailboxCache::snapshot), which
    /// preserves the read/write/both role shape and exposes no raw event
    /// history. Do NOT construct a fresh `InMemoryMailboxCache` — a divergent
    /// instance is never written by the parser and reads empty.
    pub mailbox_cache: Option<Arc<dyn nmp_core::substrate::MailboxCache>>,
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
/// `register_defaults` is the compatibility convenience entry point: it delegates to
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
/// social-feature defaults selected by the `social` / `dms` / `longform`
/// toggles on top, wires NIP-50 search fallback relays ONLY from
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
    //
    // `register_substrate` returns the shared NIP-65 mailbox cache read handle
    // (#2085) — the same `Arc` it wires into the kind:10002 parser writer, the
    // routing factory, and the NIP-19 encoder reader. Surface it on the handles
    // struct so app-core crates get an instance-identical read handle.
    handles.mailbox_cache = Some(register_substrate(app, coverage_gate));

    register_nip50_protocol_defaults(app);

    // ── Social-feature defaults (toggleable) ─────────────────────────────

    if social {
        let social_handles = register_social_protocol_defaults(app, search_defaults);
        handles.wot = social_handles.wot;
        handles.mute = social_handles.mute;
        handles.search_relays = social_handles.search_relays;
    }

    if dms {
        register_dm_protocol_defaults(app);
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
/// Constructs one [`nmp_content::LongformProjection`] and registers it twice:
/// as a [`ObservedProjectionSink`](nmp_core::ObservedProjectionSink) and as the
/// typed snapshot projection under
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
    app: &(impl ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
) {
    use nmp_content::LongformProjection;
    use nmp_core::ObservedProjectionSink;

    let projection = Arc::new(LongformProjection::new());
    let observer_id = app.open_observed_projection(ObservedProjection::from_kinds(
        Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
        nmp_content::LONGFORM_PROJECTION_KEY,
        1,
        [nmp_content::KIND_LONG_FORM_ARTICLE],
        512,
    ));
    if observer_id == nmp_core::ObservedProjectionId(0) {
        return;
    }
    let projection_for_closure = Arc::clone(&projection);
    app.register_typed_snapshot_projection(nmp_content::LONGFORM_PROJECTION_KEY, move || {
        Some(projection_for_closure.typed_projection())
    });
}
