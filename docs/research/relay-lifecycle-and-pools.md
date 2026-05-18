# Relay lifecycle + connection pools — NMP vs NDK/applesauce

> **Status:** research / gap analysis. No code changes proposed in this doc — only
> evidence + comparison + severity-ranked gaps + a D0-respecting design sketch
> and `Tnnn (proposed)` task stubs for the orchestrator.

This doc answers three questions about the current `master` tip:

1. **What does the core do when a relay socket drops?** (reconnect, backoff, replay,
   in-flight publish, AUTH re-do)
2. **App lifecycle (iOS background → foreground)** — kernel concern or shell?
   What is documented vs actually wired in `ios/NmpPulse`?
3. **Relay-purpose segregation** (NIP-65 outbox, NIP-46 bunker, NIP-29 group host,
   NIP-50 search, NIP-37 drafts) — generic pool or ad-hoc per-feature?

Honest TL;DR up front:

- Reconnect-on-drop ships in the relay worker, but with **a fixed 3 s delay, no
  jitter, no cap-because-no-growth, no live-REQ replay, no AUTH re-do, no
  publish replay**. The `SubscriptionLifecycle::handle_reconnect` *primitive*
  ships in `subs/mod.rs` but is **never called** from the actor today.
- The kernel exposes no foreground/background trigger; `nmp-nip77` carries a
  `TriggerEvent::{Foreground, RelayReconnected}` engine, but the iOS shell
  (`NmpPulse`) wires **zero** of it — no `scenePhase`, no `UIApplication.*`.
- Relay-purpose segregation is **hardcoded to two relays, two `RelayRole`
  values (`Content`, `Indexer`)**. The planner has the right shape
  (`RoutingSource` four-lane, `relay_pin` Case-E, NIP-65 outbox) but the live
  REQ/EVENT wire path — including publish — still routes through
  `RelayRole::Content` (the T105 outbox-on-wire migration is the named gap, see
  `docs/builder-guide/27-discrepancies.md` row 1).

---

## 1 — What NMP does TODAY

### 1.1 Connection lifecycle in the relay worker

Per-relay-role worker thread, one per `RelayRole` (`Content`,
`Indexer`). The whole socket lifecycle is in
`crates/nmp-core/src/relay_worker.rs`:

| Behavior | Evidence | Verdict |
|---|---|---|
| Drop detected | `socket.read()` Err → `RelayEvent::Failed` + `RelayWorkerResult::Reconnect` (`relay_worker.rs:159-171`) | OK |
| Backoff before reconnect | `RELAY_RECONNECT_DELAY = Duration::from_secs(3)` constant (`relay_worker.rs:64`); `wait_before_reconnect` loops a flat 3 s (`:197-217`) | **PARTIAL** — fixed delay, no exponential, no jitter, no cap |
| Reconnect-storm protection | absent — 2 fixed URLs make storms moot today, but no jitter/cap once relay set is dynamic | **ABSENT** (future-blocker) |
| Live REQ replay on reconnect | `RelayEvent::Connected` handler calls `kernel.relay_connected(role)` + `maybe_send_startup`; the latter only fires once via `startup_sent` flag (`actor/dispatch.rs:241-252` + `actor/relay_mgmt.rs:58-75`) — no per-sub replay | **ABSENT on the live path.** `SubscriptionLifecycle::handle_reconnect` ships (`subs/mod.rs:211-238`) and is unit-tested (`nmp-testing/tests/m8_subscription_lifecycle.rs:184`), but **no actor code path calls it** (`grep handle_reconnect crates/` → only definition + tests) |
| AUTH re-do on reconnect | `Nip42DriverState::reset_on_disconnect` (`kernel/auth.rs:148-152`) resets to `NotRequired`; next inbound `AUTH` re-drives the handshake. Fail-closed semantics are enforced by ADR-0019 (`kernel/auth_fail_closed_tests.rs`) | OK (the FSM resets cleanly), but conditional on the relay actually sending a fresh `AUTH` |
| CLOSED vs transient distinguished | `tungstenite` `Error::Io` `WouldBlock`/`TimedOut` → continue; all other `Err` → `RelayEvent::Failed` → reconnect (`relay_worker.rs:159-171`). NIP-01 `CLOSED` frame is parsed by `kernel/ingest` but not surfaced as a sub-fatal vs transient distinction | **PARTIAL** — socket-level only |
| In-flight publish across reconnect | `pending: VecDeque<String>` held in the worker (`relay_worker.rs:82`) — if send fails the frame is `push_front`-ed back and reconnect resumes flushing. There is **no per-frame ack correlation** at the worker level | OK for blind enqueue; **WEAK** for delivery proof |
| Publish engine retry on reconnect | `publish::state::apply_ack` has the retry FSM (`publish/state.rs:213-329`) with `RetryPolicy { transient_max_retries: 3, backoff_base_ms: 1000, backoff_factor: 4 }` and `AckClass::AuthRequired` reauth — but the **engine is not driven by the actor**; `kernel/publish_cmd.rs:88-94` emits a single `EVENT` frame to `RelayRole::Content` and stamps `accepted_locally`. The retry FSM is dead code in production. | **ABSENT on the live path** (engine ships, not wired) |

A second pool, `subs::ConnectionPool` trait + `InMemoryPool` test impl
(`subs/pool.rs:34-148`), exists as the M11 reactivity bench's send-path —
production never instantiates it. `grep ConnectionPool crates/` finds only the
trait definition, doctrine prose, and test-only callers.

### 1.2 App lifecycle (iOS foreground/background)

| Surface | What ships | Verdict |
|---|---|---|
| Kernel FFI for foreground/background | None — `crates/nmp-core/src/ffi/mod.rs` exposes `nmp_app_start/stop/configure/reset/shutdown` only (`ffi/mod.rs:145-193`). No `nmp_app_set_foreground(bool)` or equivalent | **ABSENT** |
| Kernel trigger surface | `nmp-nip77::triggers::TriggerEvent::{Foreground, RelayReconnected, ViewOpenedWithGap}` (`nmp-nip77/src/triggers.rs:32-46`) — pure-data engine, never wired to the actor on master | **PARTIAL** — engine exists, no caller |
| Subs trigger surface | `subs::CompileTrigger` enum (`subs/trigger.rs`) — A1–A11 canonical triggers per `docs/builder-guide/27-discrepancies.md` row 10. Foreground-trigger is **not** one of them; foreground belongs to NIP-77 sync coverage, not subscription compilation. | OK by design |
| iOS NmpPulse shell wiring | `ios/NmpPulse/NmpPulse/NmpPulseApp.swift` — 13 lines, `WindowGroup { RootView() }`. `grep -rn scenePhase\|didBecomeActive\|willEnterForeground ios/NmpPulse` → **zero hits** | **ABSENT** end-to-end |
| iOS NmpPulse Bridge | `ios/NmpPulse/NmpPulse/Bridge/{KernelModel,KernelBridge}.swift` — only `reconnectCount: UInt32` is even mentioned (`KernelBridge.swift:254`), as a diagnostic counter, not a control surface | **ABSENT** |
| Documentation | The `triggers.rs` docstring documents the three sync triggers (`crates/nmp-nip77/src/triggers.rs:1-27`); nothing in `docs/ffi-surface.md` or `docs/builder-guide/` tells a shell author what to call on foreground | **ABSENT** |

### 1.3 Relay-purpose segregation

| Purpose | Notion in code | Live wire path | Verdict |
|---|---|---|---|
| Content vs Indexer split | `enum RelayRole { Content, Indexer }` (`relay.rs:15-19`); fixed URLs `wss://relay.primal.net` + `wss://purplepag.es` (`relay.rs:1-2`) | All view-open REQs route via `RelayRole` (e.g. `kernel/requests/{profile,thread}.rs`) | OK (but two fixed relays — not a "pool") |
| NIP-65 outbox routing | Planner Cases A/B (`planner/compiler/partition/{case_a_authors,case_b_addresses}.rs`); `RoutingSource::Nip65` (`planner/plan.rs:65`); `Nip65OutboxResolver` (`publish/nip65/mod.rs`) | **NOT** on the wire: row 1 of `docs/builder-guide/27-discrepancies.md` — kernel REQ path still routes to `CONTENT_RELAY_URL`; publish_cmd resolves NIP-65 then emits to `RelayRole::Content` only (`kernel/publish_cmd.rs:88-94`). T105 is in-progress wiring `CompiledPlan` → live REQ | **IN-PROGRESS(T105)** for REQ; **PARTIAL** for publish (resolves but does not fan out) |
| NIP-46 bunker signer relay | `Nip46Rpc::relays: Vec<String>` (`nmp-signers/src/signers/nip46/mod.rs:62-63`) — every RPC carries its own relay list; `Nip46Transport` trait (`:70-95`) is the seam to the kernel | The transport is **unwired**; `crates/nmp-core/src/actor/commands/account.rs` validates the `bunker://` URI shape but the signer has no relay pump (per `ActorCommand::SignInBunker` doc on `actor/mod.rs:46-48`). | **ABSENT (transport)** — no leak risk *because nothing routes yet* |
| NIP-29 group host pin | `InterestShape::relay_pin: Option<RelayUrl>` (`planner/interest.rs:140`), Case-E `case_e_relay_pinned` (`planner/compiler/partition/case_e_relay_pinned.rs`), lattice Rule 9 (`planner/lattice/rules.rs:160`) | Same row 1 verdict — pin lands in the compiled plan but no production path consumes the plan to emit per-relay REQs | **PARTIAL** (compiler ships, wire path doesn't) |
| NIP-50 search-relay set | `grep -rn "NIP-50\|nip50" crates/` → 0 hits; planner docstring mentions "global search" (`planner/compiler/partition/mod.rs:96`) as a future Case-C use, no shipped support | **ABSENT** |
| NIP-37 draft-only relays | `nmp-nip23/src/kinds.rs:6` references drafts as `nmp-nip37`'s domain; no `nmp-nip37` crate exists | **ABSENT** |
| General `ConnectionPool` abstraction | `subs::ConnectionPool` trait (`subs/pool.rs:34-54`) is the right shape (`send / mark_connected / mark_disconnected / drain_deferred`) — D7-compliant capability-reports-actor-decides | Trait exists; no production wiring — only `InMemoryPool` + tests reference it | **PARTIAL** — substrate ships, no production consumer |

---

## 2 — NDK vs applesauce vs NMP (live wire path)

NDK: `core/src/relay/{connectivity,pool/index,publisher,sub-manager}.ts`.
Applesauce: `packages/relay/src/{relay,pool}.ts`. NMP: today's master.

| Concern | NDK (`core/src/relay`) | applesauce (`packages/relay`) | NMP (live path on master) |
|---|---|---|---|
| Connection state machine | `DISCONNECTED → CONNECTING → CONNECTED → RECONNECTING → FLAPPING` (`connectivity.ts:311-381,662-671`) | `connected$` + `authenticated$` observables on `Relay` (`relay.ts` Rx) | `RelayEvent::{Connected,Failed,Closed,Message}` (`relay_worker.rs:11-30`) |
| Reconnect strategy | Exp backoff `min(1000·2^attempt, 30000)` (`connectivity.ts:732-745`); aggressive schedule `[0,1k,2k,5k,10k,30k]` after idle | Exp backoff `min(1.5^tries·1000, 300000)` cap 5 min (`relay.ts:1507`) | Flat 3 s (`relay_worker.rs:64`) |
| Max retries | **Removed** — retry forever w/ capped backoff (`connectivity.ts:8`) | Forever w/ cap | Forever w/ flat delay |
| Jitter | Not in cited lines; "flapping" detection acts as soft jitter | No | None |
| Stale-conn detection | 120 s keepalive ping + 5 s socket-readyState monitor (`connectivity.ts:64-93`) | REQ-ping every 29 s, EOSE within 20 s else `onUnresponsive` policy (`relay.ts:988-1039`) | Read timeout 50 ms loop only (`relay_worker.rs:63`); no app-layer ping |
| Live sub replay on reconnect | Subs persisted in `openSubs`; **manual** resubscribe required (per the NDK transcript) — `33e75950` "fix reconnection after sleep/wake" removed the 5-attempt cap | Subs persist via `req()` control flow (`relay.ts:1287-1326`); `finalize` waits for CLOSED | `SubscriptionLifecycle::handle_reconnect` ships (`subs/mod.rs:211-238`); **never called** by actor |
| AUTH on reconnect | `AuthRequiredError` → policy + `retryPendingAuthPublishes` (`connectivity.ts:559-614,819-830`) | `AuthRequiredError` caught in `req()`/`publish()`, waits for auth then retries (`relay.ts:1378-1389,1454-1457`) | `Nip42DriverState::reset_on_disconnect` (`kernel/auth.rs:150`) + ADR-0019 fail-closed (`auth_fail_closed_tests.rs`) — handshake re-drives; deferred REQs replayed via `AuthGate::record_transition` (`subs/mod.rs:254-260`) on the *next compile*, **not on reconnect** |
| Pool model | `NDKPool` holds permanent + temporary relays (`pool/index.ts:56,58,399`); separate `outboxPool` (`ndk/index.ts:450-456`); supports `relayConnectionFilter` blacklist (`pool/index.ts:155`) | Single `RelayPool` (`pool.ts:24`); no AUTH/signer isolation, no blacklist | Two hardcoded URLs per `RelayRole`; no pool object owns them; `subs::ConnectionPool` trait shipped but unused |
| Outbox isolation | Yes — dedicated pool for relay-list lookups | No — one pool | N/A (no pool) |
| Signer relay isolation | NIP-46 driven by NDK transport on app pool; no formal isolation but signer creates its own subscriptions | NIP-46 uses caller-supplied relays via `RelayPool` — no formal isolation | Trait `Nip46Transport` exists, no wiring — nothing leaks because nothing routes |
| Per-purpose pools | Outbox-pool + main is the only formal split | None | None |

NDK is the strongest of the three on connection lifecycle (indefinite reconnect,
flapping detection, NIP-46 retry-after-auth). Applesauce is the cleanest pure-Rx
relay-as-observable model. NMP today is the weakest on the *wire* (one flat
delay, no replay, no publish retry) but has the *richest substrate* (planner,
lifecycle, auth gate, publish FSM, NIP-77 trigger engine, connection-pool
trait) — all uniformly under-wired into the live actor path.

---

## 3 — Gap list, severity-ranked

| # | Gap | Severity | Owner / proposed Tnnn | Evidence |
|--:|---|---|---|---|
| G1 | `SubscriptionLifecycle::handle_reconnect` never called by actor — every reconnect loses live REQs until the next `OpenAuthor/Thread/...` re-emits them | **HIGH** | `Tnnn (proposed): wire-reconnect-replay` | `subs/mod.rs:211-238` vs `actor/dispatch.rs:241-252` |
| G2 | Outbox-on-wire (T105 / discrepancies row 1) — planner ships, REQ/EVENT path doesn't consume `CompiledPlan` | **HIGH** | T105 in-progress; track also `Tnnn (proposed): publish-outbox-fanout` for publish_cmd | `kernel/publish_cmd.rs:88-94`; `kernel/requests/{profile,thread}.rs` (RelayRole hardcode) |
| G3 | No app-lifecycle FFI: kernel can't be told "foreground happened" / "background happened"; NIP-77 trigger engine has nowhere to fire from | **HIGH** | `Tnnn (proposed): app-lifecycle-ffi` + Pulse shell wiring | `ffi/mod.rs:145-193`; `ios/NmpPulse/NmpPulseApp.swift` (no scenePhase) |
| G4 | Reconnect backoff is flat 3 s, no jitter, no cap-because-no-growth | **MED** (low *today* with 2 relays; **HIGH** the moment T105 lands and the relay set becomes user-configured) | `Tnnn (proposed): reconnect-backoff-policy` | `relay_worker.rs:64,197-217` |
| G5 | `publish::PublishEngine` retry FSM (`publish/state.rs:213-329`) is **dead** on the live path; one-shot `EVENT` to `RelayRole::Content` is the entire publish guarantee | **HIGH** | `Tnnn (proposed): wire-publish-engine` | `kernel/publish_cmd.rs:88-94` vs `publish/engine.rs` |
| G6 | NIP-46 transport is unwired — `Nip46Transport` trait exists, no relay-pump in actor; `bunker://` sign-in is URI-shape-only | **HIGH** for product; *not currently a leak risk* because nothing routes | `Tnnn (proposed): nip46-transport-wire` (must define bunker-relay isolation invariant before wiring) | `nmp-signers/src/signers/nip46/mod.rs:62-95`; `actor/mod.rs:46-48` |
| G7 | No app-layer keepalive ping; only a 50 ms read-timeout loop. iOS sleep/wake can leave a half-dead socket undetected for minutes | **MED** | `Tnnn (proposed): relay-keepalive-policy` | `relay_worker.rs:63,233-246` |
| G8 | `ConnectionPool` trait shipped but no production consumer; the substrate is ready for a per-relay pool but the actor uses per-`RelayRole` channels | **MED** | rolled into T105 + `Tnnn (proposed): connection-pool-prod-impl` | `subs/pool.rs:34-54` only referenced from tests |
| G9 | NIP-50 search-relay segregation absent (relays that advertise NIP-50 should be a separate routing lane) | **LOW** (no NIP-50 product yet) | `Tnnn (proposed): nip50-routing-lane` | grep → 0 hits |
| G10 | NIP-37 draft-only relay segregation absent (drafts MUST NOT go to public relays — D3 bug-class #4 analogue) | **LOW** (no draft product yet) — **HIGH** as a *spec deficit* the moment drafts ship | `Tnnn (proposed): nip37-draft-relay-set` | `nmp-nip23/src/kinds.rs:6` mentions, no crate |
| G11 | CLOSED-vs-transient: NIP-01 `CLOSED` is parsed by ingest but doesn't distinguish auth-pause from sub-fatal at the actor; deferred REQ machinery is on the AuthGate path only | **MED** | `Tnnn (proposed): closed-frame-classifier` | `kernel/ingest/*.rs` + `subs/auth_gate.rs` |
| G12 | No reconnect-storm protection (jitter / per-relay token bucket / global concurrency cap) once relay set is dynamic | **MED** (latent until T105) | folded into G4 | n/a (absence) |

---

## 4 — Recommended NMP design

**Doctrine-first framing.** Per D0 the kernel must not grow app nouns
(`docs/product-spec/overview-and-dx.md §1.5`). Per D7 the kernel exposes
capability reports; the shell (or a protocol crate) decides policy. The
recommendation respects both: kernel ships a **generic mechanism**, not a
catalogue of `RelayPurpose::Bunker`/`Search`/`Draft` enums.

### 4.1 The shape

1. **One `RelayPool` (singular) inside `nmp-core`**, owning per-relay
   workers, replacing per-`RelayRole` channels. Backed by the existing
   `subs::ConnectionPool` trait (`subs/pool.rs:34-54`). Production impl wraps a
   `tungstenite` worker per URL. Tests keep `InMemoryPool`.
2. **`RelayCapabilities`** report per relay (D7): `nip11_supported_nips`,
   `auth_required: bool`, `negentropy_supported: Option<bool>` (probed),
   `last_connected_at`, `reconnect_attempts`, `keepalive_ok`. The kernel
   *reports*; the planner/protocol crates *decide*.
3. **Routing-purpose remains in the planner**, not the pool. `RoutingSource`
   (`planner/plan.rs:55-72`) already encodes lane (NIP-65/Hint/Provenance/
   UserConfigured); the planner's `CompiledPlan` is the contract the pool
   executes. `relay_pin` (Case-E) and any future NIP-29/NIP-50/NIP-37 lanes are
   compiler partitions, not pool enums. The pool **does not know** that
   `wss://signer.example` is a bunker — only the NIP-46 protocol crate does,
   and it speaks to the pool via the same generic `send` / `subscribe` surface.
4. **Reconnect/backoff is the pool's policy** (no app nouns to escape D0):
   exponential `min(base·2^attempts, cap_ms)` with full-jitter, defaults
   `base=500ms, cap=30s` (mirrors NDK's 30s cap). Per-relay token bucket
   (e.g. 5 reconnects / 60 s window) for storm protection.
5. **Replay on reconnect.** On `RelayEvent::Connected`, the actor calls
   `SubscriptionLifecycle::handle_reconnect(url)` (already implemented at
   `subs/mod.rs:211-238`) and forwards the returned `WireFrame`s. The deferred
   queue (`pool::drain_deferred`) flushes after — order: REQ replay first,
   queued publishes second.
6. **AUTH on reconnect.** `Nip42DriverState::reset_on_disconnect` already
   wipes state. The actor must additionally tag any deferred AUTH-paused REQs
   so they fire only after the new handshake completes — `AuthGate` already
   has the mechanism (`subs/mod.rs:251-260`); just call it on the new
   `Authenticated` transition that follows reconnect.
7. **In-flight publish.** Per-frame ack correlation belongs in
   `publish::PublishEngine`; the actor must wire `publish_cmd.rs` through the
   engine instead of one-shotting an `EVENT`. The engine's retry FSM
   (`publish/state.rs:213-329`) already encodes the right policy; wiring
   restores it.
8. **App lifecycle contract between kernel and shell** (D7 capability
   surface, not policy):
   - Kernel exposes `nmp_app_set_foreground(bool)` FFI — purely a fact report.
   - On `false → true` transition the actor (a) calls `pool.kick_reconnects()`
     which is a no-op on connected relays and bypasses the backoff timer on
     disconnected ones; (b) fans `TriggerEvent::Foreground` through the NIP-77
     engine (`nmp-nip77/src/triggers.rs:32-46`). No native code decides what
     "foreground" means (D7): the shell tells the truth, the kernel reacts.
   - **Document the iOS contract** in `docs/ffi-surface.md`: scenePhase
     `.active` → `nmp_app_set_foreground(true)`; `.background` →
     `nmp_app_set_foreground(false)`. Pulse must wire this; until it does,
     foreground sync is a known gap.

### 4.2 Why a generic `RelayPool`, not `RelayPurpose` enums

A `RelayPurpose::{Bunker, Outbox, Draft, Search, Group}` enum would (a) violate
D0 (app nouns in the kernel), (b) duplicate `RoutingSource` (which already
expresses *why* a relay is in a plan), and (c) lock the kernel to today's NIP
list. The planner's four-lane discipline plus `relay_pin` (Case-E) is the
*correct* place to express purpose, because that is where filters are routed.
The pool's job is to keep N sockets up and bounce frames in FIFO; it does not
need to know the difference between a bunker frame and a kind:1 EVENT.

The **isolation invariant** (D3 bug-class #4 analogue: NIP-46 RPCs MUST NOT
leak to non-NIP-46 relays; NIP-37 drafts MUST NOT leak to public relays) is a
**planner** invariant, expressed today by `relay_pin` for NIP-29 and the same
mechanism for NIP-46 / NIP-37. A regression test in `nmp-testing` asserts
"every NIP-46 wire frame's `relay_url` is the signer's declared relay set" —
no enum needed.

### 4.3 Proposed task stubs (orchestrator picks Tnnn numbers)

- **`Tnnn-app-lifecycle-ffi`** — add `nmp_app_set_foreground(bool)`,
  document the iOS contract in `docs/ffi-surface.md`, fan
  `TriggerEvent::Foreground` through `nmp-nip77`. (G3)
- **`Tnnn-wire-reconnect-replay`** — actor calls
  `SubscriptionLifecycle::handle_reconnect(url)` on `RelayEvent::Connected`;
  add an integration test in `nmp-testing` that asserts post-reconnect REQs
  match pre-disconnect REQs. (G1)
- **`Tnnn-reconnect-backoff-policy`** — replace `RELAY_RECONNECT_DELAY` with
  full-jitter exponential `min(500·2^a, 30000)` + per-relay token bucket.
  Move backoff state into a `ReconnectPolicy` struct so tests can drive
  deterministically. (G4 + G12)
- **`Tnnn-relay-keepalive-policy`** — app-layer ping (REQ-EOSE probe or
  WebSocket ping/pong) with `onUnresponsive` → reconnect, default 60 s. (G7)
- **`Tnnn-wire-publish-engine`** — kernel publish path routes through
  `PublishEngine`; replay queued publishes on reconnect; per-relay OK
  correlation. Depends on T105 outbox-on-wire. (G5 + G8)
- **`Tnnn-nip46-transport-wire`** — implement `Nip46Transport` against the
  generic pool; document + test the isolation invariant (no NIP-46 frame
  reaches a non-NIP-46 relay). (G6)
- **`Tnnn-closed-frame-classifier`** — distinguish CLOSED:auth-required,
  CLOSED:rate-limited, CLOSED:fatal at the actor; route to AuthGate vs
  surface as RelayHealth diagnostic. (G11)
- **`Tnnn-nip50-routing-lane`** — planner partition for search-only
  interests (or `relay_pin` extension on `InterestShape`). (G9)
- **`Tnnn-nip37-draft-relay-set`** — analogous to NIP-46 isolation; spec the
  invariant *before* the first draft ships. (G10)

---

## 5 — Cite ledger (verified at master tip)

`crates/nmp-core/src/relay.rs:1-19`,
`crates/nmp-core/src/relay_worker.rs:11-30,63-65,159-171,197-217,233-246`,
`crates/nmp-core/src/actor/mod.rs:113-211`,
`crates/nmp-core/src/actor/dispatch.rs:230-272`,
`crates/nmp-core/src/actor/relay_mgmt.rs:39-127`,
`crates/nmp-core/src/subs/mod.rs:11,61,211-260`,
`crates/nmp-core/src/subs/pool.rs:34-148`,
`crates/nmp-core/src/subs/lifecycle_gate.rs:64-101`,
`crates/nmp-core/src/subs/wire.rs:51-89`,
`crates/nmp-core/src/kernel/auth.rs:148-196,322-328`,
`crates/nmp-core/src/kernel/auth_fail_closed_tests.rs:1-68`,
`crates/nmp-core/src/kernel/publish_cmd.rs:25-95`,
`crates/nmp-core/src/publish/state.rs:213-329`,
`crates/nmp-core/src/planner/interest.rs:104-140`,
`crates/nmp-core/src/planner/plan.rs:55-72`,
`crates/nmp-core/src/planner/compiler/partition/mod.rs:1-17,96`,
`crates/nmp-core/src/planner/compiler/partition/case_e_relay_pinned.rs:1-50`,
`crates/nmp-core/src/ffi/mod.rs:145-251`,
`crates/nmp-nip77/src/triggers.rs:32-132`,
`crates/nmp-signers/src/signers/nip46/mod.rs:1-95`,
`ios/NmpPulse/NmpPulse/NmpPulseApp.swift:1-13`,
`docs/builder-guide/27-discrepancies.md` row 1 (T105),
`docs/decisions/0019-failed-auth-fail-closed.md`,
NDK `core/src/relay/{connectivity,pool/index}.ts` (master, lines as cited in §2),
applesauce `packages/relay/src/{relay,pool}.ts` (master, lines as cited in §2).
