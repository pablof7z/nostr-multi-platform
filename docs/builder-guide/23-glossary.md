# 23 — Glossary

One definition per term, used identically across every section. Each entry
links to the file that *defines* it on master. Cited line numbers verified at
master tip. Internal (`pub(crate)`) types are tagged **[internal]**. Types
that existed in a removed v2 design are tagged **[removed]** — do not use them.

- **actor** — the single OS thread that owns all mutable state and runs the
  TEA loop (`AppState` + `KernelAction` → `handle_message`). `dispatch()` is
  fire-and-forget; nothing else mutates state. *defined in:* `crates/nmp-core/src/app.rs`.
- **AccountManager** — owns the set of accounts and the synchronous
  active-account switch. *defined in:*
  `crates/nmp-signers/src/identity/manager.rs:68`.
- **ActionModule** — the write-seam trait: `NAMESPACE`, `type Action`,
  `start()` (validate), `execute()` (enqueue `ActorCommand`). Registered via
  `NmpApp::register_action(module)`. *defined in:*
  `crates/nmp-core/src/substrate/action.rs:56`.
- **AppState** — the kernel projection that crosses FFI: a monotonic `rev`
  plus only the data behind currently-open views (D5). Generic kernel form is
  `{ rev, open_view_count }`. *defined in:* `crates/nmp-core/src/app.rs:26`.
- **AppAction** — **[removed]** the old per-app, codegen-produced action enum
  from the deleted `nmp gen modules` generator. Dispatch is now a string-keyed
  runtime lookup: `nmp_app_dispatch_action(app, namespace, action_json)`. Action
  input types live in their module crates; the host binds them to the namespace
  string. See [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md).
- **AppUpdate** — **[removed]** the old per-app, codegen-produced update enum
  from the deleted `nmp gen modules` generator. The kernel pushes one binary
  `UpdateFrame` (FlatBuffers) with typed projection sidecars; the host decodes
  by key, not by a generated enum. See [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md).
- **capability** — a native bridge that *reports* raw platform events; it
  never decides policy (D7). Request → native execution → result envelope.
  *defined in:* `crates/nmp-core/src/substrate/capability.rs:11`
  (`CapabilityModule` trait).
- **CapabilityModule** — the native-bridge-shape trait: `NAMESPACE`,
  `type Request`, `type Result`, `callback_interface_name()`. Wired via
  C-ABI callbacks on the native side. *defined in:*
  `crates/nmp-core/src/substrate/capability.rs:11`.
- **claim** — a refcounted pin held by a consumer (view/sync job) that keeps
  an event or interest alive; GC reclaims only unclaimed state. *defined in:*
  `crates/nmp-core/src/store/types/gc.rs:10` (`ClaimerId`).
- **CompiledPlan** — the planner output: a per-relay map of exactly which REQ
  frames to emit, plus a content-addressed `plan_id` for diagnostic
  continuity. *defined in:* `crates/nmp-planner/src/plan.rs`.
- **dependent interest** — a `LogicalInterest` or ref claim whose desired shape
  is derived from another source result rather than declared directly by native
  UI code. It is still a normal registry/planner input: refcounted, cache-first,
  deduped, and closed when its owner/source withdraws. *defined by:* #2092 and
  `docs/design/subscription-compilation/intro.md` §2.2.
- **DomainModule** — **[removed]** proposed v2 trait for kernel-owned durable
  records. Never shipped. Use an app-owned `Arc<Mutex<T>>` store +
  `register_snapshot_projection` instead. See [05a](05a-substrate-traits.md)
  §Removed v2 traits.
- **EventStore** — the single-writer (D4) store of verified events; insert
  applies replaceable/delete/expiry invariants and returns an
  `InsertOutcome`. *defined in:* `crates/nmp-core/src/store/mod.rs`.
- **FlatBuffers update transport** — the canonical runtime payload format for
  Rust-to-frontend `FullState`, `ViewBatch`, and side-effect frames. UniFFI
  owns lifecycle/bindings; JSON is not a production update fallback.
  *defined in:* GitHub issue #991 (F-10).
- **FfiApp** — **[removed]** the per-app FFI entry struct from the deleted
  `nmp gen modules` generator. Composition is now a library call
  (`nmp_defaults::register_defaults`) plus a thin staticlib shell that calls
  app-core `register()`. See [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md).
- **IdentityModule** — **[removed]** proposed v2 trait for signer scopes.
  Never shipped. Identity lives in `nmp-signers`; see
  [11 — Sessions + signers](11-sessions-signers.md).
- **IdentityScopeKind** — the four identity scopes: `HumanAccount`,
  `AppLocal`, `ExternalSigner`, `Ephemeral`. *defined in:*
  `crates/nmp-core/src/substrate/identity.rs`.
- **InsertOutcome** — the typed result of `EventStore::insert`
  (`Inserted`/`Duplicate`/`Replaced`/`Superseded`/`Tombstoned`/`Rejected`/`Ephemeral`).
  *defined in:* `crates/nmp-core/src/store/types/mod.rs:17` (re-export;
  source `store/types/outcomes.rs:11`).
- **InterestRegistry** — the D4 single-writer registry that assigns stable
  `InterestId`s and is the authority over live logical interests. *defined
  in:* `crates/nmp-core/src/subs/registry.rs:23`.
- **kernel** — `nmp-core`: substrate + planner + store + subs + publish. Holds
  no app nouns (D0). Apps assemble from kernel + protocol modules + an
  app-core crate. *defined in:* `crates/nmp-core/src/lib.rs`.
- **KernelEventObserver** — the in-process event fan-out trait: a single
  `on_kernel_event(&self, event: &KernelEvent)` method, fired on the actor
  thread for every `Inserted | Replaced` ingest. The v1 mechanism for
  event-driven views. *defined in:*
  `crates/nmp-core/src/actor/commands/event_observer.rs:189`.
- **KernelEvent** — the substrate-level event passed to `KernelEventObserver`
  and `register_snapshot_projection` closures; carries `id`, `author`, `kind`,
  `created_at`, `tags`, `content`. *defined in:*
  `crates/nmp-core/src/substrate/view.rs`.
- **LogicalInterest** — what a consumer wants alive on the wire (id, scope,
  shape, hints, lifecycle). Compiler input — *not* a Nostr filter. *defined
  in:* `crates/nmp-planner/src/interest.rs`.
- **MailboxCache** — the trait the compiler consults for a pubkey's NIP-65
  relay list. *defined in:* `crates/nmp-core/src/substrate/routing.rs`
  (`MailboxCache` trait).
- **ModuleRegistry** — **[removed]** proposed v2 composition root that
  collected `(namespace, family, type_name)` strings. Never read back by
  any kernel runtime. Removed. The replacement is the `register()` convention
  plus the `nmp-defaults` composition root: an app-core crate exports a
  `pub fn register(app: &mut impl AppHost)` fn that calls
  `nmp_defaults::register_defaults(app)` once, then app-specific registrations.
- **plan-id (`plan_id`)** — content-addressed hash over
  `(sorted_interests, mailbox_snapshot, lattice_version)`; stable across
  no-op recompiles. *defined in:* `crates/nmp-planner/src/plan.rs`.
- **PublishEngine** — the per-(event, relay) state machine driving the
  durable retry queue; native never decides retry policy (D7). *defined in:*
  `crates/nmp-core/src/publish/engine.rs:62`.
- **provenance** — per-event record of which relays delivered it and when;
  the first (post-sort) relay is `primary`. *defined in:*
  `crates/nmp-core/src/store/types/outcomes.rs:62` (`ProvenanceEntry`).
- **`relay_pin`** — hard routing override on `InterestShape`: when `Some`, the
  four-lane outbox dispatch is suppressed and the interest goes to exactly
  that host (third routing lane; NIP-29 groups). Never serialized onto the
  wire. *defined in:* `crates/nmp-planner/src/interest.rs`.
- **RelayAck** — the D7 publish envelope (`ok`, `code`, `message`, `details`)
  describing one relay's response to one event. *defined in:*
  `crates/nmp-core/src/publish/state.rs:48`.
- **RelayRole** — **[internal]** crate-private classification of why the
  kernel is connected to a relay. The diagnostic-facing public concept is
  `RoutingSource`. *defined in:* `crates/nmp-core/src/relay.rs:16`.
- **rev** — the monotonic `u64` carried on every `AppState`/update; platforms
  enforce a stale-guard (drop updates with `rev` ≤ last seen). *defined in:*
  `crates/nmp-core/src/app.rs:28`.
- **ReducedSource** — an app/protocol-owned source expression plus deterministic
  reducer that turns source events/state into materialized interest shapes
  (authors, tags, ids, or addresses). Core/planner see only the resulting
  `LogicalInterest`s; NIP nouns such as contact list or mute list stay in the
  protocol/defaults crate that owns the reducer. *defined by:* #2092 and
  ADR-0036/ADR-0042 amendments.
- **scope** — `InterestScope`: the account context for mailbox resolution
  (`ActiveAccount` / `Account(id)` / `Global`). Distinct from *session* and
  *account*. *defined in:* `crates/nmp-planner/src/interest.rs`.
- **snapshot** — the default emit unit: a full view payload recomputed in the
  actor and pushed on change; granular deltas are an optimization. *defined
  in:* `crates/nmp-core/src/substrate/view.rs` (`ViewDependencies` +
  snapshot projection system).
- **snapshot projection** — a named JSON slice pushed under
  `KernelSnapshot.projections[key]` on every emit tick, registered via
  `NmpApp::register_snapshot_projection(key, closure)`. Distinct from view
  deltas. *defined in:* `crates/nmp-ffi/src/lib.rs:1109`.
- **substrate** — the two extension traits (`ActionModule`, `CapabilityModule`)
  plus `ViewDependencies`, routing types, and the reactive machinery the kernel
  provides; everything app-specific is a module on top. *defined in:*
  `crates/nmp-core/src/substrate/mod.rs`.
- **SyncStrategy** — the coverage gate's verdict for a `(filter, relay)` pair:
  `SkipReq` / `NegThenReq` / `ReqSince(u64)` / `Resume`. *defined in:*
  `crates/nmp-nip77/src/coverage_gate.rs:49`.
- **RoutingSource** — diagnostic record of which lane put a relay in the plan
  (`Nip65` / `Hint` / `Provenance` / `UserConfigured`). *defined in:*
  `crates/nmp-planner/src/plan.rs`.
- **TombstoneRow** — the suppression record for a deleted/expired event
  (`Kind5` / `NIP40Expiry` / `AdminPurge`). *defined in:*
  `crates/nmp-core/src/store/types/mod.rs:17` (re-export; source
  `store/types/outcomes.rs:40`).
- **VerifiedEvent** — a `RawEvent` past id-hash + Schnorr verification; the
  only type `EventStore::insert` accepts. *defined in:*
  `crates/nmp-core/src/store/types/events.rs:133`.
- **ViewDependencies** — the planner bridge struct a module populates to
  declare its event needs (`kinds`, `authors`, `ids`, `tag_refs`,
  `projection_keys`, `relay_pin`, `limit`); converted to a `LogicalInterest`
  via `into_logical_interest`. *defined in:*
  `crates/nmp-core/src/substrate/view.rs`.
- **ViewModule** — **[removed]** proposed v2 typed reactive projection trait
  (`Spec`/`Payload`/`Delta`/`Key`/`State`). Never shipped. Use
  `register_event_observer` + `register_snapshot_projection` instead. See
  [05a](05a-substrate-traits.md) §Removed v2 traits.
- **ViewPayload** — **[removed]** associated type on the removed `ViewModule`
  trait. See **ViewModule** entry.
- **ViewSpec** — in the codegen convention, `pub enum ViewSpec {}` is the
  per-module view-spec enum exported by every app module crate (may be empty
  if the module has no host-driven view specs). Distinct from the removed
  `ViewModule::type Spec`. *defined by codegen convention in each app module.*
- **watermark** — per-`(filter, relay)` sync bookmark (`synced_up_to`,
  method, resume blob) classified by `Coverage`. *defined in:*
  `crates/nmp-core/src/store/types/mod.rs:18` (re-export; source
  `store/types/watermark.rs:17` — `WatermarkRow`).
- **WireFrame** — a frame to push onto the wire: `Req{…}` or `Close{…}`,
  produced by the plan-diff. *defined in:*
  `crates/nmp-core/src/subs/wire.rs:29`.

Pairs that are *not* synonyms:
- **ViewSpec** (codegen export enum) ≠ the removed `ViewModule::type Spec`.
- **session/scope** ≠ **account** (an account is a key identity, a scope is
  an interest's routing context).
- **snapshot projection** (named JSON slice via `register_snapshot_projection`)
  ≠ **view delta** (the `ViewBatch` update pushed by the reactive engine).
- **M8-subs** (subscription lifecycle, §14) ≠ **M8-multi-account**
  (`AccountManager`, §11) — same milestone number, different subsystems.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md),
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md),
[07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md),
[22 — Doctrine compliance checklist](22-doctrine-checklist.md).
This glossary is the reverse-link target for every section's first use of a term.
