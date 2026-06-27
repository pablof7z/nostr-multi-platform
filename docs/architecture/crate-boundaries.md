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
| 0 | Dependency-light vocabulary and interface types | `nmp-kinds`, `nmp-signer-iface`, `nmp-nip42-types`, `nmp-nip92-types`, `nmp-nip59`, `nmp-relay-url` |
| 1 | Storage, network transport, concrete signer transport | `nmp-store`, `nmp-nostr-lmdb`, `nmp-network`, `nmp-signers` |
| 2 | Routing and subscription planning algorithms | `nmp-router`, `nmp-planner` |
| 3 | Kernel substrate contracts and actor state | `nmp-core`, `nmp-coverage-gate` |
| 4 | Reusable Nostr protocol/product modules | `nmp-nip01`, `nmp-nip02`, `nmp-nip17`, `nmp-nip18`, `nmp-nip29`, `nmp-nip42`, `nmp-nip47`, `nmp-nip51`, `nmp-nip57`, `nmp-nip60`, `nmp-nip77`, `nmp-nwc`, `nmp-marmot`, `nmp-relations`, `nmp-threading`, `nmp-feed`, `nmp-wot`, `nmp-content`, `nmp-content-fixtures` |
| 5 | App composition | `nmp-defaults`, `apps/<app>/...` Rust crates |
| 6 | Platform runtimes, bindings, and deliverables | `nmp-native-runtime`, `nmp-ffi`, `nmp-android-ffi`, `nmp-browser-runtime`, `nmp-wasm` |
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
  the note timeline/OP-feed surface (NIP-18 reposts appear in that feed as
  boosted notes — base note-feed rendering, not cross-protocol aggregation), the
  relation-count vocabulary (`NoteRelationCounts`), and the
  `NoteRelationClassifier` seam. It does NOT own cross-protocol engagement
  aggregation.
- `nmp-relations` owns cross-protocol social-relation aggregation: the
  `DefaultNoteRelationClassifier` that tallies reactions (NIP-25), reposts
  (NIP-18), zaps (NIP-57), and comments (NIP-22) onto a note, and the
  `nmp.nip01.visible_note_relations` action (byte-stable namespace; the
  implementation moved out of `nmp-nip01`, same precedent as §4's
  `nmp.nip65.publish_relay_list`). It depends one-way on `nmp-nip01` plus the
  cross-protocol NIP sources; `nmp-nip01` never depends back.
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

---

## 9. App Composition

`nmp-defaults` is a reusable NMP composition library, **not a leaf
application**. It wires generic NMP mechanisms — the default router, planner,
store, ingest parsers, action modules, coverage hook, raw-event forwarding
policies, default projections, and typed seams. Reducer-owned delivery roots
that cannot implement the full AppHost tier (currently Chirp web) must call
`nmp-substrate-defaults` for the shared router/mailbox/profile/contacts
cache-parser floor; they must not hand-copy that construction.

`nmp-defaults` composes only through `nmp_core::substrate::AppHost` and the
narrow registrar traits below it. It must not depend on `nmp-ffi`, name
`NmpApp`, own a platform runtime handle, or export a native builder. Its target
dependency direction is `platform/app runtime -> nmp-defaults`, never
`nmp-defaults -> platform runtime`.

`nmp-defaults` (like `nmp-core` and every other NMP crate) **must not own
operator policy facts** — relay URLs, nostrconnect bootstrap relay URLs, seed
pubkeys, account auto-follow lists, or signer permission batches. Those facts
belong only in leaf app Rust crates (`apps/<app>/...`, e.g.
`apps/chirp/crates/nmp-chirp-config`) or operator-provided app config (#1493).
The native and browser runtime builders enforce this at compile time: an app
must declare its initial relay set with `.with_relays(...)` or explicitly opt
out with `.without_initial_relays()` before `start()` — there is no framework
relay default to inherit silently. The current `NmpAppBuilder` location in
`nmp-defaults` is migration debt under #2205/#2210/#2212, not the durable
composition boundary.

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

Native shells also never expand ReducedSources or dependent interests. They do
not compute follow/list membership, construct dynamic author filters, run
meta-subscribe fetch cascades, or hydrate profiles/events outside Rust-owned
ref/dependent-interest lifecycles.

---

## 10. Binding Crates

`nmp-native-runtime`, `nmp-browser-runtime`, `nmp-ffi`, `nmp-android-ffi`, and
`nmp-wasm` are delivery surfaces. Runtime adapter crates own platform runtime
lifecycle and typed builders; ABI-glue binding crates own ABI shape, pointer or
byte conversion, panic guards, callbacks, lifecycle handle exposure, and
platform-specific bridge mechanics. They do not own business policy, app
defaults, or example-app namespaces unless they are explicitly app-owned
delivery crates.

Native target split (#2205/#2209):

- `nmp-native-runtime` is the native platform runtime adapter. It owns the
  native `NmpApp`/handle type, actor-thread lifecycle, native runtime slots,
  session registries, native Rust APIs, and the native typestate builder
  (`NmpAppBuilder` / `RunConfig`). It composes `nmp-defaults` like a leaf app
  runtime.
- `nmp-ffi` is a C ABI shell over `nmp-native-runtime`. It owns `extern "C"`
  symbols, opaque pointers, C strings, panic guards, callback registration
  glue, and C-compatible allocation/freeing only.
- `nmp-android-ffi` is JNI/Android delivery glue over the same native runtime
  APIs for lanes not served by UniFFI.

The current code still lets `nmp-ffi` own `NmpApp`, the actor-thread runtime,
native NIP-29/NIP-50/intent session orchestration, and part of the builder path;
`nmp-defaults` also still has native-runtime coupling. That state is migration
debt until #2210-#2214 land. It is not a durable composition-root exception for
`nmp-ffi`, and it is not permission for `nmp-defaults` to depend on `nmp-ffi`.

`nmp-browser-runtime` is the browser composition-root delivery surface
described in §10a. `nmp-wasm` is the wasm ABI shell over that runtime, analogous
to `nmp-ffi` over `nmp-native-runtime`.

The pre-v1 ABI surface is governed, not compatibility-frozen. Net-new
`nmp_app_*` symbols require an ADR or an accepted GitHub issue that explicitly
explains why the generic action, projection, or capability seam is insufficient.
Renames and deletions that collapse legacy wrappers, dead parameters, app-named
generic surfaces, or duplicate paths are preferred over compatibility aliases.
Temporary retention requires a staged GitHub issue with a deletion gate.

---

## 10a. Browser Platform Adapter (nmp-browser-runtime)

`nmp-browser-runtime` is the browser platform adapter per ADR-0067: a Layer-6
runtime adapter, sibling to `nmp-native-runtime`. Unlike pure ABI-glue binding
crates, it is a **composition root**: it composes `nmp-defaults` and protocol
crates into a typed builder (`BrowserAppBuilder`), exactly as a native runtime
does. It thus may depend on Layer-5 composition crates, breaking the usual
binding-crate rule that all siblings avoid each other.

`nmp-browser-runtime` owns:

- The Worker event-loop runtime driving a single `KernelReducer` (D4).
- The browser WebSocket transport adapter (transport bridge only; no policy).
- Browser storage initialization and lifecycle.
- Capability provider registry and browser signer provider mapping.
- Browser timer and clock seams for `nmp-core` injection.
- The typed `BrowserAppBuilder` composition root (browser twin of the native
  runtime's `NmpAppBuilder`).

`nmp-browser-runtime` must not own:

- Routing or outbox policy (that is `nmp-router` / kernel).
- Signing policy or signer-provider semantics (that is `nmp-signers`).
- NIP modules, protocol defaults, app defaults, projection policy, persistence policy.
- The wasm-bindgen ABI surface (that is the sibling `nmp-wasm` ABI shell).

Dependency direction: `nmp-browser-runtime` depends on `nmp-defaults` and protocol
crates in Layers 0–5. `nmp-wasm` and leaf web apps depend on `nmp-browser-runtime`
for the typed builder, not vice versa. No Layer 0-5 crate depends on
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
2. **wasm-safe defaults (#2047 / #2060, reconciled with #2205/#2212).** §10a
   sets the target dependency `nmp-browser-runtime -> nmp-defaults`, but
   `nmp-defaults` currently has native-runtime coupling through `nmp-ffi`. That
   must be removed so neutral `register_defaults` registrations compile for the
   browser and native runtimes alike. Until that lands, the browser builder uses
   the `nmp-substrate-defaults` floor (§9).

---

## 11. Change Policy

When a crate-boundary rule changes:

1. Update the durable owner document: this file, a product spec, or an ADR.
2. Update or create GitHub Issues only for live work still required.
3. Do not add a parallel plan, review dump, or architecture ladder.
4. Do not preserve completed migration history here. Git history already does
   that.
