//! The two composition tiers behind [`super::register_defaults`] — the
//! **substrate** tier (correctness) and the **social-feature** defaults tier —
//! plus the [`NmpDefaults`] config struct that parameterises them.
//!
//! # Why two tiers (the V-48 failure mode, restated)
//!
//! `register_defaults` historically fused two layers with *different
//! audiences*. **Substrate correctness** — without it the app is broken, not
//! minimal: routing returns `Unroutable`, `PublishTarget::Auto` fail-closes to
//! `NoTargets`, kind:10002 never populates the mailbox cache, oversized relay
//! plans are never trimmed. This is the `MinimalPlugins` analog from the Bevy
//! `DefaultPlugins` study — the irreducible floor every NMP app stands on.
//! **Social-feature defaults** — the nip02/nip17/nip57 action bundles, the
//! WOT/DM/zap runtime controllers, the NIP-23 long-form typed projection — are
//! *preferences*, not correctness; a non-social external consumer
//! (podcast-player, hl, win-the-day) wants the floor without the ceiling.
//!
//! Before this split a non-social consumer had two bad options: call
//! `register_defaults` and swallow the social bundle, or hand-copy the
//! substrate block — which is *un-copyable* because it threads a single shared
//! `Arc<InMemoryMailboxCache>` through three seams (the mailbox-cache reader,
//! the routing factory, and the kind:10002 parser). Copy it wrong and the
//! writer (parser) and readers (router + NIP-19 encoder) desync. That is the
//! V-48 failure mode this crate exists to prevent — so the substrate tier is
//! now a *callable* [`register_substrate`], not a comment block.
//!
//! # The tier boundary (drawn here, honestly)
//!
//! **Substrate** ([`register_substrate`]): `nmp_router::register_actions` (the
//! `nmp.nip65.publish_relay_list` action — the routing crate's *own* action,
//! inseparable from the routing substrate it publishes for, NOT a social
//! toggle); the shared `Arc<InMemoryMailboxCache>` + mailbox-cache reader +
//! routing factory + kind:10002 parser (one cache, three clones); the
//! publish-resolver factory; the raw-event forward/republish policy; and the
//! `CoverageGate` coverage hook + NIP-77 negentropy runtime — the gate value
//! is **shared** between the hook and the runtime, so overriding it post-hoc
//! would desync them, which is why it is a [`NmpDefaults`] field, not a literal.
//!
//! **Social** (added by [`register_defaults`] on top): nip02/nip17/nip57
//! action bundles, WOT/DM/zap runtime controllers, the long-form typed
//! projection, and explicit app-supplied operator policy such as
//! `nostrconnect` and NIP-50 search fallback relays.

use std::sync::Arc;

use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use nmp_core::substrate::{
    ActionRegistrar, BlockedRelayLookupRegistrar, CoverageHookRegistrar, ExternalEventSinkPolicy,
    IngestParserRegistrar, KernelReaderRegistrar, RelayConnectedHookRegistrar,
    RelayTextInterceptorRegistrar, ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar,
};
use nmp_coverage_gate::CoverageGate;
use nmp_nip89::ClientIdentity;
use nmp_router::{
    InMemoryBlockedRelayCache, IndexerRepublishPolicy, Kind10006Parser, Nip65OutboxResolver,
};
use nmp_store::EventStore;

use crate::SearchDefaults;

/// Declarative configuration for [`super::register_defaults_with`] — the
/// config-as-fields pattern (Bevy's `.set(WindowPlugin { .. })` insight, and
/// the discoverability win from Spring Boot's configuration metadata: every
/// knob is a named, rustdoc'd field rather than a hardcoded literal buried in
/// the composition body).
///
/// [`NmpDefaults::default()`] is the no-operator-policy composition:
/// `register_defaults(app)` ≡ `register_defaults_with(app,
/// NmpDefaults::default())`, and leaf apps opt into relay-bearing policy by
/// filling the named fields before registration.
#[derive(Clone, Debug)]
pub struct NmpDefaults {
    /// Coverage policy shared by the D2 coverage hook **and** the NIP-77
    /// negentropy runtime. One value feeds *both* collaborators: the hook
    /// trims oversized relay plans to `max_relay_connections`, and the
    /// negentropy runtime reads the same gate to decide which large
    /// author×kind REQs to replace with NIP-77 sync. Overriding the gate
    /// post-hoc is impossible without desyncing them — which is precisely
    /// why it lives here as config rather than as a hardcoded
    /// `CoverageGate::default()` inside the substrate body.
    ///
    /// **Default:** [`CoverageGate::default()`] (`max_relay_connections = 30`).
    pub coverage_gate: CoverageGate,

    /// Fallback relay for client-initiated NIP-46 (`nostrconnect://`)
    /// handshakes when the app has no configured write relay. This is an
    /// operator-chosen relay URL — leaf-app policy, NOT an `nmp-defaults`
    /// default (#1493): NMP, including this composition library, owns no relay
    /// URLs. `None` means no fallback is wired; a `nostrconnect://` handshake
    /// then resolves the relay from the app's configured write relays and, if
    /// there are none, fails-closed (the FFI returns a null URI) rather than
    /// dialing any framework-chosen relay.
    ///
    /// A leaf app that wants a bootstrap fallback sets `Some(url)` here (or
    /// calls `AppHost::set_nostrconnect_bootstrap_relay` after
    /// `register_defaults`).
    ///
    /// **Default:** `None`.
    pub nostrconnect_bootstrap_relay: Option<String>,

    /// NIP-46 permission request advertised in client-initiated
    /// `nostrconnect://` handshakes — which event kinds the app asks the signer
    /// to sign (the plain, NOT percent-encoded, comma-joined NIP-46 perm list,
    /// e.g. `"sign_event:1,sign_event:7"`). This is leaf-app PRODUCT policy, NOT
    /// an `nmp-defaults` default (#1493): NMP, including this composition
    /// library, owns no perm set. `None` means no perms are wired and a
    /// `nostrconnect://` handshake omits the `&perms=` parameter entirely.
    ///
    /// A leaf app that wants to request perms sets `Some(perms)` here (or calls
    /// `AppHost::set_nostrconnect_perms` after `register_defaults`).
    ///
    /// **Default:** `None`.
    pub nostrconnect_perms: Option<String>,

    /// App-declared fallback search relays for NIP-50 when the active account
    /// has no user-authored kind:10007 search-relay list. This is operator
    /// policy, not framework policy: user kind:10007 relays remain first
    /// authority, this field is second, and an empty list means relay search is
    /// cache-only until the user publishes a list or the app supplies defaults.
    ///
    /// **Default:** empty.
    pub search_defaults: SearchDefaults,

    /// Wire the NIP-02 follow/unfollow/react action bundle **and** the WOT
    /// bootstrap runtime. The social graph layer. Disable for a non-social
    /// consumer that never follows, reacts, or computes web-of-trust.
    ///
    /// **Default:** `true`.
    pub social: bool,

    /// Wire the NIP-17 DM action bundle (`send` + `publish_relay_list`) **and**
    /// the DM-inbox runtime (kind:1059 gift-wrap inbox projection + relay-list
    /// reconciler). Disable for a consumer that never sends or receives DMs.
    ///
    /// **Default:** `true`.
    pub dms: bool,

    /// Wire the NIP-57 zap action bundle **and** the self-zap-receipts
    /// subscription runtime (kind:9735 `#p` reconciler). Disable for a
    /// consumer with no lightning/zap surface.
    ///
    /// **Default:** `true`.
    pub zaps: bool,

    /// Wire the NIP-23 long-form (kind:30023) **typed** snapshot projection
    /// (`nmp.nip23.articles`, the `NL23` FlatBuffer). Disable for a consumer
    /// that never reads long-form articles.
    ///
    /// **Default:** `true`.
    pub longform: bool,

    /// App ClientIdentity declared once at the composition root. When `Some`,
    /// derives the relay User-Agent (always) and, if `attach_client_tag`, the
    /// NIP-89 `client` tag on PublicRoutable publishes.
    ///
    /// **Default:** `None` (transport falls back to the built-in `nmp/<ver>` UA;
    /// no client tag).
    pub client_identity: Option<ClientIdentity>,

    /// Opt-in: attach the NIP-89 public `client` tag to PublicRoutable publishes.
    /// Privacy default is OFF (the UA is always derived, but the public tag is
    /// opt-in). Ignored when `client_identity` is `None`.
    ///
    /// **Default:** `false`.
    pub attach_client_tag: bool,
}

impl Default for NmpDefaults {
    /// The canonical NMP wiring: `CoverageGate::default()`, every social
    /// feature on, and NO operator relay policy. Relay-bearing fields are empty
    /// or `None` — NMP ships no relay URL (#1493/#1924); a leaf app that wants
    /// a nostrconnect or search fallback supplies it explicitly.
    fn default() -> Self {
        Self {
            coverage_gate: CoverageGate::default(),
            nostrconnect_bootstrap_relay: None,
            nostrconnect_perms: None,
            search_defaults: SearchDefaults::default(),
            social: true,
            dms: true,
            zaps: true,
            longform: true,
            client_identity: None,
            attach_client_tag: false,
        }
    }
}

/// Wire the **substrate** tier — the correctness floor every NMP app needs,
/// regardless of which social features it enables (the `MinimalPlugins`
/// analog).
///
/// This is NOT toggleable: without it routing returns `Unroutable`,
/// `PublishTarget::Auto` fail-closes, kind:10002 never reaches the mailbox
/// cache, and oversized relay plans are never trimmed. It is *correctness*,
/// not preference, so [`super::register_defaults_with`] always calls it.
///
/// `gate` parameterises the D2 coverage hook **and** the NIP-77 negentropy
/// runtime — the single shared value whose sharing this signature makes
/// explicit (passing it in, rather than constructing it here, is what lets a
/// caller override the coverage policy for *both* collaborators at once
/// without desyncing them). [`super::register_defaults`] passes
/// `CoverageGate::default()`.
///
/// # Shared host target
///
/// The substrate tier is intentionally expressed only in terms of narrow
/// `nmp_core::substrate` registrar traits. It does not require `nmp-ffi`,
/// native storage, OS handles, or reducer internals. Host-backed browser and
/// native builders can therefore implement the same registrars and receive the
/// same routing/mailbox/profile/contact floor through this function; reducer-
/// owned web harnesses still use `nmp-substrate-defaults` for the cache/parser
/// helper until they grow a full host-backed composition root.
///
/// # `app` borrow
///
/// Takes `&mut impl AppHost` because `nmp_router::register_actions` needs the
/// `&mut`-only action-registry surface; every other substrate seam used here
/// (`set_routing_substrate`, `set_publish_resolver_factory`,
/// `set_coverage_hook`, …) is a shared-`&self` method, so the unique borrow is
/// released before they run.
///
/// Exposed `pub` so a non-social external consumer (podcast-player, hl,
/// win-the-day) can stand on the exact same routable substrate as Chirp
/// **without** swallowing the social bundle — and without hand-copying the
/// un-copyable shared-`Arc<InMemoryMailboxCache>` block (the V-48 failure mode
/// this crate exists to prevent).
pub fn register_substrate(
    app: &mut (impl ActionRegistrar
              + BlockedRelayLookupRegistrar
              + CoverageHookRegistrar
              + IngestParserRegistrar
              + KernelReaderRegistrar
              + RelayConnectedHookRegistrar
              + RelayTextInterceptorRegistrar
              + ReqFrameInterceptorRegistrar
              + RoutingFactoryRegistrar),
    gate: CoverageGate,
) {
    // NIP-65: kind:10002 relay-list publish. The `nmp-router` crate owns
    // both routing AND the kind:10002 publish path (step 3 absorbed the
    // former `nmp-nip65` crate into `nmp-router`). This is the routing
    // crate's own action — inseparable from the routing substrate it
    // publishes for, NOT a social toggle.
    nmp_router::register_actions(app);

    // ── Shared cache/parser substrate wiring ────────────────────────────
    //
    // The mailbox/profile/contacts cache+parser pairs are used by both the
    // native `AppHost` tier and reducer-owned web composition roots. Keep that
    // construction single-sourced in the wasm-safe helper crate so the writer
    // and reader handles cannot drift between native and web. The helper
    // installs the shared mailbox cache (also as the H4 `set_mailbox_cache_reader`
    // read side for the NIP-19 encoder), the routing-substrate factory, and the
    // kind:10002 / kind:0 / kind:3 cache+parser pairs.
    nmp_substrate_defaults::install_on_app_host(app);

    // ── kind:10006 blocked-relay cache (Phase 0 relay-attribution) ──────
    //
    // Install the SAME `InMemoryBlockedRelayCache` on both ends — identical
    // pattern to the kind:10002 mailbox cache above (one Arc, two roles):
    //   1. As the kernel's `Arc<dyn BlockedRelayLookup>` (reader) via
    //      `set_blocked_relay_lookup`, so `snapshot_blocked_relays` returns the
    //      set the parser populated.
    //   2. As the `Kind10006Parser`'s backing cache (writer), registered with
    //      the `EventIngestDispatcher` so every accepted (D4 `Inserted |
    //      Replaced`) kind:10006 upserts the parsed blocked-relay set.
    // The same `Arc<InMemoryBlockedRelayCache>` is captured by BOTH paths so
    // the writer (parser) and the reader (kernel routing context) see one source
    // of truth. Without this wiring `snapshot_blocked_relays` always returns an
    // empty set regardless of what the user has blocked.
    let blocked_cache: Arc<InMemoryBlockedRelayCache> = Arc::new(InMemoryBlockedRelayCache::new());
    app.set_blocked_relay_lookup(
        Arc::clone(&blocked_cache) as Arc<dyn nmp_core::substrate::BlockedRelayLookup>
    );
    let blocked_parser: Arc<dyn nmp_core::substrate::IngestParser> =
        Arc::new(Kind10006Parser::new(Arc::clone(&blocked_cache)));
    app.register_ingest_parser(10_006, blocked_parser);
    // Register the `nmp.nip51.block_relay` and `nmp.nip51.unblock_relay`
    // action modules. Both hold an `Arc` clone of the SAME
    // `InMemoryBlockedRelayCache` the parser writes into and the kernel reads
    // via `Arc<dyn BlockedRelayLookup>`, so their idempotency guards see live
    // state (Phase 4 of relay-connection-attribution — §3.4).
    nmp_router::register_block_relay_actions(app, blocked_cache);

    // ── Publish-resolver substrate (spec §271, 2026-05-25) ─────────────
    //
    // Install the production substrate-publish-resolver factory. Without
    // this swap the kernel keeps its in-crate `NoopOutboxResolver`
    // default — every `PublishTarget::Auto` publish then resolves to an
    // empty relay set and the publish engine surfaces `NoTargets`
    // (fail-closed). `nmp-core` (Layer 3) cannot depend on `nmp-router`
    // (Layer 2), so the production resolver is injected through this
    // factory slot.
    //
    // The factory receives the kernel-owned event store + the three
    // typed slots (`IndexerRelaysSlot`, `LocalWriteRelaysSlot`,
    // `ActiveAccountSlot`) — the actor reducer is the sole writer of
    // those slots (D4), so the produced `Nip65OutboxResolver` reads
    // through the same shared state the actor pushes into. The closure
    // is re-invoked by the `Reset` dispatch arm against the rebuilt
    // kernel's fresh handles.
    app.set_publish_resolver_factory(
        |store: Arc<dyn EventStore>,
         indexer_relays: IndexerRelaysSlot,
         local_write_relays: LocalWriteRelaysSlot,
         active_account: ActiveAccountSlot|
         -> Arc<dyn OutboxResolver> {
            Arc::new(Nip65OutboxResolver::with_local_relays(
                store,
                indexer_relays,
                local_write_relays,
                active_account,
            ))
        },
    );

    // ── External event sink policy ─────────────────────────────────────
    //
    // The dispatcher routes typed `SignedEventFrame`s to the injected policy
    // objects. The replaceable-kind/indexer republish policy belongs in
    // `nmp-router` beside the rest of the indexer-lane routing rules; default
    // composition injects it here. The factory is re-invoked on `Reset` with
    // the rebuilt kernel's fresh store/provenance + indexer-relay handles.
    app.set_external_event_sink_policy_factory(|context| {
        vec![Arc::new(IndexerRepublishPolicy::enabled(context)) as Arc<dyn ExternalEventSinkPolicy>]
    });

    // ── D2 coverage + NIP-77 sync hooks ─────────────────────────────────
    //
    // Install a `CoverageGate`-based hook on the kernel so the M2 compiler
    // pipeline's `apply_selection` output is trimmed to the gate's
    // `max_relay_connections` before `plan_diff`. `per_relay` is a
    // `BTreeMap` so the "keep first N" trim is deterministic across runs.
    //
    // The SAME `gate` value feeds the NIP-77 negentropy runtime below — the
    // hook and the runtime are two collaborators reading one coverage policy.
    // This shared-value invariant is why `gate` is a parameter (and a
    // `NmpDefaults` field) rather than a literal: overriding it post-hoc
    // would desync the two.
    let negentropy_runtime = Arc::new(nmp_nip77::NegentropySyncRuntime::new(gate.clone()));
    let req_interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor> =
        negentropy_runtime.clone();
    let relay_interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor> = negentropy_runtime;
    app.set_req_frame_interceptor(req_interceptor);
    app.add_relay_text_interceptor(relay_interceptor);
    app.set_coverage_hook(Arc::new(move |plan| {
        let cap = gate.max_relay_connections;
        if plan.per_relay.len() > cap {
            let keep: Vec<_> = plan.per_relay.keys().take(cap).cloned().collect();
            plan.per_relay.retain(|k, _| keep.contains(k));
        }
    }));

    // ── NIP-11 relay information (ADR-0051) ─────────────────────────────
    //
    // Relay metadata (name / icon / supported_nips) is generic transport
    // infrastructure, not a social-feature preference — it belongs in the
    // always-on substrate tier. `nmp_nip11::register` installs a
    // `RelayConnectedHook` that fetches each relay's NIP-11 document the first
    // time it connects (per-URL TTL) and surfaces it on the `relay_diagnostics`
    // projection. Apps get relay metadata with zero work; `nmp-core` names no
    // NIP-11 noun (D0).
    //
    // NIP-11-over-ureq is a native transport; the browser registers its own
    // fetch-based RelayConnectedHook in a later issue (#2046/#2057).
    #[cfg(feature = "native")]
    nmp_nip11::register(app);
}
