# High-Level Architecture Overview

> **Status:** Candidate architecture for issues #2313 and #2316, written for ADR
> review. This is not a shipped API contract and not a settled solution. It
> records the desired shape, rejection tests, and migration questions so an ADR
> can decide final naming, migration order, compatibility scope, and
> implementation details before code changes.

## Authority And Retirement

This packet is a local design workspace for the current architecture iteration.
It is not the canonical tactical queue, not a durable replacement for existing
architecture docs, and not mergeable as a parallel authority in this form.

Before this work becomes a PR, the surviving decisions must move into the
appropriate durable homes: ADRs, existing architecture/design docs, builder-guide
pages, product specs, and GitHub issues for migration work. Anything that remains
only as an exploration artifact should be deleted or explicitly retired. The
point of this directory is to converge on the right shape before editing the
canonical docs, not to add another source of truth.

This directory should not survive as a parallel plan. The final migration must
turn accepted facts into durable docs/ADRs, turn tactical work into GitHub
issues, and then delete or retire `docs/new-arch`. If this packet still needs to
be read to understand current architecture after signoff, P8 failed.

The fitness matrix in this packet is transitional: it becomes real only when it
is executable work. Each accepted ratchet must graduate into a doctrine lint,
`nmp-testing` gate, CI check, GitHub issue acceptance criterion, or ADR
enforcement section before this directory is retired.

This is the high-level entry point for the proposed architecture. It explains how
an NMP app feels from the app developer's perspective, then shows how data flows
through NMP's crates. The other docs go deeper into app assembly, live reads,
writes, and internal migration.

It treats #2316 as the problem statement, not as a settled solution.

The core idea is:

```text
install features
  -> open typed feature/ref sessions
  -> render typed Rust-owned outputs
  -> dispatch typed intents
  -> construct/finalize event drafts
  -> sign through a selected signer
  -> publish through Rust-owned routing and status
```

The framework may stay internally complex where Nostr requires it. The app API
must not require developers to manually compose raw interests, observers, cache
replay, dynamic dependency sources, projection sidecars, snapshot ticks, and
teardown recipes.

The destination is simpler because the public unit becomes a whole feature
session lifecycle. It is not simpler because Nostr routing, replay ordering,
projection delivery, signing, and publish policy disappear.

## Evidence Map

This packet is grounded in current repo/issues rather than the authority of the
existing architecture text:

- **#2313:** app-developer API pain. The missing door is not a shell-owned raw
  event stream; home feed is not special.
- **#2316:** root cause. One feature's state is split across acquisition,
  replay, sink, admission, sidecar, projection emission, ticks, dependencies,
  and teardown.
- **#2316 rescope:** the fix is not a convenience `open_feature()` wrapper. It is
  a foundational decomposition problem: one concept must own the whole lifecycle,
  or the wrapper only hides fragmentation for one more layer.
- **#2307:** concrete deletion proof for the same root. Tick-polling
  `ActiveObservedProjection`/`DynamicObservedProjection` copies should collapse
  into one event-driven reconciler, then `register_snapshot_tick_observer` should
  be deleted once sibling consumers migrate.
- **#2088/#2089/#2090/#2091/#2092/#2113:** replay-before-live, no public
  filterless observers, dynamic sources, pointer/ref demand, and related
  lifecycle fragments must become one owned session contract.
- **Projection and pull-cursor ADRs:** pushed typed outputs remain the app UI
  state path; raw event-log/pull surfaces are not a replacement for screen state.
- **Downstream audits:** Highlighter, Podcast Player, and `nmp-gallery` are
  acceptance tests for whether the public model hides NMP internals without
  moving policy into native shells.
- **Wiki/episode evidence:** prior generated wiki pages are useful evidence, but
  not authority. Some pages already encode later corrections; others still teach
  old surfaces such as app-facing `ReducedSource`/`open_feed`,
  `register_defaults()` gallery registration, or `RoutingContext::explicit_targets`
  as if those were settled destinations.

Current durable docs still teach some of the old architecture. That is evidence
of migration debt, not a contradiction this packet should absorb as truth. The
P8 retirement phase must correct those owners in place before this packet becomes
durable documentation.

Known contradiction ledger for P8:

| Surface | Current wiki/doc signal | Required resolution before signoff |
|---|---|---|
| `docs/wiki/guides/reduced-source.md` | treats `ReducedSource`, `open_feed(FeedParams)`, and `FeedParams` as typed app-facing dynamic-feed architecture | either the ADR keeps that public surface, or the page is rewritten around typed sessions with private source reconciliation |
| `docs/wiki/guides/publish-outbox-pipeline.md` and `docs/wiki/guides/nip29-wiring.md` | record the old `RoutingContext::explicit_targets` versus live `PublishTarget::Explicit` split | implementation must delete or migrate the dead seam; no work may route NIP-29/NIP-17 through dead plumbing for cosmetic consistency |
| `docs/wiki/guides/nmp-gallery-app.md` | still records gallery registration through `register_defaults()` and snapshot/readiness probes | gallery must move to explicit composition or be labeled tutorial/showcase compatibility with owner and removal gate |
| `docs/wiki/guides/operator-data-leaf-apps-only.md` | reinforces leaf-app ownership for relays, seed follows, NIP-46 permissions, and signer labels | this packet must preserve that boundary; no defaults rewrite may reintroduce operator policy into NMP crates |
| `docs/wiki/guides/signer-broker-handshake-loop.md` and NIP-46 research pages | protocol state must not own transport/process loops; reconnect/cancel must be event-driven | signer/session phases must prove transport-agnostic protocol core plus runtime-owned execution, not a second signer runtime framework |
| `docs/wiki/guides/action-module-adr.md` | dual action seams and typed external-effect rules remain part of the evidence base | write-flow migration must converge on typed actions/builders and explicit capability results, not create another dispatch door |
| ADR-0009, ADR-0046, ADR-0053, ADR-0062 and builder-guide 02/15/19/28 | teach app assembly through defaults, observed projections, projection tiers, or action-triggered subscription recipes | amend or retire once typed sessions, explicit composition, and session-scoped output demand are accepted |
| ADR-0049 composition ledger/yield behavior | records useful observability for what composition installed and yielded | preserve observable composition ledgers when deleting hidden defaults; do not lose auditability while simplifying |
| `docs/product-spec/api-surface.md`, `docs/product-spec/cli-toolchain-phasing.md`, `docs/ffi-surface.md`, `docs/wasm-surface.md`, and `docs/recipes/app-shapes.md` | still expose old public read/write/init surfaces as normal product API | rewrite around typed sessions/actions, explicit feature composition, and scoped compatibility doors |
| wiki noun/topic pages for `nmp-defaults`, `ObservedProjection`, `read-surface`, `write-register-surface`, `nmp-wasm`, and `nmp-browser-runtime` | generated pages may preserve stale surface names as facts | regenerate, correct in place, or retire after durable owners are updated |

## Concept Status

The design must not turn every existing internal mechanism into a new permanent
API. The intended public vocabulary is deliberately small and the rest is
classified by target disposition:

Survivor vocabulary budget:

```text
public: feature composition, typed sessions, typed actions/builders,
        capability results, typed outputs/status
private or deleted: everything else unless a named invariant, live owner,
        and kill criterion prove it must survive
```

| Concept | Status | Rule |
|---|---|---|
| Feature bundle | Candidate public concept pending ADR | Installs typed sessions, actions, outputs, builders, and capability needs through narrow registrars or builder methods. Avoid a broad `dyn AppFeature` object unless it deletes existing complexity. |
| Typed session / session descriptor | Candidate public concept pending ADR | The app opens typed demand and receives a handle plus typed output. This is the public replacement for hand-wiring interest, replay, observer, projection, and teardown. |
| Typed action / generated builder | Candidate public concept pending ADR | User intent enters Rust through typed action data with correlation ids, validation, signer route, and status. |
| Capability result | Candidate boundary concept pending ADR | Native/web executes OS or platform capabilities and reports raw results back into Rust. |
| Typed projection/status output | Candidate render contract pending ADR | Rust emits semantic state, publish status, signer status, and app output; shells render it. |
| Route provenance | Candidate internal contract surfaced in status | Exact relays are insufficient. Publish routing must preserve why a route is valid: automatic, host-pinned, verified private inbox, manual override, or imported/verbatim. |
| `ObservedProjection` | Private machinery | Keep if it protects replay-before-live, scoped delivery, relay provenance, and close semantics. App developers should not assemble it. |
| `ReducedSource` / shape reconciler | Private/provisional machinery | Treat as the dynamic-source invariant, not a public noun. The current private feed machinery and adjacent pointer/browser/defaults reconcilers generalize only after semantic proof across source families. |
| Reverse wake/admission indexes | Private machinery when proven | Wake sources and bounded admission are mandatory; specific indexes are added only when scoped fanout cannot prove the invariant. |
| Projection tiers / `SnapshotRegistry` / `DeclaredProjections` | Private executor machinery | They may remain internally only where they preserve bounded output and merge correctness. They are not app-facing composition language. |
| Generated adapters | Contract machinery | Use for schema, merge/cache, and action-builder drift prevention. Generation is mandatory where hand-authored adapters cannot prove the same contract. |
| Public `LiveQuery` engine | Rejected public concept | A typed session descriptor may be named `LiveQuery`, but it must not become a second lifecycle engine or protocol owner. |
| Public `ReducedSource` | Rejected public concept | Dynamic source reconciliation stays behind typed sessions unless a later ADR proves a real app-facing need. |
| Raw `open_interest` app reads | Rejected public product model | Keep only substrate, protocol-internal, diagnostic/test/export, or migration scopes with deletion/formalization criteria. |
| `register_defaults()` production composition | Rejected public product model | Production composition is explicit feature opt-in. A preset may exist only as tutorial or migration compatibility with live consumers, support window, owner, and deletion/formalization gate. |
| Compatibility doors | Migration-scoped | Raw `open_interest`, defaults presets, JSON dispatch, and explicit relay escape paths need scope labels, live consumers, and deletion/formalization criteria. Zero live consumers means delete. |

Compatibility clarification: `nmp-defaults` can survive as a reusable
composition crate if it contains explicit installers and shared substrate
building blocks. What is rejected is a hidden production `register_defaults()`
mental model where the app root cannot tell which protocol features, runtimes,
policy, projections, and route machinery are installed. A preset can remain only
as tutorial/migration/test compatibility with named live consumers, owner,
support window, and deletion/formalization gate.

Foundational-decomposition clarification: a typed session is acceptable only if
it becomes the owner of the full read lifecycle. It is not acceptable to add an
`open_feature()` facade that still requires separate interest registration,
projection declaration, replay wiring, observed sink activation, sidecar/cache
registration, tick emission, dynamic source reconciliation, and close-token
bookkeeping underneath. That would answer the DX complaint while preserving the
architecture defect #2316 names.

## Deletion-First Rule

The architecture should remove more concepts than it adds. The preferred outcome
for a disputed module, crate, public method, or executor mechanism is:

```text
delete it
  -> or collapse it into an existing owner
  -> or make it private executor machinery
  -> or keep it temporarily with live consumers and a removal gate
  -> only then add a new public concept
```

Fewer modules is usually better, but module count is not the primary metric.
Combining unrelated owners into one large crate would hide complexity rather
than remove it. The real metric is fewer permanent public nouns, fewer lifecycle
recipes, fewer sources of truth, fewer publish/read doorways, fewer compatibility
shims, and fewer places a feature author must edit to make one behavior work.

A new module or crate is justified only when it makes one of those counts go
down or protects an invariant that cannot be protected by narrowing existing
code. If the first implementation of `FeatureSession`, route provenance,
generated adapters, or source reconciliation adds a layer while the old layer
remains an equal production path, the design has failed its simplification
claim.

## Design Hypothesis

This is the right destination only if it reduces the public model to a few
concepts while making the existing correctness invariants harder to violate:

```text
feature composition
session lifecycle
typed output
typed action / generated builder
capability result
publish status
```

It is the wrong destination if implementation adds a new `LiveQuery` layer while
leaving product apps to keep using raw `open_interest`, manual projection
declarations, tick observers, native relay selection, or native publish JSON. The
test is not whether the names are nicer; the test is whether one feature's live
state has one owner, one handle, one teardown path, one output contract, and one
route policy.

This hypothesis must be enforced by the ratchets in [Internal
Machinery](04-internal-machinery.md), especially FF-001 through FF-026. A prose
claim that the new model is simpler is insufficient without those checks moving
old-pattern counts down or keeping them from growing.

## Current Forces

The architecture is hard to change today because one product behavior is split
across too many independently wired mechanisms:

- `register_defaults()` hides which protocol features, runtimes, policies, and
  projections are active.
- Interest acquisition and projection output can be registered separately, so
  invalid states are representable: data fetched but invisible, or output
  declared without demand.
- Projection tiers leak an executor detail into app composition. The app thinks
  it declared what it consumes, but host-registered outputs bypass that manifest.
- Dynamic source sets are re-derived by feature recipes instead of being a
  reusable Rust-owned session capability.
- Publish variants can all say "explicit relay" while losing why the route is
  valid.
- Downstream apps still have direct web NDK paths, Swift protocol parsing,
  app-side signer inference, and native policy/state that should be Rust-owned.

The target is worthwhile only if it reduces those forces. If it leaves the same
number of lifecycle recipes, manifests, route paths, and shell policy sites, it
is not simplification.

## What Becomes Easier

The design should make these changes materially cheaper:

- add a new Nostr read feature without inventing a new open/replay/projection
  recipe;
- add a custom app read model without modifying NMP crates or moving relay logic
  to native;
- migrate one feature across Swift, Kotlin, TypeScript, TUI, and browser without
  reimplementing protocol parsing in every shell;
- reason about publish status, retries, signer continuations, and route proof
  from one Rust-owned status stream;
- audit app composition by reading the app root instead of spelunking defaults.

## What Becomes Impossible

New code must make these states hard or impossible:

- a product screen opens raw relay interest and then forgets to declare output;
- a projection emits app-visible rows without an owning feature/session demand;
- native code chooses Nostr relays, mutates protocol tags, or infers publish
  success;
- an empty dynamic author/source set becomes a wildcard subscription;
- NIP-29 and NIP-17 routes degrade to a generic explicit-relay bucket with no
  route provenance;
- a starter template teaches projection tiers, `register_defaults()`, or
  `open_interest` as normal product architecture.

## Downstream Acceptance Checklists

These are not migration chores to defer after the ADR. They are kill-tests for
whether the destination architecture is real.

**Highlighter**

- Decide whether Highlighter web is an NMP target runtime, an SSR/migration
  exception, or out of scope. There is no mixed answer: if web is a product
  runtime, direct NDK reads/writes/signing must migrate behind NMP typed
  sessions/actions or be deleted; if it is SSR/migration/out-of-scope, that
  boundary needs an owner and formal criteria. Direct NDK cannot remain both a
  violation and a normal shipping path.
- Treat this as a hard signoff blocker, not an implementation detail. If
  Highlighter web remains a shipping runtime, every direct NDK fetch, subscribe,
  sign, publish, cache, relay-set, and tag-parser product path must be migrated
  behind NMP or explicitly classified with owner and deletion/formalization gate.
- Replace or classify web NDK product runtime paths for onboarding/profile,
  rooms/invites/membership, highlights, comments, capture, Blossom, NIP-05,
  search/SSR, and signer sessions.
- Replace Swift/TypeScript protocol parsing for `tagsJson`, NIP-10/NIP-22 refs,
  article cards, embeds, relay hints, and discussion roots with Rust descriptors
  or generated adapters.
- Move Wi-Fi-only, offline/cache, signer/session persistence, and publish status
  policy behind Rust-owned state with explicit native capability mirrors.
- Product event writes must be correlated typed actions with publish/status
  outputs; fire-and-forget raw writes fail the proof.

**Podcast Player**

- Real NIP-F4 publish must leave `relay_pending` diagnostics behind: show, episode,
  and feed/list events must build, sign, route, store, publish, and report relay
  ack/error/retry status through Rust.
- Widgets, AppIntents/Siri, CarPlay, remote commands, Live Activities, Handoff,
  and suspended/cold starts must use app-lifetime/service sessions or typed
  capability results. They must not own playback queue, signer state, relay
  policy, or publish status.
- "Dispatch accepted" is not completion. OS-owned surfaces must get Rust-owned
  operation result, pending, error, or completion state rather than treating a
  foreground singleton enqueue as user-visible success.
- Native mirrors need an explicit owner: App Group widget snapshots,
  `MPNowPlayingInfo`, ActivityKit state, `NSUserActivity`, Keychain, media caches,
  and Swift/SQLite stores are allowed only as capability/rendering mechanics when
  Rust remains durable truth.
- Configured relays, legacy single relay settings, agent relays, Blossom server
  lists, NIP-46 relay settings, and manual publish relays must converge on typed
  route provenance.
- App-specific Rust is correct; hand-authored app FFI/action glue is not a final
  framework proof unless it is generated, typed, or explicitly app-local with no
  protocol-policy leakage.

**nmp-gallery**

- The proof includes iOS, Android, TUI, desktop, and web. `web/nmp-gallery`
  exists; its wasm build deferral and raw worker ref API are migration evidence,
  not proof of completion.
- Gallery proof is split into separate gates: ref lifecycle, web wasm/worker,
  generated ref API, and per-shell auth/signing matrix. Android NIP-55 progress
  or a native demo does not imply web, iOS, TUI, or desktop correctness.
- Component refs and embeds must use deterministic open/close ownership. Web
  release/reclaim loops and desktop/TUI claim-on-render/tick patterns are the
  lifecycle smell to remove; copied-label timers are only presentation.
- Gallery signing coverage is a matrix, not a blanket claim. Android NIP-55,
  web NIP-07, iOS, TUI, and desktop each need a decision or proof.
- The gallery composition root must move off hidden `register_defaults()` /
  `consume_all_builtin_projections()` teaching, or label that path as
  tutorial/showcase compatibility.

## Signoff Gates

The architecture is not signoff-ready until these downstream gates either pass
or trigger a named kill criterion. They are not follow-up polish.

| Gate | Required proof |
|---|---|
| Highlighter web runtime inventory | Every `@nostr-dev-kit`, `$subscribe`, `fetchEvents`, direct sign/publish, relay-set, cache, and tag-parser product path is classified as NMP target-runtime migration, SSR-only, diagnostic, deleted, or explicitly out of scope, with owner and deletion/formalization criterion. Direct NDK cannot remain both violation and normal runtime. |
| Highlighter iOS/session policy | Wi-Fi/offline/cache policy is Rust-owned from raw platform capability facts; profile/event/embed refs use typed owner handles with close/clear tests; production app roots do not rely on hidden `register_defaults()` or `consume_all_builtin_projections()` except labeled migration/tutorial paths. |
| Highlighter semantic parsing | `tagsJson`, manual NIP-10/NIP-21/NIP-22/NIP-29 parsing, article/highlight card derivation, and discussion-root inference either move behind Rust descriptors/generated adapters or are proven presentation-only. |
| Podcast service sessions | Widget, AppIntent/Siri, CarPlay, remote command, Live Activity, Handoff, and suspended/cold start flows work through app/service sessions or typed capability results, not UI-process singletons or shell-local durable state. Results report Rust-owned pending/error/completion, not only dispatch acceptance. |
| Podcast NIP-F4 publish | Show, episode, feed/list, Blossom references/server provenance, named/per-podcast signer, key-storage capability, route provenance, local ingest, relay ack/error/retry/exhausted status, and user-visible completion are proven end to end. Constructed JSON, queued-only status, `publish_dispatched`, or `relay_pending` is not enough. |
| `nmp-gallery` web/ref lifecycle | wasm/worker runtime builds, missing wasm/worker fails closed, ref APIs are generated/typed, no correctness `setInterval` release/reclaim loop is needed, and component refs clear by owner lifecycle across web/iOS/Android/TUI/desktop. |
| `nmp-gallery` auth/signing matrix | Android NIP-55, web NIP-07, iOS, TUI, desktop, local/no-auth, remote signer, and unauthenticated modes are classified independently; one shell's proof cannot stand in for another. |
| Generated merge/cache parity | Full, delta, clear/tombstone, stale-frame, decode-poison, and baseline recovery behavior match across every shell used by a migrated feature. |
| Publish route provenance | Status payloads expose provenance class and reason, not just relay URLs or queued/signed. Manual, host-pinned, verified inbox, imported/verbatim, and diagnostic routes remain distinguishable through dispatch, signing, retry/resume, local ingest, and status. |
| App-feature API classification | Every cross-boundary app API is generated typed, capability/result, diagnostic/test, or migration-with-deletion. Event-producing APIs always route through typed action/publish status, never hand-authored JSON/event doors. |
| Downstream no-polling | Downstream timers are classified as presentation/capability sampling or deleted. Service/session/signer/product-state retry, refresh, and reconciliation timers fail the gate. |
| Browser runtime/storage lifecycle | Browser storage initializes through runtime-owned async-before-start, OPFS/SQLite runs in a dedicated Worker with real-browser conformance, and missing wasm/worker paths fail instead of silently degrading. |
| Generated catalogs/manifests | Signer catalogs, platform manifests/plists, relay config, release manifests, and client identity derive from one Rust or manifest source of truth with drift gates. |
| Protocol taxonomy ownership | Kind predicates and protocol taxonomy are single-sourced; generic layers do not add per-NIP/per-kind branch tables. |
| Metadata privacy gate | Client/NIP-89 metadata is appended only at one outbound-finalization site, only for public-routable unsigned events, and never for private/imported/pre-signed/reserved surfaces. |
| Binding strategy | Generated bindings or UniFFI work is accepted only when it deletes drift or narrows old public doors; binding churn alone is not an architecture proof. |

## Signoff Dossier

Before the ADR can say "this is the right architecture," there must be a concrete
dossier that proves the claim. A clean narrative is not enough.

Required dossier sections:

- current baseline counts for every old-pattern family in FF-001 through FF-026;
- disposition of every public door in P-1: delete, privatize, formalize, or
  migration-scope with owner/support window/removal gate;
- proof that the first descriptor slice reduces lifecycle recipe count rather
  than adding a parallel read engine;
- proof that route provenance uses the smallest carrier that preserves the
  invariant, with the dead explicit-route seam deleted or given a real owner;
- downstream matrices for Highlighter, Podcast Player, and `nmp-gallery`, with
  each row marked migrated, deleted, diagnostic/test, SSR-only, out-of-scope, or
  kill-criterion-triggered;
- browser runtime/storage proof, including OPFS worker conformance and
  multi-tab/ephemeral-tab policy;
- generated catalog/manifest proof that native/web tables derive from Rust or a
  manifest source of truth;
- durable-doc retirement list with each stale doc corrected in place or retired;
- subjective product calls explicitly resolved, including Highlighter web,
  tutorial preset, downstream release gates, and manual explicit relay UX.

The dossier should make a negative answer possible. If the first descriptor
proof, publish-provenance proof, or downstream matrix shows the model adds more
permanent concepts than it deletes, the correct result is to reject or narrow
the architecture rather than carry the new names forward.

## Deletion-First Bias

The preferred outcome is not "a new architecture layer." The preferred outcome
is a smaller NMP: fewer permanent public doors, fewer kernel concepts, fewer
crate-level extension seams, fewer binding-specific tables, and fewer recipes an
app developer has to memorize.

For every proposed module, crate, or kernel responsibility, the signoff question
is subtraction-first:

```text
can this concept be deleted entirely?
can it be private implementation detail instead of public API?
can two mechanisms collapse into one existing owner?
does this protect an invariant that cannot live in an app or protocol crate?
does it reduce the number of app-visible recipes?
does it delete more old surface than it adds?
```

The kernel should be boring infrastructure: lifecycle admission, session/output
ownership, event ingestion, routing handoff, publish orchestration, replay-safe
state transitions, and capability/effect correlation. Protocol behavior belongs
in protocol crates. App-specific behavior belongs in app Rust. If a kernel
module cannot name a cross-app invariant it alone protects, it should move down,
collapse into an existing mechanism, or disappear.

This is especially important for names used in this packet. `ObservedProjection`,
`ReducedSource`, projection manifests, route provenance carriers, generated
adapters, and publish contexts are candidate ways to preserve invariants. They
are not automatically permanent architecture. If typed sessions can absorb
observed projection lifecycle, delete the public registrar. If source planning
can be local to session descriptors, do not invent a reusable source framework.
If explicit publish routing can be one private route-provenance value, delete
the dead second seam.

The only kind of simplification that counts is net simplification. It is not
simpler to remove a kernel module by making Highlighter, Podcast Player, the
gallery, Swift, TypeScript, or app code reimplement relay routing, signing
policy, privacy gates, cache invalidation, route provenance, or protocol parsing.
That would shrink NMP by exporting its complexity. The target is to delete
concepts while keeping single ownership of the real invariants.

## Complexity Budget

This proposal does not assume today's internal machinery is automatically right.
Each retained concept has to survive a YAGNI review:

```text
what invariant does it protect?
what simpler design was considered?
what breaks if the simpler design is used?
can two existing mechanisms collapse into one?
can this stay private instead of becoming API?
what test or downstream app proves it is needed?
```

Current infrastructure is evidence, not authority. Names in this sketch are
invariants to prove, not commitments to add new Rust types. If
`ObservedProjection`, `ReducedSource`, projection manifests, generated adapters,
publish context, live counts, or any other mechanism cannot defend its cost
against a simpler design, it should be deleted, collapsed, or kept as
migration-scoped compatibility with an owner and deletion/formalization criteria.

## Prior Concern Coverage

This packet is meant to capture the essence of the prior long-form questions and
episode/wiki decisions:

- **#2313:** Home feed is not special. Default subscriptions should be planned by
  NMP, usually through outbox routing, and relay-pinned subscriptions are the
  explicit exception.
- **#2316:** Serving one feature's state is fragmented across acquisition,
  replay, sink, admission, projection, tick, dependency tracking, and teardown.
  The design must collapse that lifecycle; a convenience helper is not enough.
- **Cache warming:** delivery must happen because a session owns demand,
  replay, activation, output, and teardown. Pre-warming the store, seeding
  caches, or asking shells to retry is a lifecycle bug if the open path still
  cannot hydrate its output by construction.
- **Base query primitive vs feed policy:** multi-author dynamic query/routing is
  substrate/session capability. Follow-feed ranking, recency, viewport windows,
  and fallback behavior are feature policy. A bad feed implementation must not
  become a substrate cap, cache-warming workaround, or public `ReducedSource`
  concept.
- **NDK-style subscribe:** the comparable DX target is not a shell-owned raw
  event stream. It is a Rust-owned session descriptor plus generated host API so
  every shell gets a one-call open/render surface without owning Nostr policy.
- **`nmp.follow_list`:** reusable protocol outputs belong to reusable protocol
  or NMP feature crates. `nmp.follow_list` cannot live in Chirp/FFI glue while
  pretending other apps have a coherent social primitive.
- **#2314/display separation:** presentation vocabulary such as colors, labels,
  icons, and relative formatting stays in shells. Rust outputs semantic facts
  and status tokens only.
- **Operator policy:** reusable NMP composition must not own app relays, seed
  follows, bootstrap relays, signer permissions, onboarding defaults, or product
  policy.
- **No polling:** cache-serve wakeups and session reconciliation should be
  event-driven. A snapshot tick is not a hidden scheduler for product logic.
- **Projection contract:** clear/tombstone, stale-frame, transactional merge,
  baseline, and D6 poison semantics are correctness requirements, not optional
  optimization details.
- **Publish routing:** explicit relay paths, NIP-17 private routing, and NIP-29
  host pins must converge on one publish doorway while keeping fail-closed
  protocol policy outside native shells.
- **Temporal source of truth:** this directory is not a new planning authority.
  Final decisions move into ADRs, durable docs, and GitHub issues.

## Issue #2313 Traceability

| #2313 concern | Design answer | Proof gate |
|---|---|---|
| `register_defaults()` hides the app's real architecture. | Production apps use explicit feature composition; presets are tutorial/test/migration only with owner and deletion/formalization gate. | P0/P8 classify and migrate defaults, `nmp init`, gallery, Highlighter, Podcast Player, and builder-guide teaching. |
| `declare_consumed_projections` looks like a complete manifest but is not. | Session open declares scoped output demand; always-on app chrome is explicit composition; projection tiers/declarations are private executor or compatibility machinery. | P3 proves scoped output demand and stops teaching projection tiers as app concepts. |
| `nmp.follow_list` belongs to reusable NIP-02/NMP, not Chirp/FFI glue. | Reusable protocol projections live in protocol/NMP feature crates; app crates consume them through sessions/outputs. | FF-020 plus P0 owner inventory for every reusable protocol projection. |
| Interest and projection are wired separately, causing silent desync. | A typed session contract owns acquisition, route planning, replay, sink, admission, output, wakes, status, and teardown. | P1 lifecycle-owner proof and FF-018 per-session contract table. |
| `nmp_app_open_interest` is only half an API. | Raw acquisition remains substrate/diagnostic/test/migration only; product reads use typed session handles with pushed typed output. | FF-001 raw-read ratchet and P-1 public-door disposition ledger. |
| `open_*` features leak replay/observer/sidecar/teardown ritual. | Existing safe machinery is compiled behind typed session descriptors; app developers never assemble `ObservedProjection` or sidecars. | P1 proves first descriptor over observed replay; P4/P5 migrate refs/feed/group/search families. |
| NDK/applesauce offer a one-call subscribe mental model. | NMP equivalent is one generated open/render API over a Rust-defined session, with outbox planning, replay, admission, and teardown hidden. | Clean-room app path plus planned/outbox and relay-pinned examples in `02-live-queries.md`. |
| Home feed is not special; default reads should use outbox routing. | Planned routing is the default for public author-scoped reads; relay-pinned/private/explicit routes are named exceptions with provenance. | FF-019 read-route planning contract and P5 dynamic/composite reads. |
| A helper would hide but not fix fragmentation. | Every phase must delete, privatize, or compatibility-scope an old door; layering a new facade over old public recipes fails. | Proof ladder rungs 0-3 and per-slice deletion ledger. |
| Writes should separate construction, signing, and publishing. | Builders construct drafts; finalizers add protocol envelope/route context before signing; signer and publish status stay Rust-owned. | P6 publish route provenance, generated builder, signer continuation, retry/resume, and status tests. |

## Developer-Level Model

From an app developer's perspective, an NMP app is built out of five things:

1. A Rust app crate defines the product and installs features.
2. Screens, components, widgets, and app services open typed sessions for the
   state they need.
3. Rust emits typed outputs; generated adapters make those outputs pleasant to
   render in Swift, Kotlin, TypeScript, TUI, or another shell.
4. User actions become typed intents or generated builder calls.
5. Native/web shells render UI and execute capabilities. Rust decides what those
   capability results mean.

The app developer should think in terms of product features and typed sessions:

```text
install NIP-29 groups
install profile refs
install app-owned playback

open RoomChat(group)
open NostrAvatar(pubkey)
open PodcastPlayback(app_lifetime)

render room messages
render profile row
render playback state

dispatch SendGroupMessage(...)
dispatch TogglePlayback(...)
```

For a new product-specific stream, the developer writes the Rust session once
and gets generated shell calls. NMP should not require a framework PR for every
app read model, but it also should not let a Swift/Kotlin/TypeScript shell own
arbitrary raw subscriptions as the shipped product path.

The app developer should not manually wire:

```text
raw interest + observer + replay + projection sidecar + snapshot tick + teardown
```

That wiring exists, but it is feature/session machinery.

## User-Visible Data Flow

For a user action that reads data:

```text
screen appears
  -> shell opens a typed session
  -> Rust decides the source and route policy
  -> NMP replays cached/store data
  -> NMP subscribes to live relays if needed
  -> Rust reduces events into typed output
  -> shell renders output
  -> screen disappears
  -> shell closes the handle
  -> Rust tears down unowned demand
```

For a user action that writes data:

```text
user taps reply/react/publish
  -> shell dispatches a typed intent
  -> Rust constructs and finalizes the draft
  -> Rust selects the signer
  -> native/web executes the signer capability if needed
  -> Rust validates the signed event
  -> Rust stores it when read-your-writes is allowed
  -> Rust plans publish relays
  -> Rust publishes and records status
  -> shell renders status from typed output
```

The visible product behavior is still simple: open state, render state, dispatch
intent, render updated state. The complex parts are kept behind Rust-owned
feature and runtime seams.

## Crate-Level Flow

The exact crate names can change, but the dependency direction should not:

```text
apps/<app> Rust crate
  -> installs app features and reusable NMP features
  -> depends on NMP crates

crates/nmp-defaults and protocol crates
  -> provide reusable feature bundles, builders, descriptors, parsers, policy
  -> depend on nmp-core seams

crates/nmp-core
  -> owns the actor, state transitions, session lifecycle, projection execution,
     capability requests, signer continuation, publish engine, and typed updates

store/planner/network crates
  -> provide focused infrastructure used by nmp-core and feature crates

runtime/FFI/codegen crates
  -> adapt typed actions, outputs, and capabilities to host platforms

native/web/TUI shells
  -> render outputs and execute raw capabilities
```

`nmp-core` must not import app domains. App crates and protocol crates contribute
typed behavior through seams. Runtime crates adapt the core to host platforms;
they do not decide protocol or product policy.

### Read Path Through Crates

```text
app screen/component
  -> generated/runtime open-session call
  -> app or protocol feature descriptor
  -> nmp-core session lifecycle
  -> nmp-planner / routing policy
  -> nmp-network relay IO when live acquisition is needed
  -> nmp-store / indexed event storage for replay and ingest
  -> nmp-core ObservedProjection and reducers
  -> typed UpdateFrame/output manifest
  -> nmp-ffi, nmp-native-runtime, or nmp-browser-runtime adapter
  -> shell render cache
  -> UI
```

The key point: the shell asks for `RoomChat`, `ProfileRef`, `EventEmbed`, or
`HomeFeed`. It does not ask for naked relay filters and then separately decide
which projections to refresh.

### Write Path Through Crates

```text
user intent in shell
  -> generated typed action / DispatchEnvelope
  -> nmp-ffi or browser/native runtime doorway
  -> nmp-core ActionModule
  -> owning protocol/app feature builder
  -> unsigned-event finalization + routing/privacy context
  -> signer interface and capability bridge
  -> nmp-core publish policy and publish engine
  -> nmp-planner / route policy
  -> nmp-network relay publish
  -> nmp-store local ingest when allowed
  -> typed publish/action status output
  -> shell render
```

The key point: construction, signing, and publishing are separable phases, but
they remain one Rust-owned flow. A native shell can execute a signer or OS
capability, but it does not infer tags, choose relays, retry policy, or publish
state.

### Capability Path Through Crates

```text
Rust feature needs an external effect
  -> nmp-core emits typed capability request
  -> runtime/FFI adapter delivers it to the shell
  -> shell executes OS/API capability
  -> shell reports raw result
  -> nmp-core reducer decides next state
  -> typed output updates UI
```

This is how playback, camera, share extensions, HTTP fetches, Keychain, NIP-55,
NIP-46, Blossom upload, STT, or local AI can fit without moving product policy
into native code.

## Documents

- [App Model](01-app-model.md) explains how an app is assembled, what feature
  bundles provide, and where app-specific Rust domains belong.
- [Live Queries](02-live-queries.md) explains how screens subscribe to data,
  including `ObservedProjection`, dynamic source reconciliation, component refs,
  and projection delivery.
- [Write Flow](03-write-flow.md) explains the split between event construction,
  event finalization, signing, and publishing.
- [Internal Machinery](04-internal-machinery.md) explains what NMP does under
  the hood and the migration milestones needed to delete the old recipes.

## North Star

An NMP app should be understandable from a small set of concepts:

- A Rust composition root installs explicit feature bundles.
- Screens, components, widgets, and app services open typed sessions for the
  data they render or keep resident.
- Shells render typed outputs produced by Rust and hold only projection caches
  generated for rendering.
- Event construction is composable, protocol-aware, and app-crate extensible.
- Signing is explicit enough to choose a signer, but Rust-owned enough to keep
  native backends interchangeable.
- Publishing applies route policy, protocol pins, delivery, retry, and status in
  Rust.
- Native and web shells render UI and execute capabilities. They do not own
  protocol correctness, durable state, relay planning, or product logic.

## Terms Used Here

The names are deliberately provisional ADR candidates. They describe invariants
the design must preserve, not a commitment to add new public types or keep
current internal types:

- `Typed session`, `FeatureSession`, or `LiveQuery` means a typed descriptor and
  handle for the live lifecycle a screen, component, widget, or app service
  opens. `LiveQuery` is acceptable only as naming for that contract, not as a
  second public engine.
- `ObservedProjection` means the internal safe pattern for replaying cached
  events into a scoped projection before accepting future live events.
- `ReducedSource` means one current private feed-local implementation candidate
  for dynamic source reconciliation. It is not the architecture noun unless the
  ADR proves the same semantics across non-feed source families.
- `EventDraft` means the invariant that unsigned event bytes may still be
  finalized before signing. It is not necessarily a new public type.
- `PublishContext` means the invariant that route, privacy, and protocol policy
  travel with a draft or signed event. It is not necessarily a new type.
- `ReactiveCount` means the invariant for live counts derived from a source and
  filter. A dedicated primitive is justified only if typed projections are not
  enough.

The ADR can rename any of these. The shape is the important part.

## What This Must Fix

This design addresses the concerns behind #2313 and #2316 only if the final
implementation satisfies these constraints:

- `open_interest` stops being taught as the app read model. It may remain only
  in named substrate, protocol-internal, diagnostic, test, or migration scopes.
- `register_defaults()` stops being the mental model for real products. It may
  remain as a named preset for examples, tests, or a clearly labeled tutorial
  path only with live consumers, owner, support window, and deletion/formalization
  gate.
- Projection tiers stay internal. The app sees typed outputs and handles, not
  `SnapshotRegistry` categories or sidecar rituals.
- Dynamic sources are first-class. Follow lists, group members, visible thread
  roots, embeds, and source fallbacks are Rust-owned descriptors.
- Writes preserve three separable phases: construction/finalization, signing,
  and publishing. They still run through one Rust-owned action/publish path.
- Explicit write routes preserve provenance: manual overrides, NIP-29 host pins,
  verified private inboxes, and imported/verbatim events are not one anonymous
  relay bucket.
- App crates can define product sessions and builders without moving podcast,
  highlighter, playback, capture, queue, or RSS behavior into NMP crates.
- Generated app-feature APIs are valid for playback, STT/TTS, agents, provider
  catalogs, imports, and capability control, but event-producing work still goes
  through typed actions and publish status.
- Timers are allowed for capability sampling or presentation affordances, not for
  reducer/session reconciliation or projection repair.

## ADR Readiness Bar

This packet is ready to turn into durable ADR and architecture edits only after:

- the public vocabulary is reduced to feature bundles, typed sessions, typed
  actions/builders, capability results, and typed outputs/status;
- `LiveQuery`, `ObservedProjection`, `ReducedSource`, route provenance, generated
  adapters, and wake indexes are classified as public, private, migration-scoped,
  or rejected;
- every accepted session family has a contract for acquisition, routing, replay,
  live sink, admission, output, wakes, teardown, and error/status behavior;
- one simple session and one dynamic-source session prove the descriptor can sit
  on existing safe machinery without creating a second read lifecycle;
- construction/finalization, signing, and publishing are proven separable without
  splitting into native-owned routes or publish JSON paths;
- Highlighter, Podcast Player, and `nmp-gallery` pass their acceptance matrices
  or trigger a named kill criterion;
- the first executable ratchets are identified, with baseline counts and owners;
- stale docs and examples have a retirement path instead of becoming a competing
  source of truth.

## Decision Taxonomy

Do not treat every unresolved item as a human product decision. Most are
technical proof questions that the implementation plan should answer with code,
tests, and downstream migrations.

**Direction this packet already chooses**

- The public app model is feature composition, typed sessions, typed actions or
  generated builders, capability results, and typed output/status.
- `open_interest`, projection tiers, `ObservedProjection`, raw sidecar rituals,
  and snapshot ticks are not app-developer concepts.
- `register_defaults()` is not the production mental model. If retained, it is a
  tutorial, compatibility, or test preset with explicit scope and deletion target.
- NIP-29 is a kind-agnostic group-publish finalizer/route wrapper over already
  constructed events, not a reply/comment/article helper namespace. NIP-29 group
  reads are likewise kind-agnostic over valid group-context events; product
  filtering belongs to consumers.
- Route provenance is required. Exact relay lists alone cannot represent manual
  override, host pin, verified private inbox, automatic route, and imported event
  semantics.
- App-specific Rust crates own product domains; NMP crates own reusable Nostr
  mechanisms. Native/web shells render and execute capabilities only.

**Technical ADR questions**

These do not need subjective product input unless the evidence produces two
valid endpoints:

- final public naming: `typed session`, `FeatureSession`, or per-feature open
  helpers over one descriptor model;
- whether private `ObservedProjection` can be narrowed or should remain as the
  internal replay-before-live primitive;
- whether private feed-shaped `ReducedSource` is amended, renamed, replaced by a
  smaller reconciler, or kept private until another source family proves it;
- exactly how route provenance fits in existing publish target/command fields or
  one narrow internal enum/type;
- how far generation must go before schema/action/cache drift is actually
  prevented;
- which compatibility doors have live consumers and which should be deleted
  rather than formalized.

**Human/product-scope calls**

These can be documented and worked around temporarily, but final signoff needs a
real decision if the evidence does not force one:

- whether Highlighter web is an NMP target runtime, an SSR/migration exception,
  or deliberately out of scope;
- whether a separate tutorial preset should exist in addition to the production
  `nmp init` scaffold;
- which downstream migrations are release gates versus tracked follow-up work
  after the architecture is accepted;
- whether manual explicit relay selection is a product affordance, and what
  audit language/ownership the product wants users to see.
