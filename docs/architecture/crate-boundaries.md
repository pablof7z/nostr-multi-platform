# NMP Crate Boundaries — Canonical Specification

> **Status:** decided architectural reference. Not a proposal, migration ladder,
> or progress report.

This document owns durable crate-boundary rules: which layer owns which
responsibility, which dependency directions are valid, and which seams are
allowed between layers. It does **not** own migration status, completed-step
history, active branches, PR state, or "what is currently being fixed" claims.
Temporal coordination belongs in `docs/plan.md`, GitHub Issues, and the
ignored live `WIP.md` tracker.

If this document disagrees with code, ADRs, or doctrine, fix the single source
of truth that owns the concept. Do not create a second crate-boundary plan.

---

## 1. Source Of Authority

- `AGENTS.md` owns repository-wide contribution doctrine, file-size rules,
  planning discipline, and the NMP-vs-app-specific crate test.
- `docs/product-spec/doctrine.md` owns durable doctrine.
- `docs/decisions/` owns accepted architectural decisions.
- This file owns the durable crate graph and crate responsibility rules.
- GitHub Issues own unresolved violations and queued work.
- `WIP.md` owns active branch/worktree coordination only.

Plans are temporary. Once a migration has landed, this file keeps only the
resulting rule.

---

## 2. Layer Model

Dependencies flow from higher layers to lower layers unless a lower-layer
implementation depends on a higher-layer trait as an explicit dependency
inversion. In that case, the trait lives in the owner layer and the concrete
implementation is injected at composition time.

| Layer | Owns | Current crates |
|---|---|---|
| 0 | Dependency-light vocabulary and interface types | `nmp-kinds`, `nmp-signer-iface`, `nmp-nip42-types` |
| 1 | Storage, network transport, concrete signer transport | `nmp-store`, `nmp-nostr-lmdb`, `nmp-network`, `nmp-signers`, `nmp-signer-broker` |
| 2 | Routing and subscription planning algorithms | `nmp-router`, `nmp-planner` |
| 3 | Kernel substrate contracts and actor state | `nmp-core`, `nmp-coverage-gate` |
| 4 | Reusable Nostr protocol/product modules | `nmp-nip01`, `nmp-nip02`, `nmp-nip17`, `nmp-nip18`, `nmp-nip29`, `nmp-nip42`, `nmp-nip47`, `nmp-nip51`, `nmp-nip57`, `nmp-nip59`, `nmp-nip60`, `nmp-nip77`, `nmp-nwc`, `nmp-marmot`, `nmp-threading`, `nmp-feed`, `nmp-wot`, `nmp-content`, `nmp-content-fixtures` |
| 5 | App composition | `nmp-defaults`, `apps/<app>/...` Rust crates |
| 6 | Bindings and deliverables | `nmp-ffi`, `nmp-android-ffi`, `nmp-wasm` |
| Sidecars | Tooling, tests, diagnostics | `nmp-cli`, `nmp-codegen`, `nmp-testing`, app shells |

Sibling crates do not depend on each other unless the dependency is part of
their declared responsibility. Binding crates are siblings: one binding crate
must not depend on another binding crate for business behavior.

---

## 3. Kernel Substrate

`nmp-core` owns substrate contracts and actor-owned state:

- Actor loop, `Kernel`, `KernelReducer`, `KernelUpdate`, monotonic `rev`.
- Capability sockets and raw capability-result intake.
- Session/account state and active-account switching.
- Trait seams: `ActionModule`, `ProtocolCommand`, `IngestParser`,
  `EventIngestDispatcher`, `KernelEventObserver`, `ExternalEventSinkPolicy`
  (the internal in-process relay-forwarding seam; replaces the retired
  `RawEventObserver` / `RawEventForwardPolicy` pair — there is no native push
  sink),
  `OutboxRouter`, `MailboxCache`, and publish resolver traits.
- Snapshot/update envelopes, projection registry, and generic transport
  machinery.
- Shared display helpers as render-side utilities, not projection-builder
  policy.

`nmp-core` must not grow new protocol-specific parsers, routing algorithms,
action bodies, or app-specific nouns. If an existing protocol-shaped exception
remains, it belongs in a GitHub issue with a code citation and removal path.

NIP-50 search follows the same split: core/planner may carry the generic
bounded `search` wire-filter field, filter serialization, merge equality,
diagnostics, and cache-coverage refusal. Query semantics, app-facing search
actions/views, ranking, and result projection belong to an owning search
crate/module such as `nmp-nip50`. NIP-51 kind:10007 relay-list facts belong to
`nmp-nip51`, not to the generic router.

---

## 4. Router Ownership

`nmp-router` is the single home for NIP-65 mailbox routing and generic relay
selection. There is no standalone `nmp-nip65` crate in this architecture.
The action namespace `nmp.nip65.publish_relay_list` remains byte-stable for
callers, but its implementation belongs to `nmp-router`.

`nmp-router` owns:

- `GenericOutboxRouter`.
- `InMemoryMailboxCache`.
- `Kind10002Parser`, the single writer for the substrate `MailboxCache`.
- `PublishRelayListAction` under `nmp.nip65.publish_relay_list`.
- `Nip65OutboxResolver`, the production publish-side resolver.
- `IndexerRepublishPolicy`.
- Blocked-relay parsing and post-filtering when wired by composition.

`nmp-core` owns the traits that `nmp-router` implements. That compile-time edge
(`nmp-router -> nmp-core`) is intentional dependency inversion: the kernel sees
`Arc<dyn OutboxRouter>` / `Arc<dyn MailboxCache>` injected by composition, never
a concrete router dependency.

---

## 5. Routing Contract

Routing is one generic automatic algorithm. NIP crates do not register routing
rules, and the outbox router does not carry a manual relay override seam.

The generic router may consult:

- Event kind for discovery/indexer eligibility.
- Event author and tags.
- NIP-65 mailbox data through `MailboxCache`.
- Session read/write/app/indexer relay configuration.
- Relay hints, provenance, p-tag inbox hints, and blocked-relay state.

When a publish flow already knows the correct relay set, it uses
`PublishTarget::Explicit { relays }` in the publish engine. That path is the
single explicit-relay mechanism for NIP-17 DM relay lists, NIP-29 group host
relays, Marmot MLS group relays, and other audited D3 opt-outs. The outbox
router remains the automatic routing seam rather than a second explicit
publish path.

The router never owns socket lifecycle. It returns relay decisions; the actor
uses `nmp-network` to open and send.

---

## 6. Planner Ownership

`nmp-planner` owns subscription compilation:

- Logical interests and interest coalescing.
- Per-relay filter projection.
- Plan diffing and compile triggers.
- Selection policy that bounds fan-out while preserving author coverage.
- Read-only score lookup inputs used by claim expansion.

The planner does not own relay sockets, event persistence, or protocol-specific
parsing. Its output is data for the kernel actor to execute.

---

## 7. Network And Store Ownership

`nmp-network` owns WebSocket I/O and pool lifecycle only:

- `Pool`, `RelayHandle`, `PoolEvent`, reconnect/backoff, health snapshots,
  wire frames, and push delivery to the actor.
- No routing, subscription planning, Nostr event policy, or "send to all"
  convenience method.

`nmp-store` owns the `EventStore` contract and shared store-facing helpers.
`nmp-nostr-lmdb` owns the LMDB backend. Replaceable-event, deletion, provenance,
and index invariants belong behind the store contract, not in app shells.

---

## 8. Protocol Crates

A Layer-4 crate owns the full reusable product/protocol module for its domain:
builders, decoders, projections, action modules, protocol commands, and
registered ingest parsers. A protocol crate may depend on substrate traits, but
the substrate must not depend on the protocol crate's concrete module logic.

Examples:

- `nmp-nip17` owns NIP-17 DM send/receive behavior and its DM relay-list cache.
- `nmp-nip57` owns zap request/receipt and LNURL zap action behavior.
- `nmp-marmot` owns Marmot/MLS group behavior.
- `nmp-feed` and `nmp-threading` are reusable algorithms used by protocol
  modules; they carry no app-specific shell behavior.
- `nmp-content` owns content parsing/render substrate, not link-preview network
  fetching or app navigation.

If a feature would be useful to a different Nostr app, it belongs in an NMP
crate. If it is specific to one app's product domain, it belongs under
`apps/<app>/`.

---

## 9. App Composition

`nmp-defaults` is a reusable NMP composition library, **not a leaf
application**. It wires generic NMP mechanisms — the default router, planner,
store, ingest parsers, action modules, coverage hook, raw-event forwarding
policies, default projections, and typed seams. Reducer-owned delivery roots
that cannot implement the full AppHost tier (currently Chirp web) must call
`nmp-substrate-defaults` for the shared router/mailbox/profile/contacts
cache-parser floor; they must not hand-copy that construction.

`nmp-defaults` (like `nmp-core` and every other NMP crate) **must not own
operator policy facts** — relay URLs, nostrconnect bootstrap relay URLs, seed
pubkeys, account auto-follow lists, or signer permission batches. Those facts
belong only in leaf app Rust crates (`apps/<app>/...`, e.g.
`apps/chirp/nmp-chirp-config`) or operator-provided app config (#1493). The
`NmpAppBuilder` enforces this at compile time: an app must declare its initial
relay set with `.with_relays(...)` or explicitly opt out with
`.without_initial_relays()` before `start()` — there is no framework relay
default to inherit silently.

App crates under `apps/<app>/` compose `nmp-defaults` plus app-specific
state **and own all operator policy** (relays, seed follows, signer perms).
They may expose app-specific FFI helpers only for kernel-shaped observer,
projection, opaque-handle, or lifecycle seams — including thin wrappers that
inject the app's own operator policy into a generic command (e.g.
`nmp_app_chirp_create_new_account` threading `chirp_default_follows` into
`ActorCommand::CreateAccount`, mirroring `nmp_app_chirp_seed_default_relays`).
Mutating product behavior should flow through registered actions or protocol
commands.

Native platform shells render Rust-owned state and execute capabilities only;
they never carry operator policy (relay URLs, seed pubkeys) — that originates
in the leaf app's Rust crate and is injected by app-owned FFI.

---

## 10. Binding Crates

`nmp-ffi`, `nmp-android-ffi`, and `nmp-wasm` are delivery surfaces. They own ABI
shape, panic guards, callbacks, lifecycle handles, and platform-specific bridge
mechanics. They do not own business policy, app defaults, or example-app
namespaces unless they are explicitly app-owned delivery crates.

The pre-v1 ABI surface is governed, not compatibility-frozen. Net-new
`nmp_app_*` symbols require an ADR or an accepted GitHub issue that explicitly
explains why the generic action, projection, or capability seam is insufficient.
Renames and deletions that collapse legacy wrappers, dead parameters, app-named
generic surfaces, or duplicate paths are preferred over compatibility aliases.
Temporary retention requires a staged GitHub issue with a deletion gate.

---

## 11. Change Policy

When a crate-boundary rule changes:

1. Update the durable owner document: this file, a product spec, or an ADR.
2. Update or create GitHub Issues only for live work still required.
3. Do not add a parallel plan, review dump, or architecture ladder.
4. Do not preserve completed migration history here. Git history already does
   that.
