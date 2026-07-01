# NMP Crate Boundaries — Canonical Specification

> **Status:** decided architectural reference. Not a proposal, migration ladder,
> or progress report.

This document owns durable crate-boundary rules: which layer owns which
responsibility, which dependency directions are valid, and which seams are
allowed between layers. It does **not** own migration status, completed-step
history, active branches, PR state, or "what is currently being fixed" claims.
Temporal coordination belongs in GitHub Issues.

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

Plans are temporary. Once a migration has landed, this file keeps only the
resulting rule.

---

## 2. Layer Model

Dependencies flow from higher layers to lower layers unless a lower-layer
implementation depends on a higher-layer trait as an explicit dependency
inversion. In that case, the trait lives in the owner layer and the concrete
implementation is injected at composition time.

| Layer | Owns | Durable crate owners |
|---|---|---|
| 0 | Dependency-light vocabulary and interface types | `nmp-kinds`, `nmp-signer-iface`, `nmp-nip42-types`, `nmp-nip65-types`, `nmp-nip92-types`, `nmp-nip59`, `nmp-relay-url`, `nmp-nostr-id` |
| 1 | Storage, network transport, concrete signer transport | `nmp-store`, `nmp-nostr-lmdb`, `nmp-network`, `nmp-signers` |
| 2 | Routing and subscription planning algorithms | `nmp-router`, `nmp-planner` |
| 3 | Kernel substrate contracts and actor state | `nmp-core`, `nmp-coverage-gate` |
| 4 | Reusable Nostr protocol/product modules | `nmp-nip01`, `nmp-note-feed`, `nmp-feed-session`, `nmp-replies`, `nmp-nip02`, `nmp-nip17`, `nmp-nip18`, `nmp-nip29`, `nmp-nip42`, `nmp-nip47`, `nmp-nip51`, `nmp-nip57`, `nmp-nip60`, `nmp-nip77`, `nmp-nwc`, `nmp-marmot`, `nmp-threading`, `nmp-feed`, `nmp-wot`, `nmp-content`, `nmp-content-fixtures` |
| 5 | App composition | `apps/<app>/...` Rust crates and runtime builders that explicitly compose substrate/protocol/app features |
| 6 | Platform runtimes, bindings, and deliverables | `nmp-native-runtime`, `nmp-uniffi`, `nmp-browser-runtime`, app-owned delivery crates |
| Sidecars | Tooling, tests, diagnostics | `nmp-cli`, `nmp-codegen`, `nmp-testing`, app shells |

Sibling crates do not depend on each other unless the dependency is part of
their declared responsibility. Binding crates are siblings: one binding crate
must not depend on another binding crate for business behavior. If a durable
crate owner named here has not yet been extracted in code, the current owner is
migration debt tracked in GitHub Issues, not an exception to this graph.

`nmp-signer-iface` (Layer 0) owns the dependency-light signing substrate
vocabulary so lower-layer signer and protocol crates can name it without
depending on the kernel: the NIP-01 event value types `SignedEvent` /
`UnsignedEvent` / `SigningError`, the `SignerOp` / `SignerError` op vocabulary,
the NIP-46 `Nip46Rpc` / `Nip46Transport` and NIP-55 external-signer transport
contracts, and the actor-facing `RemoteSignerHandle` trait. `nmp-signers`
(Layer 1) depends only on `nmp-signer-iface` for this vocabulary, not on
`nmp-core` (issue #1720). `nmp-core` temporarily re-exports `SignedEvent` /
`UnsignedEvent` / `SigningError` through `nmp_core::substrate` and
`RemoteSignerHandle` at the crate root so the ~94 existing kernel-side and
protocol-crate import paths keep resolving; that re-export is a staged
migration aid, not a durable seam — issue #1772 tracks migrating every
remaining importer onto direct `nmp_signer_iface` imports and deleting the
re-exports. The type owner is `nmp-signer-iface`.

`nmp-relay-url` (Layer 0) owns the dependency-free relay-URL canonicalization
vocabulary: the single `canonicalize(&str) -> Option<String>` authority that
normalizes a `ws`/`wss` relay URL (lowercase scheme+host, strip empty-path
trailing slash, fail-closed on a non-ws/wss or hostless URL). A relay URL is the
key the transport pool, the routing/mailbox caches, and the blocked-relay filter
hand each other, so all of them — `nmp-network` (L1), `nmp-router` /
`nmp-planner` (L2), `nmp-core` (L3, re-exported as
`nmp_core::substrate::canonicalize_relay_url`), and protocol crates such as
`nmp-nip17` (L4) — depend on this one crate rather than each re-implementing the
rules. This centralizes the five previously drifting copies (#967). The type/authority owner is
`nmp-relay-url`.

---

## 3. Kernel Substrate

`nmp-core` owns substrate contracts and actor-owned state:

- Actor loop, `Kernel`, `KernelReducer`, `KernelUpdate`, monotonic `rev`.
- Capability sockets and raw capability-result intake.
- Session/account state and active-account switching.
- Trait seams: `ActionModule`, `ProtocolCommand`, `IngestParser`,
  `EventIngestDispatcher`, `ObservedProjectionSink` delivery slots (internal
  plumbing activated only by declared `ObservedProjectionRegistrar` sessions),
  `ExternalEventSinkPolicy`
  (the internal in-process relay-forwarding seam; replaces the retired
  `RawEventObserver` / `RawEventForwardPolicy` pair — there is no native push
  sink),
  `OutboxRouter`, `MailboxCache`, `PaymentPort` (the BOLT-11 pay-invoice seam:
  NIP-57 emits a typed `PaymentIntent`, NIP-47 supplies the implementation, so
  there is no `nmp-nip57 → nmp-nip47` sibling edge), and publish resolver
  traits.
- Snapshot/update envelopes, projection registry, and generic transport
  machinery.
- Shared display helpers as render-side utilities, not projection-builder
  policy.

`nmp-core` must not grow new protocol-specific parsers, routing algorithms,
action bodies, or app-specific nouns. If an existing protocol-shaped exception
remains, it belongs in a GitHub issue with a code citation and removal path.

NIP-50 search follows the same split. `nmp-core` owns the generic search/index
**seams**: the bounded `InterestShape.search` wire-filter field, filter
serialization, merge equality, diagnostics, cache-coverage refusal, the
`substrate::search` module (`SearchScopeRegistrar` / `SearchScopeProvider` /
`SearchScopeRegistry`) that protocol crates populate at composition time and
the kernel compiles into `nmp-store::CompiledIndexSpec` (the cache-serve hook),
and the account-config self-kind bootstrap including kind:10007 in
`SELF_KINDS_TAILING` (`crates/nmp-core/src/kernel/requests/startup.rs`).
`nmp-core` does **not** own NIP-50 query semantics, relay-selection policy,
result ranking, domain target classes, or result projection — those belong to
`nmp-nip50`. NIP-51 kind:10007 relay-list parsing belongs to `nmp-nip51`
(`SearchRelayListProjection`). There is no `EventClass::Search` variant.

> **Label-vocabulary drift (known, minor).** The scope labels `nip50.profiles`,
> `nip50.notes`, and `nip50.longform` are defined as named constants in
> `crates/nmp-nip50/src/scopes.rs` (`SCOPE_LABEL_PROFILES`,
> `SCOPE_LABEL_NOTES`, `SCOPE_LABEL_LONGFORM`) but the name parts
> (`"profiles"`, `"notes"`, `"longform"`) are re-hardcoded as bare string
> literals in `crates/nmp-intent/src/classifier/text.rs` rather than imported
> from those constants. Centralising the name-part constants (or importing the
> existing ones via the `nmp-nip50` dep that `nmp-intent` already carries) is a
> follow-up cleanup; it is not a correctness bug but is a known small duplication
> to eliminate before the scope set grows.

---

## 4. Router Ownership

`nmp-router` is the single home for NIP-65 mailbox routing and generic relay
selection. There is no standalone routing/action-owning `nmp-nip65` crate in
this architecture. The dependency-light `nmp-nip65-types` crate owns only the
canonical kind:10002 tag decoder so router and test fixtures cannot drift. The
action namespace `nmp.nip65.publish_relay_list` remains byte-stable for callers,
but its implementation belongs to `nmp-router`.

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

The planner also does not own feed-source semantics. A ReducedSource such as
active-user follows, a public people list, a mute list, or a follow pack is
declared and reduced in app/protocol/defaults composition before planner input.
`nmp-planner` consumes the resulting `LogicalInterest`/`InterestShape` data and
may coalesce authors, tags, ids, and addresses, but it must not name the source
domain that produced them.

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

- `nmp-nip01` owns base note/profile/reply primitives: the kind:1 note
  builder/decoder, reply/thread views, kind:0 profile + kind:3 contacts caches,
  and NIP-10 timeline grouping. It does not own concrete feed rows, render
  payloads, social/action-row aggregation, or OP-feed wire. Counts, loading
  state, and teardown for replies, reactions, reposts, zaps, bookmarks, mutes,
  and other markers belong to the concept crate that defines that behavior.
- `nmp-note-feed` owns concrete note-feed composition: OP/flat feed rows,
  typed note-feed wire, repost row composition, and reusable feed projection
  mechanisms.
  It composes `nmp-nip01` kind:1/NIP-10 facts, `nmp-nip18` repost facts,
  `nmp-content` content trees, and `nmp-feed` mechanics; it does not own
  relation-count concepts or app render policy.
- `nmp-feed-session` owns runtime-independent feed-session compilation:
  mapping reusable feed declarations to source graphs, controller registration
  plans, and session-scoped dependency re-resolution. App-owned projection keys
  and product feed meaning remain in the composing app/runtime.
- `nmp-replies` owns app-facing reply policy and read planning: a `ReplyTarget`
  plus content becomes either a NIP-10 kind:1 note or a NIP-22 kind:1111
  comment. Apps do not choose tag names, NIP-10 markers, NIP-22 root scopes, or
  kind:1-vs-kind:1111; protocol crates supply the lower-level builders and
  decoders.
- There is no central `nmp-relations` owner. Reactions, reposts, zaps,
  replies/comments, bookmarks, mutes, and app-specific markers belong to the
  concept crate that defines their semantics. Cross-protocol social bars are
  app/composition recipes over those concept-owned active reads, not reusable
  framework buckets.
- `nmp-nip17` owns NIP-17 DM send/receive behavior and its DM relay-list cache.
- `nmp-nip57` owns zap request/receipt and LNURL zap action behavior. It pays
  through the substrate `PaymentPort` (it emits a typed `PaymentIntent`); it does
  not depend on `nmp-nip47`.
- `nmp-nip47` owns NIP-47 NWC wallet runtime and supplies the `PaymentPort`
  implementation (`WalletPaymentPort`) injected into the zap chain at
  composition time.
- `nmp-nip59` is the Layer-0 gift-wrap primitive crate (pure seal/wrap/unwrap
  over the `nostr` crate; only a `nmp-kinds` workspace dep). NIP-17 and Marmot
  re-use it for DM and MLS-Welcome delivery; `nmp-core` has no production
  dependency on it (the former `SendGiftWrappedDm` kernel arm was deleted).
- `nmp-marmot` owns Marmot/MLS group behavior.
- `nmp-feed` and `nmp-threading` are reusable algorithms used by protocol
  modules; they carry no app-specific shell behavior.
- `nmp-content` owns content parsing/render substrate, not link-preview network
  fetching or app navigation.

If a feature would be useful to a different Nostr app, it belongs in an NMP
crate. If it is specific to one app's product domain, it belongs under
`apps/<app>/`.

"Useful to another app" means usable unchanged as a generic Nostr mechanism,
not merely plausible future reuse. A single app's feature request does not
justify app-shaped code in a shared crate. Shared NMP crates must not gain
app-named commands, bespoke projection shapes, hard-coded product defaults,
operator policy, or temporary compatibility paths for one consumer. When a
shared crate needs to help an app, the acceptable shape is a reusable substrate
seam or protocol mechanism that other apps can compose. Otherwise the work
stays in the leaf app's Rust crate.

### App-private event kinds (#2408)

App-private Nostr kinds are first-class app code, not failed protocol crates.
If a kind's semantics, tag policy, validation, publish intent, or read model
only make sense for one product, its schema and action contract live beside the
leaf app Rust crate (`apps/<app>/...` in this repository, or the external app's
Rust crate). They do not move under `crates/` merely to receive typed builders,
native/web bindings, or drift checks.

The accepted #2408 boundary is:

- The app Rust crate owns the event construction semantics, validation rules,
  tag policy, publish intent, app-private state, and `ActionModule`.
- NMP owns the reusable substrate and tooling: `ActionModule` /
  `ActionRegistrar`, `DispatchEnvelope`, signing/publish routing, typed action
  builder generation, Swift/Kotlin/TypeScript binding generation, and
  correctness/drift gates.
- The app-private contract is app-local codegen input. It is not a global NMP
  registry entry and not a runtime-loaded schema.

An app-private action/kind contract must identify the action namespace stamped
into `DispatchEnvelope.action_namespace`; the event kind number and whether the
action publishes an event or only starts app work; the FlatBuffers schema path,
root type, file identifier, schema id, and schema version; the generated builder
method name and flat-table field list/order; the owning Rust crate/module/type
names for the app's `ActionPayload` and `ActionModule`; the Swift, Kotlin, and
TypeScript builder output targets; and the drift/check commands the app runs in
CI.

This lane deliberately excludes plugin-platform features: no runtime schema
loading, no plugin marketplace or dynamic module discovery, no generated
composition root, no generic tag ontology, and no automatic read-model or
projection generation. Reusable protocols still graduate to Layer 4 NMP crates;
app-private kinds stay app-owned while using NMP tooling.

---

## 9. App Composition

App/runtime composition roots install the substrate and protocol features they
need explicitly. The deleted defaults bundle is not a current composition
target, not a test helper, and not a compatibility shim to recreate under a new
name.

`nmp-substrate` is the reusable shared floor for router/mailbox/profile/contacts
cache-parser construction. Reducer-owned delivery roots that cannot implement
the full AppHost tier (currently Chirp web) must call `nmp-substrate`; they must
not hand-copy that construction. Above that floor, app/runtime roots compose
protocol crates and app-owned features by name.

Composition roots may name `nmp_core::substrate::AppHost` and the narrow
registrar traits below it. Reusable protocol crates take only the narrow traits
they use. No reusable substrate/protocol crate may depend on platform runtime
crates, name `NmpApp`, own a platform runtime handle, export a native builder,
or hide app policy behind a preset.

Shared crates **must not own operator policy facts** — relay URLs,
nostrconnect bootstrap relay URLs, seed pubkeys, account auto-follow lists, or
signer permission batches. Those facts belong only in leaf app Rust crates
(`apps/<app>/...`, e.g. `apps/chirp/crates/nmp-chirp-config`) or
operator-provided app config (#1493). The native and browser runtime builders
enforce this at compile time: an app must declare its initial relay set with
`.with_relays(...)` or explicitly opt out with `.without_initial_relays()`
before `start()` — there is no framework relay default to inherit silently.

App crates under `apps/<app>/` compose substrate/protocol features plus
app-specific state **and own all operator policy** (relays, seed follows,
signer perms).
They may expose app-specific delivery helpers only for kernel-shaped observer,
projection, opaque-handle, or lifecycle seams. Those helpers are app-owned glue,
not reusable framework ABI. Mutating product behavior should flow through
registered actions or protocol commands.

Native platform shells render Rust-owned state and execute capabilities only;
they never carry operator policy (relay URLs, seed pubkeys) — that originates
in the leaf app's Rust crate and is injected by app-owned FFI.

Native shells also never expand ReducedSources or dependent interests. They do
not compute follow/list membership, construct dynamic author filters, run
meta-subscribe fetch cascades, or hydrate profiles/events outside Rust-owned
ref/dependent-interest lifecycles.

---

## 10. Binding Crates

`nmp-native-runtime`, `nmp-uniffi`, and `nmp-browser-runtime` are the reusable
framework delivery surfaces. Runtime adapter crates own platform runtime
lifecycle and typed builders; binding crates own binding shape, byte conversion,
panic guards, callbacks, lifecycle handle exposure, and platform-specific bridge
mechanics. (`nmp-wasm` was deleted in #2202 — it was a dead parallel browser
runtime with zero live dependents; its protocol types are now owned by
`nmp-browser-runtime`.) These crates do not own business policy, app defaults, or
example-app namespaces unless they are explicitly app-owned delivery crates.

Native target split (#2205/#2209, amended by M14):

- `nmp-native-runtime` is the native platform runtime adapter. It owns the
  native `NmpApp`/handle type, actor-thread lifecycle, native runtime slots,
  session registries, native Rust APIs, and the native typestate builder
  (`NmpAppBuilder` / `RunConfig`). It composes `nmp-substrate` and protocol
  installers explicitly like a leaf app runtime.
- `nmp-uniffi` is the reusable framework native binding surface for iOS,
  Android, and desktop native hosts. It exposes the framework runtime object
  model, typed records, callbacks, and FlatBuffers byte payload doorways through
  generated bindings.
- App-owned UniFFI facade crates may expose app-specific protocol verbs in the
  app's generated namespace. They must delegate lifecycle, update-sink,
  capability, dispatch, quiescence, and clamp mechanics to `nmp-native-runtime`
  / `nmp-uniffi-support` instead of copying framework bridge policy or reviving
  raw C/JNI symbols.
- App-owned delivery crates may keep local C/JNI glue for app-specific adapters
  such as Gallery, but that glue is not reusable framework API and must not
  revive deleted framework symbols.

`nmp-browser-runtime` is the browser composition-root delivery surface
described in §10a. It owns the wasm-bindgen Worker export
(`nmp-browser-runtime::wasm` is the sole browser ABI glue) and the serializable
browser Worker protocol types.

The pre-v1 binding surface is governed, not compatibility-frozen. Net-new
framework native APIs target UniFFI, not raw exported C/JNI symbols. Renames and
deletions that collapse legacy wrappers, dead parameters, app-named generic
surfaces, or duplicate paths are preferred over compatibility aliases. Temporary
retention of app-owned raw glue requires a staged GitHub issue with a deletion
gate when it affects reusable framework behavior.

---

## 10a. Browser Platform Adapter (nmp-browser-runtime)

`nmp-browser-runtime` is the browser platform adapter per ADR-0067: a Layer-6
runtime adapter, sibling to `nmp-native-runtime`. Unlike pure ABI-glue binding
crates, it is a **composition root**: it composes `nmp-substrate` and protocol
crates into a typed builder (`BrowserAppBuilder`), exactly as a native runtime
does. It thus may depend on the substrate/protocol composition surface needed
to start the browser runtime, breaking the usual binding-crate rule that all
siblings avoid each other.

`nmp-browser-runtime` owns:

- The Worker event-loop runtime driving a single `KernelReducer` (D4).
- The browser WebSocket transport adapter (transport bridge only; no policy).
- Browser storage initialization and lifecycle.
- Capability provider registry and browser signer provider mapping.
- Browser timer and clock seams for `nmp-core` injection.
- The typed `BrowserAppBuilder` composition root (browser twin of the native
  runtime's `NmpAppBuilder`).
- The wasm-bindgen Worker ABI surface (`NmpWasmRuntime`) and JS callback
  registration.

`nmp-browser-runtime` must not own:

- Routing or outbox policy (that is `nmp-router` / kernel).
- Signing policy or signer-provider semantics (that is `nmp-signers`).
- NIP modules, protocol defaults, app defaults, projection policy, persistence policy.

Dependency direction: `nmp-browser-runtime` depends on `nmp-substrate` and
protocol crates in Layers 0–5. Leaf web apps depend on `nmp-browser-runtime`
for the typed builder and Worker export. No Layer 0-5 crate depends on
`nmp-browser-runtime`.

### Shared composition target: reuse `AppHost` (#2059, ADR-0067)

`BrowserAppBuilder` composes through the **existing** `nmp_core::substrate::AppHost`
super-trait — no browser-specific composition trait is introduced. `AppHost` is
already platform-neutral: it is the blanket-impl union of the narrow D6 registrar
traits, every method registers a Rust-owned fact (action modules, ingest parsers,
snapshot projections, declared observed projections, routing/publish factories,
kernel-reader slots, capability seams), and platform capabilities (storage, sockets, OS
keychains) are deliberately excluded. Native (`nmp-native-runtime::NmpAppBuilder`)
and browser (`BrowserAppBuilder`) implement the same narrow registrars and obtain
`AppHost` through the blanket impl; only a composition root names `AppHost`,
while protocol modules continue to take the narrow trait(s) they use.

Layer ownership of registered facts (all browser-relevant; none native-only):

- Action modules / protocol commands → protocol crates (Layers 1–4) via `ActionRegistrar`.
- Ingest parsers and kernel-reader slots (profile / contacts / mailbox / DM-inbox / blocked-relay) → protocol crates; the reducer reads, never names the wire format (D0).
- Snapshot projections, observed-projection sinks, identity-change hooks → runtime crates.
- Routing / publish / raw-forward factories, nostrconnect bootstrap, relay User-Agent, outbound tags → `nmp-router` + composition root.
- `HostCapabilities` (active pubkey, actor command sender, configured-relays slot, preferred-relay source) → composition root / kernel.

Two implementation seams are deferred to their owning issues (they do **not**
justify a narrower trait):

1. **Command sender (#2046 / #2057).** `HostCapabilities::actor_sender()` returns a
   `CommandSender`, whose constructor is currently `feature = "native"`-gated.
   `nmp-browser-runtime` owns the single-writer Worker loop driving the
   `KernelReducer` (D4), so it constructs the inbox and `CommandSender` itself;
   the resolution is a wasm-safe headless inbox constructor in `nmp-core` plus the
   builder's Worker loop — not a trait change.
2. **wasm-safe installers (#2047 / #2060).** Browser delivery roots that cannot
   yet implement the full AppHost tier use the `nmp-substrate` floor
   (§9). Full explicit feature composition requires a browser runtime handle
   that can supply the same AppHost-rooted registrar surface as the native
   runtime.

---

## 11. Change Policy

When a crate-boundary rule changes:

1. Update the durable owner document: this file, a product spec, or an ADR.
2. Update or create GitHub Issues only for live work still required.
3. Do not add a parallel plan, review dump, or architecture ladder.
4. Do not preserve completed migration history here. Git history already does
   that.
