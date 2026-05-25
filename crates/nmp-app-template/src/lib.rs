//! `nmp-app-template` — canonical composition root for an NMP-based Nostr app.
//!
//! Step 10 of `docs/architecture/crate-boundaries.md` §5. Closes **V-48**:
//! "No `nmp-app-template` crate — second-app developer must read 403 LOC of
//! Chirp to understand registration".
//!
//! # What this crate is
//!
//! A single function — [`register_defaults`] — that, given a freshly
//! constructed [`NmpApp`], wires every registration a generic Nostr app
//! needs to participate in the standard NMP composition:
//!
//! 1. **Action modules** for the common NIPs:
//!    * `nmp.follow` / `nmp.unfollow` / `nmp.nip25.react` — [`nmp_nip02`]
//!    * `nmp.nip17.send` / `nmp.nip17.publish_relay_list` — [`nmp_nip17`]
//!    * `nmp.nip57.zap` — [`nmp_nip57`]
//!    * `nmp.nip65.publish_relay_list` — [`nmp_router`]
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
//!    via [`NmpApp::set_routing_substrate`]. The kernel re-invokes the
//!    factory on `Reset` so the production routing survives a state wipe.
//! 4. **Production publish resolver** — a factory closure that returns
//!    `Arc<Nip65OutboxResolver>` is installed via
//!    [`NmpApp::set_publish_resolver_factory`] (spec §271, 2026-05-25).
//!    The kernel re-invokes the factory on `Reset` so the production
//!    resolver survives a state wipe. Mirrors the routing factory — both
//!    deliberately live in `nmp-router` (Layer 2) so `nmp-core` (Layer 3)
//!    stays NIP-neutral (D0).
//! 5. **D2 coverage hook** — a [`CoverageGate`]-based hook is installed via
//!    [`NmpApp::set_coverage_hook`] so the production kernel enforces D2
//!    ("negentropy before REQ") for large follow sets — backstop trim on
//!    `max_relay_connections`.
//! 6. **Canonical runtime controllers** — see [`runtimes`] — for the
//!    NIP-17 DM-inbox subscription/projection and the NIP-57
//!    self-zap-receipts subscription. These are pure host-side
//!    reconcilers; the kernel ships zero DM/zap nouns (D0).
//!
//! # What this crate is NOT
//!
//! * It does not register any app-specific projection (Chirp's
//!   `ModularTimelineProjection`, group-chat projection, Marmot, etc.).
//!   Per-app crates wire those themselves on top of `register_defaults`.
//! * It does not own a C-ABI surface. The `nmp_app_*` FFI lives in
//!   `nmp-core` (and per-app `nmp_app_<app>_*` shells live in the app
//!   crate). The template is pure Rust composition.
//! * It does not call [`nmp_core::nmp_app_start`]. The caller drives
//!   lifecycle.
//!
//! # Usage
//!
//! ```ignore
//! use nmp_core::{nmp_app_free, nmp_app_new};
//!
//! // 1. Construct the app.
//! let app = nmp_app_new();
//!
//! // 2. Inherit the canonical NMP composition.
//! // SAFETY: `app` is a valid pointer from `nmp_app_new`.
//! nmp_app_template::register_defaults(unsafe { &mut *app });
//!
//! // 3. (Optional) Register any app-specific projections / actions.
//! //    — e.g. a `ModularTimelineProjection` for a Twitter-style client.
//!
//! // 4. Drive the lifecycle (`nmp_app_start`, callbacks, etc.).
//!
//! // 5. Tear down.
//! nmp_app_free(app);
//! ```
//!
//! # Ordering contract
//!
//! `register_defaults` MUST be called **before** [`nmp_core::nmp_app_start`].
//! All registrations need to be visible to the kernel when the first event
//! arrives — late wiring is dropped silently per `D6`.
//!
//! [`NmpApp`]: nmp_core::NmpApp
//! [`NmpApp::set_routing_substrate`]: nmp_core::NmpApp::set_routing_substrate
//! [`NmpApp::set_publish_resolver_factory`]: nmp_core::NmpApp::set_publish_resolver_factory
//! [`NmpApp::set_coverage_hook`]: nmp_core::NmpApp::set_coverage_hook
//! [`CoverageGate`]: nmp_coverage_gate::CoverageGate

use std::sync::Arc;

use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use nmp_core::store::EventStore;
use nmp_core::substrate::{MailboxCache, OutboxRouter, RoutingTraceObserver};
use nmp_ffi::NmpApp;
use nmp_coverage_gate::CoverageGate;
use nmp_router::{GenericOutboxRouter, InMemoryMailboxCache, Nip65OutboxResolver};

pub mod runtimes;

/// Wire the canonical NMP composition into `app`.
///
/// One call. Idempotency is the same as the underlying per-NIP
/// `register_actions` calls — the action registry rejects duplicate
/// namespaces; the ingest dispatcher allows additive parsers per kind; the
/// routing-substrate slot is overwritten on each call. A second call is a
/// no-op for actions, additive for parsers, and last-writer-wins for the
/// routing substrate / coverage hook.
///
/// See the crate-level doc for the full list of registrations and the
/// rationale for each.
///
/// # Ordering
///
/// MUST run before `nmp_core::nmp_app_start`. The kernel reads the
/// ingest-parser dispatcher, the routing-substrate factory, and the
/// coverage hook during its first compile/dispatch tick.
///
/// # `app` borrow
///
/// Most NIP-crate `register_actions` calls take `&mut NmpApp` (the action
/// registry is a `&mut`-only surface — registrations happen at init, never
/// concurrently with `dispatch_action`). The substrate-routing factory +
/// coverage-hook installation paths take `&NmpApp` (shared); the unique
/// borrow on the action-registry side is released before they run.
pub fn register_defaults(app: &mut NmpApp) {
    // ── Action modules ───────────────────────────────────────────────────
    //
    // NIP-02: kind:3 follow/unfollow + kind:7 reactions. Substrate-level
    // social verbs every Nostr app shares. Originally lived as
    // `ChirpFollowModule` / `ChirpUnfollowModule` / `ChirpReactModule`
    // inside `nmp-app-chirp`; lifted into `nmp-nip02` so the template can
    // wire them through one call.
    nmp_nip02::register_actions(app);

    // NIP-17: kind:14 chat-message DM send + kind:10050 DM-relay-list
    // publish. Critically, this call also installs the substrate
    // `DmInboxRelayLookup` (so the gift-wrap publish path's
    // `recipient_dm_relays` reader sees the cache) AND registers the
    // `Kind10050Parser` as an `IngestParser` for kind:10050 (V-40).
    nmp_nip17::register_actions(app);

    // NIP-57: kind:9734 zap-request build + LNURL fetch + bolt11 surfacing.
    // The protocol crate owns the action module and the
    // `FetchLnurlInvoiceCommand` protocol command end-to-end (V-41).
    nmp_nip57::register_actions(app);

    // NIP-65: kind:10002 relay-list publish. The `nmp-router` crate owns
    // both routing AND the kind:10002 publish path (step 3 absorbed the
    // former `nmp-nip65` crate into `nmp-router`).
    nmp_router::register_actions(app);

    // ── Routing substrate (V-51 phase 5 + kind:10002 ingest wiring) ─────
    //
    // Install the production substrate-routing factory AND register the
    // [`nmp_router::Kind10002Parser`] against the same shared cache the
    // factory hands the kernel. Without the swap the kernel keeps its
    // in-crate `EmptyOutboxRouter` (substrate-honest debt B, 2026-05-24)
    // — every routing decision returns `Unroutable`. Without the parser
    // registration kind:10002 events arrive but never populate the cache
    // (D0 violation flagged 2026-05-25 — the kernel previously named
    // kind:10002 by hand in `ingest/mod.rs:391` because the parser was
    // never wired; that explicit arm + `kernel/ingest/relay_list.rs` are
    // both deleted in the same PR as this wiring lands).
    //
    // `nmp-core` (Layer 3) cannot depend on `nmp-router` (Layer 2), so
    // both injections go through the substrate seam: the routing factory
    // for `(OutboxRouter, MailboxCache)`, and the dispatcher slot for
    // the parser. The same `Arc<InMemoryMailboxCache>` is captured by
    // BOTH paths so the writer (parser) and the readers (router +
    // planner adapter) see one source of truth.
    //
    // Cache lifetime: created once at composition time and shared
    // process-lifetime (mirrors `nmp_nip17::register_actions`'s
    // single-`DmRelayCache` pattern). A `Reset` rebuilds the router but
    // re-uses this cache — the parser's `Arc` clone in the dispatcher
    // would otherwise dangle (writes to a cache no longer held by the
    // rebuilt kernel). The factory closure captures `Arc::clone(&cache)`
    // so the rebuilt kernel sees the live cache, not a fresh empty one.
    //
    // The supplied `RoutingTraceObserver` is threaded through
    // `GenericOutboxRouter::with_trace_observer` so the kernel's
    // trace-projection ring buffer (V-51 phase 1) keeps populating across
    // the swap — the FFI snapshot surface (phase 2) and `chirp-repl
    // routing-trace` (phase 4) keep working unchanged. The closure is
    // re-invoked by the `Reset` dispatch arm against the rebuilt kernel's
    // fresh trace projection.
    let mailbox_cache: Arc<InMemoryMailboxCache> = Arc::new(InMemoryMailboxCache::new());
    let cache_for_factory = Arc::clone(&mailbox_cache);
    app.set_routing_substrate(
        move |observer: Arc<dyn RoutingTraceObserver>|
              -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>) {
            let router: Arc<dyn OutboxRouter> =
                Arc::new(GenericOutboxRouter::new().with_trace_observer(observer));
            let cache: Arc<dyn MailboxCache> = Arc::clone(&cache_for_factory) as _;
            (router, cache)
        },
    );
    // Register the substrate kind:10002 ingest parser against the same
    // cache `set_routing_substrate` hands the kernel. The
    // `EventIngestDispatcher` fans every accepted (D4 `Inserted | Replaced`)
    // kind:10002 to this parser; the parser upserts the resolved
    // `ParsedRelayList` (or removes the entry on an empty list) into
    // the shared cache. The kernel observes the cache transition in its
    // wildcard ingest arm and fires the recompile trigger — no NIP
    // knowledge in `nmp-core/src/kernel/`.
    let parser: Arc<dyn nmp_core::substrate::IngestParser> =
        Arc::new(nmp_router::Kind10002Parser::new(mailbox_cache));
    app.register_ingest_parser(10_002, parser);

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
    // through the same shared state the actor pushes into (e.g. local
    // relay-row edits become visible to the resolver immediately, before
    // the just-sent kind:10002 round-trips from a relay). The closure
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

    // ── D2 coverage hook ────────────────────────────────────────────────
    //
    // Install a `CoverageGate`-based hook on the kernel so the M2 compiler
    // pipeline's `apply_selection` output is trimmed to the gate's
    // `max_relay_connections` before `plan_diff`. `per_relay` is a
    // `BTreeMap` so the "keep first N" trim is deterministic across runs
    // — important for reproducible test runs and human-readable
    // diagnostics.
    //
    // Stage 3 (post-v1) will extend this closure with negentropy steering
    // — once the negentropy infrastructure is available the body will
    // check `gate.should_use_negentropy(author_count)` and mark sub-shapes
    // for a reconciliation handshake instead of a raw REQ.
    let gate = CoverageGate::default();
    app.set_coverage_hook(Arc::new(move |plan| {
        let cap = gate.max_relay_connections;
        if plan.per_relay.len() > cap {
            let keep: Vec<_> = plan.per_relay.keys().take(cap).cloned().collect();
            plan.per_relay.retain(|k, _| keep.contains(k));
        }
    }));

    // ── Canonical runtime controllers ───────────────────────────────────
    //
    // Two snapshot-projection-driven reconcilers that own per-tick
    // PushInterest / WithdrawInterest book-keeping for the active account.
    // Kernel ships zero DM/zap nouns (D0); these controllers are the
    // canonical host-side wiring every NMP-based app needs.
    runtimes::register_dm_runtime(app);
    runtimes::register_zap_receipts_runtime(app);
}
