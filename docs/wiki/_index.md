# Wiki Index

> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.

Last updated: 2026-07-02

## action-dispatch (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [action-dispatch](guides/action-dispatch.md) | Action Registration and Dispatch | Registering an action means implementing one trait; the framework owns dispatch | capture | warm | 2026-06-29 | action-dispatch |

## adr-governance (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [adr-governance](guides/adr-governance.md) | ADR Lifecycle and Governance | ADR-0073 is the framing-rule ADR that establishes the 'not a museum' principle for the ADR directory and the ratchet/follow-up discipline for folded/amended ADR | capture | warm | 2026-06-29 | adr-governance |

## app-defined-kinds (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [app-defined-kinds](guides/app-defined-kinds.md) | App-Defined Event Kinds: First-Class Support and Codegen | An app should be able to define its own made-up event kind â number, schema, builder â and have it be a first-class citizen in the app's own codebase, on pa | capture | warm | 2026-06-29 | app-defined-kinds |

## app-lifecycle (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [app-lifecycle](guides/app-lifecycle.md) | NmpApp Lifecycle and Shutdown | The UniFFI runtime object exposes an explicit idempotent `shutdown()` method (not `close`, to avoid Kotlin friction from #2149) | capture | warm | 2026-06-29 | app-lifecycle |

## autonomous-loop (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [autonomous-loop](guides/autonomous-loop.md) | Autonomous Refactor Loop | The autonomous loop runs once per hour | capture | warm | 2026-06-29 | autonomous-loop |

## ci-gates (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [ci-gates](guides/ci-gates.md) | CI Gate Policies During Migration | During the migration, CI checks that help identify issues as we build are kept, but unnecessary CI gates that slow things down while things are supposed to be b | capture | warm | 2026-06-29 | ci-gates |

## codex-usage (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [codex-usage](guides/codex-usage.md) | Codex Usage Policy | Codex is used sparingly, only at major milestones | capture | warm | 2026-06-29 | codex-usage |

## component-registry (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [component-registry](guides/component-registry.md) | Component Registry Unification | The project maintains two component registries that have historically existed as separate artifacts: the gallery showcase catalog (`apps/nmp-gallery/registry.js | capture | warm | 2026-06-29 | component-registry |

## composition-root (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [composition-root](guides/composition-root.md) | Explicit Composition Root and register_defaults Elimination | Per ADR-0069, the composition root requires `register_defaults()` to be dead as a production path everywhere â in the starter, gallery, and browser | capture | warm | 2026-06-29 | composition-root |

## crate-ownership (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [crate-ownership](guides/crate-ownership.md) | NMP Crate Ownership and Helper Policy | Anything a second platform would have to reimplement to stay correct â relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning â | capture | warm | 2026-06-29 | crate-ownership |

## data-persistence (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [browser-storage-init](guides/browser-storage-init.md) | Browser Durable Storage Initialization | Browser durable storage (Worker/OPFS) must initialize before the product starts | capture | warm | 2026-06-29 | data-persistence |

## deletion-ledger (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [deletion-ledger](guides/deletion-ledger.md) | Architecture Deletion Ledger and Ratchets | Each architecture slice carries a deletion ledger that tracks old doors deleted or privatized, new concepts introduced, and the net change in permanent concepts | capture | warm | 2026-06-29 | deletion-ledger |

## dx-proof (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [dx-proof](guides/dx-proof.md) | DX Clean-Room Proof Gate | Issue #2256 is the clean-break DX gate: a clean-room onboarding proof | capture | warm | 2026-06-29 | dx-proof |

## head-coordinator (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [head-coordinator](guides/head-coordinator.md) | Head Coordinator and PR Workflow | The head-coordinator merges clean pull requests | capture | warm | 2026-06-29 | head-coordinator |

## issue-queue (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [issue-queue](guides/issue-queue.md) | Issue Queue as Canonical Tracker | The issue queue is the single canonical temporal tracker for the project â not a museum | capture | warm | 2026-06-29 | issue-queue |

## project-status (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [in-tree-harnesses](guides/in-tree-harnesses.md) | In-Tree Conformance Harnesses and Extracted Apps | Gallery stays in-tree as a cross-platform conformance and regression harness â a storybook proving every NMP component decodes and renders on every platform a | capture | warm | 2026-06-29 | project-status |
| [project-status](guides/project-status.md) | NMP Project Status: NIP Scope and ADR Spine | EPIC-NS-001 (#2340) is the governing p0 north-star epic for the clean-break NMP app architecture migration; all active slices trace back to it | capture | warm | 2026-06-29 | project-status |

## publish-workflow (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [draft-builders](guides/draft-builders.md) | Draft Builder Composability and Side-Effect Limits | Event construction is composable: template event builders (such as react_to_event or reply_to_event) produce unsigned draft events, and the publish action may t | capture | warm | 2026-06-29 | publish-workflow |

## read-door (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [read-door](guides/read-door.md) | The Read Door: Typed Read Sessions and API Surface | The read door follows the typed sessions architecture established in ADR-0070 | capture | warm | 2026-06-29 | read-door |

## uniffi-migration (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [uniffi-migration](guides/uniffi-migration.md) | M14 UniFFI Native Surface Migration | The M14 epic (#2125) collapses the native public binding surface to UniFFI: one public UniFFI surface serves iOS and Android, with FlatBuffers `Vec<u8>` bytes r | capture | warm | 2026-06-29 | uniffi-migration |

## write-pipeline (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [relay-provenance](guides/relay-provenance.md) | Relay Route Provenance and Access Control | Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic | capture | warm | 2026-06-29 | write-pipeline |
| [write-pipeline](guides/write-pipeline.md) | The Write Pipeline: Construction, Signing, Publishing | The 'door' metaphor refers to the app-facing API surface: the read door is how an app consumes data (typed read sessions), and the write door is how an app prod | capture | warm | 2026-06-29 | write-pipeline |

## Research Records (11 records)

| Record | Date | Finding | Agent |
|--------|------|---------|-------|
| [2026-06-29-1-codegen-extensibility-trace-can-app-owned](research/2026-06-29-1-codegen-extensibility-trace-can-app-owned.md) | 2026-06-29 | Codegen extensibility trace: can app-owned event kinds get first-class generated typed builders? Verdict: NO — codegen is hardcoded to NMP's own NIPs with no external registry hook | a96c931d134848ae1 |
| [2026-06-29-1-codegen-pipeline-trace-investigating-whether-app](research/2026-06-29-1-codegen-pipeline-trace-investigating-whether-app.md) | 2026-06-29 | Codegen pipeline trace investigating whether app-owned event kinds can get first-class generated typed builders; verdict: NO — codegen is hardcoded to NMP's schemas, no extensibility hook exists | a96c931d134848ae1 |
| [2026-06-29-1-codegen-seam-investigation-tracing-whether-app](research/2026-06-29-1-codegen-seam-investigation-tracing-whether-app.md) | 2026-06-29 | Codegen seam investigation tracing whether app-owned event kinds get first-class generated builders; verdict: NO, with file:line evidence across 5 evidence sections | a96c931d134848ae1 |
| [2026-06-29-1-codegen-seam-trace-investigation-verdict-no](research/2026-06-29-1-codegen-seam-trace-investigation-verdict-no.md) | 2026-06-29 | Codegen seam trace investigation: verdict NO — app-owned event kinds cannot get first-class typed write builders today, with file:line evidence across 5 evidence sections and gap analysis | a96c931d134848ae1 |
| [2026-06-29-1-investigation-of-whether-app-defined-event](research/2026-06-29-1-investigation-of-whether-app-defined-event.md) | 2026-06-29 | Investigation of whether app-defined event kinds can get first-class typed builders in their own codebase; verdict: NO, codegen pipeline is hardcoded to NMP only | a96c931d134848ae1 |
| [2026-06-29-1-uniffi-vs-c-abi-callback-byte](research/2026-06-29-1-uniffi-vs-c-abi-callback-byte.md) | 2026-06-29 | UniFFI vs C-ABI callback byte-push transport A/B microbench, methodology_sound, verdict COLLAPSE (pure UniFFI adoption confirmed, weighted-p99 delta 2142ns = 0.013% of 60fps budget, 390× below collapse threshold) | main |
| [2026-06-29-1-uniffi-vs-c-callback-byte-push](research/2026-06-29-1-uniffi-vs-c-callback-byte-push.md) | 2026-06-29 | UniFFI-vs-C-callback byte-push A/B benchmark (#2388): pre-registered COLLAPSE/KEEP/ESCALATE decision rule against 60fps budget thresholds; measured surcharged weighted-p99 delta = 2,142 ns (0.013% of 16.67ms budget), verdict COLLAPSE — pure UniFFI confirmed, ~390× below threshold | main |
| [2026-06-29-1-uniffi-vs-c-callback-byte-transport](research/2026-06-29-1-uniffi-vs-c-callback-byte-transport.md) | 2026-06-29 | UniFFI vs C-callback byte-transport A/B microbenchmark — COLLAPSE verdict: pure UniFFI sufficient, surcharged weighted-p99 delta 2,142 ns = 0.013% of 60fps budget, ~390× below pre-registered threshold | subagent (workflow #2388) |
| [2026-06-29-1-uniffi-vs-c-lane-byte-push](research/2026-06-29-1-uniffi-vs-c-lane-byte-push.md) | 2026-06-29 | UniFFI vs C-lane byte-push transport A/B microbench; pre-registered decision rule and verdict bands; COLLAPSE verdict (pure UniFFI sufficient) | workflow |
| [2026-06-29-1-uniffi-vs-c-lane-byte-transport](research/2026-06-29-1-uniffi-vs-c-lane-byte-transport.md) | 2026-06-29 | UniFFI vs C-lane byte transport benchmark: COLLAPSE verdict — surcharged weighted-p99 delta 2,142 ns is 390× below pre-registered threshold across 3 runs | main |
| [AGENTS](research/AGENTS.md) |  |  |  |

## Episode Cards (2 cards)

| Card | Date | Title | Salience | Status |
|------|------|-------|----------|--------|
| [2026-06-29-1-collapse-uniffi-performance-assumption-unify-to](episodes/2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.md) | 2026-06-29 | Collapse UniFFI performance assumption; unify to single-surface architecture | reversal | active |
| [2026-06-29-1-disable-perf-gates-during-clean-break](episodes/2026-06-29-1-disable-perf-gates-during-clean-break.md) | 2026-06-29 | Disable perf-gates during clean-break migration | architecture | active |

## Nouns (65 entities)

| Noun | Name | Origin | Definition |
|------|------|--------|------------|
| [action-builders](nouns/action-builders.md) | ACTION_BUILDERS | extracted | A hardcoded public const array of hand-coded ActionBuilder struct entries — the schema source-of-truth for the action-builder codegen pipeline. Contains only NMP's built-in NIPs with no mechanism for external/app-provided entries. |
| [actionmodule](nouns/actionmodule.md) | ActionModule | extracted | the per-NIP adapter that owns write action execution; implements the ActionModule trait with execute() entry point; receives generic signer capability and routes by provenance |
| [adr-directory](nouns/adr-directory.md) | ADR directory | extracted | The decision record archive is not a museum; old ADRs survive only when they preserve invariants that don't conflict with the current spine; otherwise they are folded, amended, or retired in place |
| [adrs](nouns/adrs.md) | ADRs | extracted | The decision spine — they are not the fast developer architecture guide. The ADR file itself is authoritative for its own status; the index is only a navigation aid. |
| [app-owned-kinds](nouns/app-owned-kinds.md) | App-owned kinds | extracted | event kinds (with custom schemas/numbers) defined and owned within an app's own codebase, distinct from NMP's built-in NIPs; executed via ActionModule registration at composition root |
| [architecture-decision-record-adr](nouns/architecture-decision-record-adr.md) | Architecture Decision Record (ADR) | extracted | a document that preserves durable decision context; must be edited in place to describe the current design rather than preserving outdated guidance |
| [builtin-registry-sections](nouns/builtin-registry-sections.md) | BUILTIN_REGISTRY_SECTIONS | extracted | include_str! of per-platform `nmp add` install manifests (registry.toml, registry.swiftui.toml, etc.), not a hardcoded section list duplicating the gallery catalog. |
| [chirp](nouns/chirp.md) | Chirp | extracted | A real Nostr client product extracted to an external repository as proof of consumability from outside the monorepo |
| [clean-break-nmp-app-architecture-migration-epic-ns-001](nouns/clean-break-nmp-app-architecture-migration-epic-ns-001.md) | Clean-break NMP app architecture migration (EPIC-NS-001) | extracted | the single overarching north-star epic; a coordinated three-reform redesign (read door / composition root / write door) applied lockstep across native/browser/starter targets with doctrine-lint ratchets |
| [clean-break-redesign](nouns/clean-break-redesign.md) | clean-break redesign | extracted | A coordinated three-reform redesign governed by ADR spine 0069–0073: kill register_defaults() (explicit composition), retire raw interest/feed C ABI in favor of typed sessions (read door), and classify publish routes with explicit pre-sign provenance (write door), applied to native, browser, and starter targets with doctrine-lint ratchets locking each slice shut. |
| [cli-install-registry](nouns/cli-install-registry.md) | CLI install registry | extracted | per-platform component install manifests; what `nmp add` scaffolds |
| [cli-install-registry-nmp-cli-registry](nouns/cli-install-registry-nmp-cli-registry.md) | CLI install registry (nmp-cli registry) | extracted | crates/nmp-cli/registry/*.toml — per-platform install manifests for `nmp add` (version + source files + deps), split by platform target for the 500-LOC gate. A legitimately separate concern from the gallery showcase catalog. |
| [collapse-verdict](nouns/collapse-verdict.md) | COLLAPSE verdict | extracted | The measured benchmark result that pure UniFFI has sufficient performance for all surfaces including the hot update-sink push lane, eliminating the need for an internal byte-ABI exception |
| [composition-root](nouns/composition-root.md) | composition root | extracted | The only place where a production app chooses product policy; explicitly installs substrate, reusable Nostr protocol features, app-owned product features, shell capability contracts, then starts the runtime |
| [deletion-ledger](nouns/deletion-ledger.md) | deletion ledger | extracted | Per-architecture-slice accounting of old doors deleted/privatized, new concepts, and net permanent concepts — tracks the concept/surface collapse that LOC numbers understates |
| [dispatch-success](nouns/dispatch-success.md) | Dispatch ≠ success | extracted | the doctrine that dispatching an event to publish pipeline is orthogonal from confirming receipt on relays; Rust owns tracked status intent that survives offline |
| [dispatchenvelope](nouns/dispatchenvelope.md) | DispatchEnvelope | extracted | FlatBuffers encoding for a typed action, pushed through dispatch_action — the single FFI command lane (ADR-0064), identical in shape across native and browser |
| [dispatchoutcome](nouns/dispatchoutcome.md) | DispatchOutcome | extracted | Typed result of action dispatch carrying correlation_id, error, and code field; replaces raw JSON for typed command execution |
| [doc-vocabulary-ratchets](nouns/doc-vocabulary-ratchets.md) | doc/vocabulary ratchets | extracted | CI lint gates that stop old register_defaults/raw-projection vocabulary from creeping back into docs and code. They are pro-migration — the opposite of friction-for-nothing — enforcing the clean-break by failing CI if old vocabulary re-enters. |
| [door-read-door-write-door](nouns/door-read-door-write-door.md) | door (read door / write door) | extracted | Metaphor for the app-facing API surface. Read door = how an app consumes data (typed read sessions). Write door = how an app produces and emits Nostr events (construct → sign → publish pipeline). |
| [dx-clean-room-proof-2256](nouns/dx-clean-room-proof-2256.md) | DX clean-room proof (#2256) | extracted | A DX gate requiring a fresh developer to build a small NMP app in ≤2h from published repo docs and starter artifacts alone, using only the clean-break public model (explicit composition root, typed read sessions, generated typed writes). It supersedes a prior clean-room proof that validated the wrong (pre-reset) model. Passing it unblocks release blocker #2121 and is the migration-readiness starting line. |
| [epic-ns-001](nouns/epic-ns-001.md) | EPIC-NS-001 | extracted | The single overarching north-star epic (#2340) driving the clean-break NMP app architecture migration — elimination of register_defaults(), raw open_interest, ObservedProjection/ReducedSource, platform-specific ABI choices, and compatibility shims; replaced with explicit composition, typed read sessions, construction/signing/publishing separation, and one UniFFI native surface. |
| [epic-ns-001-clean-break-redesign](nouns/epic-ns-001-clean-break-redesign.md) | EPIC-NS-001 (clean-break redesign) | extracted | The single overarching north-star p0 epic (#2340) driving the clean-break NMP app architecture migration. It governs a coordinated three-door reform — kill register_defaults, retire raw interest/feed C ABI in favor of typed sessions, classify publish routes with explicit pre-sign provenance — applied to native, browser, and starter targets in lockstep with doctrine-lint/doc ratchets locking each slice shut. |
| [explicit-composition-adr-0069](nouns/explicit-composition-adr-0069.md) | Explicit Composition (ADR-0069) | extracted | explicit wiring of app composition root at build time, replacing `register_defaults()` as the production path |
| [first-class-for-app-owned-kinds](nouns/first-class-for-app-owned-kinds.md) | First-class (for app-owned kinds) | extracted | full generated-builder treatment (typed Rust builder, generated Swift/Kotlin/TS bindings, drift gating) within an app's own codebase, on par with NMP's built-in kinds; not hand-rolled ActionModules |
| [gallery-showcase-catalog](nouns/gallery-showcase-catalog.md) | Gallery showcase catalog | extracted | The platform-agnostic component catalog defined in `apps/nmp-gallery/registry.json` with `sections[]` structure, containing the canonical list of components the gallery demonstrates |
| [gallery-showcase-catalog-registry-json](nouns/gallery-showcase-catalog-registry-json.md) | gallery showcase catalog (registry.json) | extracted | platform-agnostic component showcase catalog; what the gallery demos |
| [issue-queue](nouns/issue-queue.md) | issue queue | extracted | The single canonical temporal tracker — supposed to hold deferred and future work. Not a museum; 'predates the epic' is not by itself a reason to remove anything. The test for each issue is whether it is still true, still independent, and not contradicted by the reset. |
| [m14](nouns/m14.md) | M14 | extracted | The epic to collapse the native public binding surface to a single UniFFI surface for iOS/Android, replacing the transitional raw C/JNI ABI (nmp-ffi); FlatBuffers Vec<u8> bytes remain payload, wasm-bindgen stays separate for browser. |
| [m14-collapse-native-public-binding-surface-to-uniffi](nouns/m14-collapse-native-public-binding-surface-to-uniffi.md) | M14 – Collapse native public binding surface to UniFFI | extracted | post-v1 epic to consolidate native (iOS/Android) public FFI to one UniFFI surface; eliminate legacy C/JNI public lanes; 56 symbols migrate, zero internal-ABI exceptions |
| [nmp-cli-component-install-registry](nouns/nmp-cli-component-install-registry.md) | nmp-cli component-install registry | extracted | Per-platform install manifests in `crates/nmp-cli/registry/*.toml` that define what components the `nmp add` scaffolding tool can install for each platform (swiftui, compose, tui, desktop, web) |
| [nmp-core-rule-rust-owns-durable-behavior](nouns/nmp-core-rule-rust-owns-durable-behavior.md) | NMP core rule (Rust owns durable behavior) | extracted | NMP inherits RMP's core rule: Rust owns durable behavior and each platform renders native UI. Anything a second platform would have to reimplement to stay correct (relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning) belongs in Rust. |
| [nmp-defaults](nouns/nmp-defaults.md) | nmp-defaults | extracted | A reusable installer library only — never owning seed follows, bootstrap relay brands, signer permission defaults, or onboarding/product policy; register_defaults() is killed as production app architecture. |
| [nmp-ffi-nmp-native-runtime-crate-roles](nouns/nmp-ffi-nmp-native-runtime-crate-roles.md) | nmp-ffi / nmp-native-runtime (crate roles) | extracted | nmp-ffi is C-ABI glue; nmp-native-runtime owns lifecycle. Under the UniFFI collapse, nmp-native-runtime also owns the FFI-free dispatch core that both C-ABI and UniFFI consume. |
| [nmp-gallery](nouns/nmp-gallery.md) | nmp-gallery | extracted | A conformance/regression harness — a storybook that proves every NMP component decodes and renders on every platform at HEAD, with value in breadth + currency, not shippability |
| [nmp-uniffi](nouns/nmp-uniffi.md) | nmp-uniffi | extracted | A new in-tree crate providing UniFFI 0.29 proc-macro bindings wrapping nmp-native-runtime. Exposes NmpApp (Arc-wrapped: start/configure/stop/reset/shutdown/dispatch_action/set_update_sink), UpdateSink callback interface, and DispatchOutcome record. |
| [nmpgallery](nouns/nmpgallery.md) | NmpGallery | extracted | A cross-platform conformance/regression harness (storybook) proving every NMP component decodes and renders on every platform at HEAD — not a shippable product. Its value is breadth + currency, which is why it stays in-tree unlike product apps like Chirp. |
| [observedprojection](nouns/observedprojection.md) | ObservedProjection | extracted | Private machinery (not app vocabulary) under ADR-0070's typed read sessions; carries replay-before-live and scoped delivery lessons but is no longer app-facing. |
| [observedprojection-reducedsource](nouns/observedprojection-reducedsource.md) | ObservedProjection / ReducedSource | extracted | Private machinery, not app vocabulary. They survive as internal replay/source-arrival mechanisms but are not exposed as the way app developers assemble product screens. |
| [open-interest](nouns/open-interest.md) | open_interest | extracted | Acquisition-only substrate demoted from app-facing read model; the typed read session owns the complete lifecycle above it. |
| [perf-gates](nouns/perf-gates.md) | perf-gates | extracted | Modelled (deterministic, no live relays) perf benchmarks promoted to PR-blocking gates — runs reactivity-bench and firehose-bench with reduced event volume to keep wall-clock under ~5 min while exercising all absolute gate thresholds. |
| [pre-signed-verbatim-publish](nouns/pre-signed-verbatim-publish.md) | Pre-signed verbatim publish | extracted | Publishing an already-signed event without re-signing, routed via the event's own pubkey outbox. Needed for protocol-owned events (e.g. Marmot/MLS wire events); WRITE-005 restricts it to those protocol seams so it can't be used as a general app write door. |
| [provenance-class](nouns/provenance-class.md) | Provenance class | extracted | A typed enumeration that classifies the source and permission basis for explicit relay routes: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic |
| [provenance-in-publish-context](nouns/provenance-in-publish-context.md) | Provenance (in publish context) | extracted | explicit classification of why/how a publish is routed (outbox relays, NIP-29 group, etc.); non-anonymous and predetermined; cannot default |
| [publish-intent](nouns/publish-intent.md) | publish intent | extracted | Separable construction → finalization → signing → publishing stages as one actor-owned workflow, with typed provenance class for each route (automatic / host-pin / verified-private-inbox / manual / imported / diagnostic) |
| [publish-provenance-class](nouns/publish-provenance-class.md) | publish provenance class | extracted | typed enum for the source/path of an explicit relay route: automatic / host-pin / verified-private-inbox / manual / imported / diagnostic |
| [publish-route-provenance-adr-0071](nouns/publish-route-provenance-adr-0071.md) | Publish Route Provenance (ADR-0071) | extracted | explicit classification of publish routes in core; finalization moved to pre-sign gate; dispatch orthogonal from success |
| [publish-signed-event-pre-signed-verbatim-publish](nouns/publish-signed-event-pre-signed-verbatim-publish.md) | publish_signed_event (pre-signed verbatim publish) | extracted | Publishing an already-signed event without re-signing, routed via the event's own pubkey outbox — needed for protocol-owned events (Marmot/MLS wire events); WRITE-005 restricts it to protocol-owned seams so it can't be used as a general app write door |
| [publishraw](nouns/publishraw.md) | PublishRaw | extracted | A generic low-level 'publish any bytes' seam. WRITE-001 banned it from DX/starter paths as the normal way to write; it remains underneath as the generic substrate builders are built on, but typed intentful writes are the sanctioned app-facing door. |
| [ratchets](nouns/ratchets.md) | ratchets | extracted | Mechanism that prevents old-pattern counts from rising — each slice must keep old public-surface counts flat-or-decreasing, or update the owning ADR in place |
| [read-door](nouns/read-door.md) | Read door | extracted | The app-facing API surface for how an app consumes data — typed read sessions ('given the store, what should the UI see?'), replacing raw open_interest/ObservedProjection/ReducedSource which are now crate-private. |
| [redesign-spine](nouns/redesign-spine.md) | Redesign spine | extracted | ADRs 0069 through 0073; the current set of architectural decision records that define the clean-break app architecture direction |
| [reducedsource](nouns/reducedsource.md) | ReducedSource | extracted | Private machinery (not app vocabulary) carrying source-arrival/source-withdrawal lessons — demoted from app-facing read architecture to internal implementation under typed read sessions. |
| [register-defaults](nouns/register-defaults.md) | register_defaults() | extracted | A preset/function that provided an opaque bundle of protocol features, runtimes, projections, and policy; the hidden composition model that app developers could call without understanding what they received |
| [registry-json](nouns/registry-json.md) | registry.json | extracted | The single source of truth for the NMP Gallery component showcase catalog; Rust hosts read the typed value directly, iOS reads the same JSON through a C-ABI accessor, and Android reads it through JNI |
| [signerprovenance](nouns/signerprovenance.md) | SignerProvenance | extracted | A typed enum that replaces the loose raw optional signer_pubkey in publish commands — names which identity is signing and whether it is known, with structured failure for unknown signers. |
| [status-fact](nouns/status-fact.md) | status fact | extracted | A Rust-owned local publish intent / status object tracking the lifecycle of a write: pending → signed → stored → planned → sent → failed/exhausted. It makes dispatch ≠ success — the app gets honest, offline-first write state instead of fire-and-forget. |
| [typed-read-session](nouns/typed-read-session.md) | typed read session | extracted | One typed session descriptor+handle owning the complete read lifecycle (acquisition → route policy → bounded replay → live sink → admission → typed output → wake sources → teardown); `open_interest` and `ObservedProjection`/`ReducedSource` become private machinery, not app vocabulary |
| [typed-read-sessions](nouns/typed-read-sessions.md) | Typed read sessions | extracted | The app-visible read model: one typed session descriptor and handle owning the complete lifecycle from acquisition through route policy, bounded replay, live sink, admission predicates, typed output, wake sources, to teardown |
| [typed-read-sessions-adr-0070](nouns/typed-read-sessions-adr-0070.md) | Typed Read Sessions (ADR-0070) | extracted | the sole app-facing read API; replaces raw `open_interest`, `ObservedProjection`, `ReducedSource`, and raw feed/interest C-ABI with typed session doorways |
| [uniffi](nouns/uniffi.md) | UniFFI | extracted | A binding surface generator; not the data payload path, which FlatBuffers owns. The binding surface and the data payload are separable — FlatBuffer frames can be passed through UniFFI as bytes. |
| [uniffi-in-this-project](nouns/uniffi-in-this-project.md) | UniFFI (in this project) | extracted | A binding surface generator for native platforms; not the data payload path (FlatBuffers owns that). The decision is one UniFFI surface for all native (iOS/Android/desktop); browser stays wasm-bindgen. |
| [uniffi-nmp-native-strategy](nouns/uniffi-nmp-native-strategy.md) | UniFFI (NMP native strategy) | extracted | One unified UniFFI surface for all native platforms (iOS/Android/desktop); browser stays wasm-bindgen; FlatBuffers remains the typed payload passed THROUGH UniFFI bindings |
| [updatesink](nouns/updatesink.md) | UpdateSink | extracted | A UniFFI callback trait that receives typed update frames with proven quiescence semantics, verified to handle shutdown during in-flight callbacks without deadlock or use-after-free |
| [write-door](nouns/write-door.md) | Write door | extracted | the app-facing API for producing/emitting Nostr events; structured as orthogonal construct → sign → publish phases with explicit provenance classification; dispatch ≠ success |

