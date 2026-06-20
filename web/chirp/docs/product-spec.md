# Chirp Web — Product Spec

> Status: design (2026-06-12). This document defines the full product feature
> set for Chirp Web — the browser showcase app for NMP at `web/chirp/` — scoped
> honestly to what NMP supports today. Every feature carries exactly one tag:
>
> - `[backed-by-nmp-today]` — `nmp-wasm` already exposes it; ship UI only.
> - `[needs-wasm-exposure]` — the capability exists in `nmp-core`'s pure
>   kernel (the `KernelReducer` path the browser runtime drives); `nmp-wasm`
>   (plus, where noted, a mechanical public accessor on `KernelReducer`) must
>   expose it. No new core logic.
> - `[needs-core-work]` — the capability lives only behind the native actor
>   (`feature = "native"`) or does not exist; NMP itself must grow before
>   Chirp Web can have the feature. **Chirp Web never works around these
>   client-side.**
>
> The tag list doubles as the build-driving gap list (§5).

---

## 0. Philosophy

Chirp Web is an example app that proves the framework. It must be:

1. **Deployable with a straight face** — a real, polished Nostr client
   (already wired for static deploy: `web/chirp/vercel.json`, `vercel.json`).
2. **A showcase of what NMP does in the background** — a tasteful, legible
   inspector into relays, routing, reconciliation, caches, claims, and signer
   state is a first-class product surface, not an afterthought.
3. **Simple in implementation terms.** The shell renders snapshots and
   forwards intents. If a feature would require client-side protocol logic,
   that is a framework gap — it is listed in §5, not papered over in
   TypeScript. Zero technical debt.

Product reference: the existing Chirp shells —
`apps/chirp/nmp-app-chirp` (shared app crate), `apps/chirp/chirp-desktop`,
`apps/chirp/chirp-tui`, and `ios/Chirp`. Chirp Web is the same product on the
`nmp-wasm` worker runtime, plus the diagnostics surface no other shell makes
the hero.

> Note: `web/chirp/README.md:77-80` still claims "nmp-core does not yet
> compile to browser wasm". That is stale — `nmp-wasm` drives a real
> `nmp_core::KernelReducer` with live `BrowserRelayDriver` WebSockets
> (`crates/nmp-wasm/src/runtime.rs:1-50`, `crates/nmp-wasm/src/relay_pool.rs`).
> The README should be corrected as part of this work.

---

## 1. Ground truth — what `nmp-wasm` exposes today

The worker protocol is `crates/nmp-wasm/src/protocol.rs`; the wasm-bindgen
surface is `apps/chirp/nmp-app-chirp-web/src/wasm_binding.rs` (`NmpWasmRuntime`).

| Surface | Where | Notes |
|---|---|---|
| `Hello` / `Start` / `Stop` lifecycle, protocol v1 | `protocol.rs:6-30`, `runtime.rs:185-306` | `Start` spawns one live WebSocket `BrowserRelayDriver` per (URL, role) lane with native-equivalent reconnect/backoff (`relay_pool.rs:242-264`); the host supplies `relays` + `relay_bootstrap` explicitly (relay policy is host policy, not framework default — #1125); Chirp's web defaults live in `web/chirp/src/chirpConfig.ts` |
| `SetSigner` (kind `"nip07"` only) | `protocol.rs:26`, `protocol.rs:181-196`, `runtime.rs:336-351` | Host does `window.nostr.getPublicKey()` itself; other kinds honestly rejected (`unsupported_signer_kind`) |
| Async write path: `dispatch_app_action_async` | `lib.rs:188-215`, `publish_path.rs:145-252` | **`PublishNote` kind:1 top-level only.** Replies fail closed (`publish_path.rs:190-196`); `React`/`Follow`/`Unfollow` fail closed (`publish_path.rs:158-172`, issue #1007); non-NIP-07 backends fail closed (`publish_path.rs:82-88`). Correlation id threads into the publish engine (`publish_path.rs:232-235`) |
| Claims: `nmp.kernel.claim_profile` / `release_profile` / `claim_event` / `release_event` | `dispatch_routing.rs:62-86`, `runtime.rs:388-424` | Emit real REQs through the live driver pool; F-TTL `force` is hardcoded `false` (`runtime.rs:393-399`) |
| Kernel dispatches: `nmp.kernel.start/stop/diagnostics/open_uri/open_view/close_view` | `dispatch_routing.rs:148-176` | `open_uri` routes NIP-19 / `nostr:` URIs through the kernel resolver |
| Binary snapshot push (`Uint8Array` FlatBuffers via callback) | `lib.rs:123-126`, `snapshot.rs:148-180` | Fires on every kernel-mutating relay frame and after publishes |
| **Snapshot content** | `snapshot.rs:95-126` | **Nearly empty:** rev, running flag, relay bootstrap echoed with hardcoded `connection: "configured"`. `build_snapshot_bytes` ignores the reducer (`_reducer`, `snapshot.rs:95`) and passes an empty typed-projection sidecar — this is the single biggest exposure gap |
| Routing-decision inspector (pull) | `lib.rs:141-144`, `runtime.rs:480-483` | `recent_routing_decisions()` → JSON, schema-versioned, same shape as iOS `nmp_app_recent_routing_decisions` (`nmp-core/src/kernel/routing_trace_dto.rs`) |
| Honest degraded modes | `protocol.rs:205-220`, `dispatch_routing.rs:112-138` | Stable, pattern-matchable reason prefixes (`signer_not_installed`, `publish_path_not_wired*`, `browser_actor_driver_missing`) |

What the **pure kernel** already holds when driven by the wasm runtime (all
reachable through `KernelReducer`, none of it surfaced to JS yet): per-lane
relay health incl. auth + negentropy probe state
(`nmp-core/src/kernel/status.rs:90-117`), wire-subscription lifecycle, event
store + timeline ingest, profile cache, claim registries, publish engine with
per-relay verdicts, planner/interest registry
(`nmp-core/src/kernel/mod.rs:2714` `open_interest_sub`), Tier-2 typed
projections (`nmp-core/src/typed_projections/mod.rs:281-370`), and the routing
trace. The snapshot encoder already supports relay statuses, wire subs,
metrics, error fields, and a typed-projection sidecar
(`nmp-core/src/update_envelope.rs:249-300`) — the wasm builder just doesn't
populate it.

What is **actor-only today** (native feature gate, not reachable from the
browser runtime): identity store / account lifecycle (`nmp.sign_in_nsec`,
`nmp.create_account`, switch/remove), NIP-46 signer broker
(`crates/nmp-signer-broker`), NIP-17 DM runtime, NIP-29 group observers,
NIP-47 wallet runtime (`apps/chirp/nmp-app-chirp/src/wallet_runtime.rs`),
the capability socket (HTTP fetches — wasm returns
`browser_actor_driver_missing`, `runtime.rs:208-219`), the NIP-77 reconciler
(`crates/nmp-nip77`), LMDB persistence (`crates/nmp-store/src/lib.rs` —
Memory or Lmdb only; **no IndexedDB binding exists anywhere**,
`runtime.rs:36`), and drain-on-emit action-lifecycle projections (actor tick
scoping).

---

## 2. Half 1 — Core Nostr client features

### 2.1 MVP — genuinely deployable

The MVP is the shortest path to "a real Nostr client": NIP-07 login →
live home timeline → profiles → compose → threads, with a minimal live
diagnostics panel (§3.1). Mirrors the Home/Thread/Author/Compose surface of
`chirp-desktop` (`apps/chirp/chirp-desktop/src/app.rs`).

| # | Feature | Tag | Backing |
|---|---|---|---|
| M1 | **Boot & connect** — worker starts, dials the default Chirp relay set, reconnects with backoff; honest fallback banner when Worker/wasm is unavailable | `[backed-by-nmp-today]` | `WorkerRequest::Start` (`protocol.rs:8,40-48`); driver pool `relay_pool.rs:242-264`; degraded fallback already in `web/chirp/src/nmp/client.ts:45-63` |
| M2 | **Sign in with NIP-07** — connect browser extension, show logged-in pubkey/avatar; signed-out mode is read-only browse | `[backed-by-nmp-today]` | `SetSigner` kind `"nip07"` (`protocol.rs:26,181-196`; `runtime.rs:336-351`). Caveat folded into M3: the kernel must also learn the viewer pubkey (today the signer slot is runtime-local) |
| M3 | **Home timeline** — contact feed (kinds 1+6) of the signed-in user's follows, live-updating; sensible signed-out default (e.g. a curated/global interest) | `[needs-wasm-exposure]` | All logic is pure-kernel: contact feed = `Kernel::set_follow_feed_kinds` (the actor wrapper is 10 lines, `nmp-core/src/actor/commands/publish.rs:698-715`), kind:3 ingest + follow-feed registration, timeline ingest. Gaps: (a) no dispatch verb for contact feed / interests in `dispatch_routing.rs:148-176`; (b) `SetSigner` must hand the viewer pubkey to the reducer; (c) feed items must reach JS — see G2/G4; (d) the worker never pumps `KernelReducer::tick()` (`nmp-core/src/kernel_reducer.rs:186`; no call site in `nmp-wasm`), so lifecycle-drained REQs/retries have no driver — see G6 |
| M4 | **Note cards with resolved profiles** — author name/avatar/nip05 on every card via refcounted profile claims (components self-claim on mount, release on unmount) | `[needs-wasm-exposure]` | Claim/release REQs work today (`dispatch_routing.rs:62-86`); the **resolved profile data has no read-back** — `nmp.profile.resolved` / `nmp.profile.claimed` Tier-2 projections exist in core (`typed_projections/mod.rs`) but the wasm snapshot encodes no projections (`snapshot.rs:95-97`) |
| M5 | **Compose & publish a note** (kind:1, top-level) with optimistic pending state | `[backed-by-nmp-today]` | `dispatch_app_action_async` (`lib.rs:188-215`, `publish_path.rs:145-252`). Per-relay verdict feedback is M7 |
| M6 | **Threads** — open a note's thread (root + replies, NIP-10), reply from the thread view | `[needs-wasm-exposure]` | Read: `Kernel::open_interest_sub` exists (`kernel/mod.rs:2714`, `pub(crate)`) — needs a `KernelReducer` forwarding + dispatch verb, mirroring native `nmp_app_open_interest` (`docs/ffi-surface.md` §6); thread feed composition mirrors `nmp-app-chirp/src/ffi/interest_feed.rs:215-260`. Write: replies fail closed on wasm (`publish_path.rs:190-196`); NIP-10 tag building is host-side per issue #906 via `nmp-nip01::Note::reply_to` (`protocol.rs:122-131`) — the wasm publish path must accept tags |
| M7 | **Publish outcome surfacing** — "accepted by 2/3 relays", retrying, failed (the `chirp-desktop` Outbox, minimum slice) | `[needs-wasm-exposure]` | Publish engine + per-relay verdicts run in the reducer (`KernelReducer::publish_signed_event`, `kernel_reducer.rs:228`; correlation threaded, `publish_path.rs:232-235`); `nmp.publish.queue` / `nmp.publish.outbox` / `nmp.publish.summary` Tier-2 projections exist (`typed_projections/mod.rs`) — not encoded into the wasm snapshot |
| M8 | **Error toasts** — kernel-owned `last_error_toast` / category rendered verbatim | `[needs-wasm-exposure]` | Envelope fields exist (`update_envelope.rs:37-83`); wasm builder never populates them (`snapshot.rs:102-126`) |

### 2.2 v1 — the full Chirp parity that wasm can honestly reach

| # | Feature | Tag | Backing |
|---|---|---|---|
| V1 | **React (like)** kind:7 | `[needs-wasm-exposure]` | `AppAction::React` is in the protocol (`protocol.rs:105-109`) and fails closed on the async path (`publish_path.rs:158-172`, issue #1007). NIP-25 tag derivation exists in `nmp-nip02`/native command path |
| V2 | **Follow / Unfollow** kind:3 | `[needs-wasm-exposure]` | Same fail-closed path (issue #1007). Follow-set merge logic must come from the kernel's contact state (kernel ingests kind:3), not the shell |
| V3 | **Repost** kind:6 | `[needs-wasm-exposure]` | Native shells publish kind:6 via `nmp.publish` `PublishRaw` (`docs/dispatch-actions.md:72-87`); wasm async path is `PublishNote`-only — generalizing it to `PublishRaw` covers this |
| V4 | **Author / profile pages** — profile header + author feed; pull-to-refresh = F-TTL `force` | `[needs-wasm-exposure]` | Author feed declares primary `[1]`; the NIP-18 adapter derives acquisition `{"kinds":[1,6],"authors":[pk]}`. Same gap as M6; profile header via claim (works today) + projection read-back (M4); `force` is hardcoded `false` on wasm (`runtime.rs:393-399`) — expose it |
| V5 | **Hashtag feeds** | `[needs-wasm-exposure]` | Tag feed declares primary `[1]`; the Chirp app seam derives NIP-18 repost-wrapper acquisition and opens the compiled `#t` filter globally. Same exposure as M6 |
| V6 | **`nostr:` URI / NIP-19 routing** — paste or click `npub`/`note`/`nevent`/`naddr`, kernel resolves and routes | `[backed-by-nmp-today]` | `nmp.kernel.open_uri` (`dispatch_routing.rs:153-157`) + `claim_event` for hydration (`runtime.rs:403-409`); *display* of the claimed event depends on projection read-back (G2) |
| V7 | **Embedded note rendering** — quoted `nostr:` entities inside notes hydrate via event claims | `[needs-wasm-exposure]` | `claim_event` REQs work today; `nmp.event.claimed` projection not in the wasm snapshot (G2); content tokenization belongs to `nmp-content` (already a crate), not the shell |
| V8 | **Reaction / repost / zap counts on cards** | `[needs-wasm-exposure]` | `nmp.reactions.summary` / `nmp.reactions.reposts` (`nmp-nip01/note_relations`) and `nmp.nip57.zaps` (`nmp-nip57`) are observer-registered projections; the kernel owns observer registration (`kernel/mod.rs:193,445-457`) — needs a wasm-side registration seam (G2). If the observer seam proves actor-bound in practice, this escalates to core work — flag at implementation time |
| V9 | **Edit profile** (kind:0 publish) | `[needs-wasm-exposure]` | `nmp.publish` `PublishProfile` variant exists in core (`docs/dispatch-actions.md:44-55`); wasm write path must route it |
| V10 | **Relay management** — add/remove relays + roles at runtime, NIP-65 list publish | `[needs-wasm-exposure]` | Kernel owns configured-relay state (`nmp.relay.configured_relays` Tier-2) and `nmp.nip65.publish_relay_list` exists (`docs/dispatch-actions.md:217-233`); wasm gap: driver pool is fixed at `Start` (`runtime.rs:316-326`) — needs add/remove driver mutation + dispatch verbs |
| V11 | **Persistence across reloads** | `[needs-core-work]` | "Kernel still runs in memory, resets on page reload" (`runtime.rs:36`); `nmp-store` has `Memory`/`Lmdb` only (`crates/nmp-store/src/lib.rs`); zero IndexedDB code in the workspace. Core must grow an IndexedDB (or OPFS) `EventStore` backend |

### 2.3 Stretch — blocked on core, listed so nobody fakes them

| # | Feature | Tag | Why it's blocked |
|---|---|---|---|
| S1 | **DMs (NIP-17)** | `[needs-core-work]` | DM inbox runtime is actor-managed (`nmp-app-chirp/src/ffi/register.rs:345-361`); wasm also lacks a NIP-44 bridge (`window.nostr.nip44.*`) in `nmp-signers` |
| S2 | **Zaps (NIP-57 send)** | `[needs-core-work]` | Zap flow needs LNURL HTTP fetches via the capability socket; wasm capability completions return `browser_actor_driver_missing` (`runtime.rs:208-219`) |
| S3 | **Wallet (NIP-47)** | `[needs-core-work]` | `WalletRuntime` (TTL sweep, heartbeat, on-demand wallet relay lane) is actor-side (`nmp-app-chirp/src/wallet_runtime.rs`) |
| S4 | **Groups (NIP-29)** | `[needs-core-work]` | Group chat/discovery are actor-registered Tier-1 observers (`nmp-app-chirp/src/ffi/register.rs:254-343`) |
| S5 | **NIP-46 bunker login** | `[needs-core-work]` | Broker is a native thread (`crates/nmp-signer-broker`); "NIP-46 bunker on wasm needs a wasm-native NIP-46 transport the broker side does not yet expose" (`publish_path.rs:79-88`) |
| S6 | **Multi-account / account switching** | `[needs-core-work]` | Identity store + account actions are actor commands (`docs/ffi-surface.md` §5); wasm has a single signer slot |
| S7 | **nsec import / create account in browser** | **deliberately excluded** | "LocalKey can't run in the wasm runtime (the runtime should not hold key material)" (`publish_path.rs:80-83`). NIP-07 (and later NIP-46) are the only honest web signers. This is a product stance, not a gap |

---

## 3. Half 2 — The NMP showcase / diagnostics surface

This is what makes Chirp Web special: a permanently available, tasteful
inspector (the existing `RuntimePanel` in `web/chirp/src/features/RuntimePanel.tsx`
grows into this). Everything here renders kernel truth — the shell computes
nothing.

### 3.1 MVP diagnostics panel

| # | Diagnostic | Tag | Source |
|---|---|---|---|
| D1 | **Runtime strip** — worker vs in-process fallback, protocol version, `Ready/Running/Degraded(reason)/Stopped`, honest degraded banners with the kernel's stable reason prefixes | `[backed-by-nmp-today]` | `RuntimeStatus`/`DegradedMode` (`protocol.rs:205-220`); reason prefixes (`dispatch_routing.rs:112-138`); already partially rendered (`client.ts:90-136`) |
| D2 | **Relay table** — per (URL, lane): connection state, auth state, events_rx, reconnect count, last error/NOTICE | `[needs-wasm-exposure]` | The reducer tracks real per-lane `RelayHealth` (`relay_pool.rs:93-167` feeds `handle_relay_connected/failed/closed/frame`); the wire format supports it (`RelayStatusEntry`, `update_envelope/relay_status.rs:22-36`); the wasm builder hardcodes `"configured"` (`snapshot.rs:108-123`) |
| D3 | **Routing inspector** — last N publish/subscription routing decisions: which lanes (NIP-65 / Hint / Provenance / UserConfigured / Indexer / fallback) matched, which relay URLs won | `[backed-by-nmp-today]` | `NmpWasmRuntime.recent_routing_decisions()` (`lib.rs:141-144`); schema `nmp-core/src/kernel/routing_trace_dto.rs:104-175`; shares the renderer contract with iOS (V-51 phase 3) |
| D4 | **Live publish outbox** — queue entries, per-relay OK verdicts, retry state, summary counts | `[needs-wasm-exposure]` | Same exposure as M7 (`nmp.publish.queue/outbox/summary` Tier-2 projections) |
| D5 | **Snapshot heartbeat** — rev, schema version, update counter, bytes/frame; makes the push channel itself visible | `[backed-by-nmp-today]` | Envelope `rev`/`kernel_schema_version` already decoded by the shell (`web/chirp/src/nmp/updateFrame.ts`, `client.ts:97-128`) |

### 3.2 v1 diagnostics

| # | Diagnostic | Tag | Source |
|---|---|---|---|
| D6 | **Wire-subscription inspector** — every open NIP-01 REQ: wire id, relay, filter summary, state (open/eose/closed), events_rx, close reason (web twin of iOS `WireSubscriptionDetailView`) | `[needs-wasm-exposure]` | Kernel wire-sub lifecycle is pure; wire slot exists (`WireSubscriptionEntry`, `update_envelope/relay_status.rs:42-51`); not populated by `snapshot.rs` |
| D7 | **Claim registry view** — live profile/event claims with consumer ids, refcounts, F-TTL freshness (`check_again_after`) | `[needs-wasm-exposure]` | Claims mutate the reducer today (`runtime.rs:388-424`); `nmp.profile.claimed` / `nmp.event.claimed` Tier-2 projections exist (`typed_projections/mod.rs`) — needs G2 |
| D8 | **Profile-cache panel** — resolved profiles count, hit/miss on claim (TTL gate vs network), mention-derived profiles | `[needs-wasm-exposure]` | `nmp.profile.resolved` / `nmp.profile.mentions` Tier-2 projections; TTL design `docs/design/replaceable-freshness.md` |
| D9 | **Store/cache stats** — events ingested, duplicates dropped, stored events, visible items | `[needs-wasm-exposure]` | `Metrics { events_rx, duplicate_events, stored_events, visible_items, … }` lives in the kernel (`nmp-core/src/kernel/types.rs:650-777`); envelope `Metrics` slot exists (`update_envelope.rs:264-273`); unpopulated on wasm |
| D10 | **Negentropy (NIP-77) probe badge** — per relay lane: `unknown / probing / supported / unsupported` | `[needs-wasm-exposure]` | Probe state is kernel-owned (`kernel/status.rs:90-117`, `kernel/types.rs:226`); carried on the full Tier-3 relay row, absent from the wasm envelope subset |
| D11 | **Signer panel** — backend kind (NIP-07), cached pubkey, last sign outcome; queue of events that went to the extension | `[needs-wasm-exposure]` | Signer slot is runtime state (`runtime.rs:116`); `nmp.event.signed` Tier-2 projection exists; failure reasons already stable (`publish_path.rs:57-88`) |
| D12 | **Interest/planner view** — logical interests with refcounts and cache coverage (native `InterestRow` twin, `nmp-app-chirp/src/snapshot_types.rs:104-115`) | `[needs-wasm-exposure]` | Interest registry is pure-kernel (planner + `SubscriptionLifecycle`); surfacing rides G2/G3 |

### 3.3 Stretch diagnostics

| # | Diagnostic | Tag | Source |
|---|---|---|---|
| D13 | **NIP-77 reconciliation stats** — ranges reconciled, events healed, bytes saved vs naive REQ | `[needs-core-work]` | The reconciler (`crates/nmp-nip77`) runs only on the native side; the kernel carries the probe diagnostic only (`kernel/status.rs:403-404`). Browser reconciliation needs core work first (and is a flagship showcase once it lands) |
| D14 | **Action lifecycle timeline** — dispatch → signed → published → per-relay OK as an animated trace | `[needs-core-work]` | `nmp.action.lifecycle/results/stages` are drain-on-emit projections whose semantics are scoped to the actor's snapshot tick; `KernelReducer` has no tick-scoped drain. Needs a core decision (drain-on-push semantics for the reducer) before wasm exposure |
| D15 | **Persistence panel** — IndexedDB store size, hydration-on-boot stats | `[needs-core-work]` | Depends on V11 (no browser store exists) |

---

## 4. Prioritized rollout

1. **MVP** = M1–M8 + D1–D5. Deployable definition of done: a stranger with a
   NIP-07 extension signs in, reads their follows' feed with names and
   avatars, posts a note, opens a thread, replies — and the diagnostics panel
   shows live relays, routing decisions, and the publish outbox the whole
   time. Without the extension, the app still browses and the panel still
   shows the runtime working.
2. **v1** = V1–V10 + D6–D12 (full social verbs, author/hashtag/URI
   navigation, relay management, and the complete inspector).
3. **Stretch** = V11, S1–S6, D13–D15 — each gated on the named core work;
   none may be approximated in the shell.

---

## 5. Gap list (drives the build)

### 5.1 `nmp-wasm` exposure gaps (core logic exists; expose it)

- **G1 — Populate the snapshot envelope from the reducer.**
  `build_snapshot_bytes` ignores its `_reducer` argument and hardcodes
  `connection: "configured"` (`crates/nmp-wasm/src/snapshot.rs:95-126`).
  Needs a mechanical public read accessor on `KernelReducer` (relay statuses,
  wire subscriptions, metrics, error fields — all already in the kernel and
  already encodable, `update_envelope.rs:249-300`). Unblocks M8, D2, D6, D9,
  D10.
- **G2 — Typed-projection sidecar over the wasm snapshot.** Expose Tier-2
  projection registration/emission (`typed_projections/mod.rs:281-370`,
  kernel seam at `kernel/mod.rs:193,445-457`) through the snapshot push so
  `nmp.publish.*`, `nmp.profile.*`, `nmp.event.claimed`, feeds, reaction/zap
  aggregates reach JS. Unblocks M4, M7, V6–V8, D4, D7, D8, D11, D12.
- **G3 — Interest + contact-feed dispatch verbs.** Forward
  `Kernel::open_interest_sub` (`kernel/mod.rs:2714`) and
  `set_follow_feed_kinds` (per `actor/commands/publish.rs:698-731`) through
  `KernelReducer` and `dispatch_routing.rs`, mirroring native
  `nmp_app_open_interest`/`open_contact_feed` (`docs/ffi-surface.md` §5–6).
  `SetSigner` must also hand the viewer pubkey to the reducer so the follow
  feed has an account to register against. Unblocks M3, M6, V4, V5.
- **G4 — Web feed composition.** A wasm-reachable equivalent of
  `nmp-app-chirp/src/ffi/interest_feed.rs` (FlatFeed predicates +
  `nmp.feed.home` / `nmp.feed.author.*` / `nmp.feed.thread.*` registration,
  `nmp-nip01/src/flat_feed.rs`) so the web shell renders the same Rust-owned
  feed snapshots as desktop/TUI — feed assembly must never move into
  TypeScript. Unblocks M3, M6, V4, V5.
- **G5 — Complete the async write path.** React / Follow / Unfollow /
  replies-with-NIP-10-tags / `PublishRaw` / `PublishProfile` on
  `dispatch_app_action_async` (fail-closed stubs at
  `publish_path.rs:158-196`; issues #1007, #906). Unblocks M6 (write half),
  V1–V3, V9.
- **G6 — Tick driver.** Nothing in `nmp-wasm` calls `KernelReducer::tick()`
  (`kernel_reducer.rs:186`) — publish retries and lifecycle-drained REQs have
  no pump in the browser. The worker needs a timer-driven tick (kernel
  decides cadence policy; the host only supplies the timer). Unblocks M3, M7
  reliability.
- **G7 — F-TTL `force` + claim parity.** Expose the `force` flag on wasm
  claims (hardcoded `false`, `runtime.rs:393-399`) for explicit-navigation
  refresh (V4).

### 5.2 Core gaps (NMP must grow; Chirp Web waits)

- **G8 — Browser persistence:** IndexedDB/OPFS `EventStore` backend in
  `nmp-store` (`runtime.rs:36`; `nmp-store/src/lib.rs`). → V11, D15.
- **G9 — NIP-44/NIP-17 on wasm:** `window.nostr.nip44` bridge in
  `nmp-signers` + a DM runtime that is not actor-bound. → S1.
- **G10 — Capability socket on wasm:** HTTP capability completions currently
  fail with `browser_actor_driver_missing` (`runtime.rs:208-219`); needed for
  LNURL/zaps. → S2.
- **G11 — Wallet runtime off-actor** (`nmp-app-chirp/src/wallet_runtime.rs`).
  → S3.
- **G12 — NIP-29 observers off-actor.** → S4.
- **G13 — NIP-46 wasm transport** (`publish_path.rs:79-88`;
  `crates/nmp-signer-broker`). → S5.
- **G14 — Identity/account store reachable from wasm.** → S6.
- **G15 — NIP-77 reconciliation from the browser runtime**
  (`crates/nmp-nip77`; probe-only today, `kernel/status.rs:90-117`). → D13.
- **G16 — Reducer-side drain semantics for action-lifecycle projections.**
  → D14.

---

## 6. Non-goals

- No client-side protocol logic: no NIP-10 thread reconstruction, follow-set
  merging, relay selection, or cache management in TypeScript — ever.
- No nsec entry in the browser (S7 — product stance).
- No mock data, no faked diagnostics: every panel renders kernel truth or is
  absent.
