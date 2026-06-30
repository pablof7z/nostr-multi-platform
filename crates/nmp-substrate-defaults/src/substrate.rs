//! AppHost-level substrate construction: routing, publish, coverage, NIP-77,
//! and NIP-11 wiring.
//!
//! This module holds [`register_substrate`], the correctness floor every NMP
//! app stands on. It is separated from the wasm-safe cache/parser wiring in
//! `lib.rs` so the heavier per-registrar imports stay local.
//!
//! The `native` cargo feature gates the NIP-11 relay-info fetcher which
//! requires ureq (not available on wasm32).

use std::sync::Arc;

use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use nmp_core::substrate::{
    ActionRegistrar, BlockedRelayLookupRegistrar, CoverageHookRegistrar, ExternalEventSinkPolicy,
    IngestParserRegistrar, KernelReaderRegistrar, RelayConnectedHookRegistrar,
    RelayTextInterceptorRegistrar, ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar,
};
use nmp_coverage_gate::CoverageGate;
use nmp_router::{
    InMemoryBlockedRelayCache, IndexerRepublishPolicy, Kind10006Parser, Nip65OutboxResolver,
};
use nmp_store::EventStore;

/// Wire the **substrate** tier — the correctness floor every NMP app needs,
/// regardless of which social features it enables (the `MinimalPlugins` analog).
///
/// This is NOT toggleable: without it routing returns `Unroutable`,
/// `PublishTarget::Auto` fail-closes, kind:10002 never reaches the mailbox
/// cache, and oversized relay plans are never trimmed. It is *correctness*,
/// not preference, so `nmp_defaults::register_defaults_with` always calls it.
///
/// `gate` parameterises the D2 coverage hook **and** the NIP-77 negentropy
/// runtime — the single shared value whose sharing this signature makes
/// explicit (passing it in, rather than constructing it here, is what lets a
/// caller override the coverage policy for *both* collaborators at once
/// without desyncing them). `nmp_defaults::register_defaults` passes
/// `CoverageGate::default()`.
///
/// # Returns
///
/// The shared NIP-65 (kind:10002) mailbox cache as a read handle (#2085) — the
/// **same** `Arc<dyn MailboxCache>` instance this function wires into all three
/// substrate seams: the NIP-19 encoder reader (`set_mailbox_cache_reader`), the
/// routing-substrate factory, and the `nmp_router::Kind10002Parser` writer.
/// Instance identity is load-bearing: a non-social external consumer that needs
/// to read an author's importable NIP-65 relay list reads through this handle
/// rather than constructing a fresh `InMemoryMailboxCache` (which would be
/// empty and divergent from the cache the parser actually writes). The handle
/// preserves the read/write/both role shape via
/// [`nmp_core::substrate::MailboxCache::snapshot`] and exposes no raw event
/// history.
///
/// # Shared host target
///
/// The substrate tier is intentionally expressed only in terms of narrow
/// `nmp_core::substrate` registrar traits. It does not require C ABI glue,
/// native storage, OS handles, or reducer internals. Host-backed browser and
/// native builders can therefore implement the same registrars and receive the
/// same routing/mailbox/profile/contact floor through this function.
///
/// # `app` borrow
///
/// Takes `&mut` because `nmp_router::register_actions` needs the `&mut`-only
/// action-registry surface; every other substrate seam used here
/// (`set_routing_substrate`, `set_publish_resolver_factory`,
/// `set_coverage_hook`, …) is a shared-`&self` method.
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
) -> Arc<dyn nmp_core::substrate::MailboxCache> {
    // NIP-65: kind:10002 relay-list publish. The `nmp-router` crate owns
    // both routing AND the kind:10002 publish path. This is the routing
    // crate's own action — inseparable from the routing substrate it
    // publishes for, NOT a social toggle.
    nmp_router::register_actions(app);

    // ── Shared cache/parser substrate wiring ────────────────────────────
    //
    // The mailbox/profile/contacts cache+parser pairs are single-sourced in
    // this crate so the writer and reader handles cannot drift between native
    // and web. The helper installs the shared mailbox cache (also as the H4
    // `set_mailbox_cache_reader` read side for the NIP-19 encoder), the
    // routing-substrate factory, and the kind:10002 / kind:0 / kind:3
    // cache+parser pairs.
    //
    // Returns the shared NIP-65 mailbox cache as a read handle (#2085) — the
    // SAME `Arc` wired into the encoder reader, the routing factory, and the
    // `Kind10002Parser` writer.
    let mailbox_cache = crate::install_on_app_host(app);

    // ── kind:10006 blocked-relay cache (Phase 0 relay-attribution) ──────
    //
    // Install the SAME `InMemoryBlockedRelayCache` on both ends — identical
    // pattern to the kind:10002 mailbox cache above (one Arc, two roles):
    //   1. As the kernel's `Arc<dyn BlockedRelayLookup>` (reader) via
    //      `set_blocked_relay_lookup`, so `snapshot_blocked_relays` returns
    //      the set the parser populated.
    //   2. As the `Kind10006Parser`'s backing cache (writer), registered with
    //      the `EventIngestDispatcher` so every accepted kind:10006 upserts
    //      the parsed blocked-relay set.
    // The same `Arc<InMemoryBlockedRelayCache>` is captured by BOTH paths so
    // the writer (parser) and the reader (kernel routing context) see one
    // source of truth.
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
    // this swap the kernel keeps its in-crate `NoopOutboxResolver` default —
    // every `PublishTarget::Auto` publish then resolves to an empty relay set
    // and the publish engine surfaces `NoTargets` (fail-closed). `nmp-core`
    // (Layer 3) cannot depend on `nmp-router` (Layer 2), so the production
    // resolver is injected through this factory slot.
    //
    // The factory receives the kernel-owned event store + the three typed
    // slots. The closure is re-invoked by the `Reset` dispatch arm against
    // the rebuilt kernel's fresh handles.
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
    // `max_relay_connections` before `plan_diff`. `per_relay` is a `BTreeMap`
    // so the "keep first N" trim is deterministic across runs.
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
    // `RelayConnectedHook` that fetches each relay's NIP-11 document the
    // first time it connects (per-URL TTL) and surfaces it on the
    // `relay_diagnostics` projection. Apps get relay metadata with zero work;
    // `nmp-core` names no NIP-11 noun (D0).
    //
    // NIP-11-over-ureq is a native transport; the browser registers its own
    // fetch-based RelayConnectedHook in a later issue (#2046/#2057).
    #[cfg(feature = "native")]
    nmp_nip11::register(app);

    // Hand the shared NIP-65 mailbox cache read handle back to the caller
    // (#2085). This is the same `Arc` wired into the encoder reader, the
    // routing factory, and the kind:10002 parser writer above.
    mailbox_cache
}
