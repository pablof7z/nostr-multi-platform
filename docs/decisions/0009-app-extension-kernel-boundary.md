# ADR 0009: App extension kernel boundary

**Date:** 2026-05-17
**Status:** accepted
**Adopts:** `docs/design/app-extension-kernel.md` (the design proposal)
**Modifies:** ADR-0006 (slice positioning), ADR-0008 (Twitter clone repositioned as first extension module)
**Companion ADR:** 0010 (resolves open question 1: generated app enum vs type-erased registry)

## Context

The product spec as originally written placed 15 social-client-shaped view kinds (Profile, Timeline, Thread, Reactions, Conversation, ...) directly in `nmp-core`, alongside a closed `AppAction` enum and a closed `AppUpdate` enum. The first developer to build something that isn't a social client — Highlighter, TENEX, Win the Day, Cut Tracker, podcast apps — has two bad options:

- Add their app nouns (`Highlight`, `Project`, `Episode`, `WeightLog`, `DailyPlan`) to `nmp-core`. The framework becomes a junk drawer of every consumer's domain concepts. Untenable.
- Build them in Swift/Kotlin. Violates the doctrine ("no business logic in native code") the framework exists to enforce.

The proposal in `docs/design/app-extension-kernel.md` identifies this as a fundamental abstraction error. It argues for reframing NMP as a **Nostr-native app kernel with first-class extension modules** rather than a framework with closed built-ins.

This ADR formally accepts that reframing.

## Decision

`nmp-core` provides **generic infrastructure only**:

- Actor runtime and unidirectional state flow.
- Verified Nostr event store with replaceable/delete/expiration semantics.
- Subscription planner with composite-keyed reverse index.
- Relay routing and publish pipeline.
- Signer/session plumbing (identity-scope agnostic).
- Domain-store substrate for non-Nostr records.
- Typed view registry (driven by `ViewModule` trait).
- Durable action ledger (driven by `ActionModule` trait).
- Capability bridge (driven by `CapabilityModule` trait).
- Platform-shadow + codegen machinery.
- Diagnostics and test harnesses.

`nmp-core` does **not** contain:

- Profile, Timeline, Thread, Reactions, Conversation, or any other view-kind business logic.
- Wallet, messaging, blossom, or any other domain feature.
- A closed `AppAction` enum or `AppUpdate` enum.
- A closed `ViewSpec` enum.
- App-specific identity concepts (agent, feedback identity, coach, etc.).

**Four layers, clear ownership.**

| Layer | Owns | May contain app nouns? |
|---|---|---|
| `nmp-core` kernel | actor, store substrate, planner, ledger, registries, codegen, diagnostics | No |
| NMP protocol modules (`nmp-nip01`, `nmp-nip17`, `nmp-nip29`, `nmp-nip65`, `nmp-blossom`, `nmp-nwc`, …) | reusable Nostr protocol concepts: Event, Filter, Keys, gift-wrap, groups, mailboxes, blossom, NWC | Only protocol nouns |
| App core crate (`twitter-core`, `highlighter-core`, `tenex-core`, …) | app domain records, view modules, action modules, app-specific capability types, policies | Yes |
| Platform shell | rendering, OS handle execution, generated wrappers | No policy nouns beyond UI labels |

**Five extension trait families** (concrete signatures in `docs/design/kernel-substrate.md`):

- `DomainModule` — durable non-Nostr records with migrations and indexes.
- `ViewModule` — typed reactive projections with payloads and deltas.
- `ActionModule` — durable workflows on the action ledger.
- `CapabilityModule` — typed native fact reports.
- `IdentityModule` — signer scopes beyond "active Nostr account."

**The rule.** If implementing Highlighter, TENEX, Win the Day, Cut Tracker, or a podcast app requires adding domain nouns to `nmp-core`, the extension boundary is wrong and the kernel must change, not the app.

## What changes from prior ADRs

- **ADR-0006 (vertical-slice-first):** the slice's discipline survives — running code at every checkpoint, one architectural ingredient per sub-phase. The slice's *target* changes: the kind:0 Profile path is now built as the canonical `ViewModule` in a Nostr-protocol module, not as a built-in feature of `nmp-core`. The slice now proves the extension boundary first, then the protocol module on top.
- **ADR-0008 (Twitter clone):** the Twitter clone is repositioned as **the first canonical extension module** demonstrating the kernel boundary at scale, not as the framework's set of built-in features. The sub-phase plan grows by one or two phases (extension-boundary prototype with a tiny non-Nostr fixture module lands before the Twitter slice begins).
- **`product-spec.md` §4 (crate roster), §6.2–§6.6 (state/action/update/capabilities/views), §7 (most subsystem specs), §12 (phasing):** rewritten to reflect kernel + protocol-module + app-module layering. Built-in view kinds become "reference protocol modules with their own view modules." Built-in wallet / messages / blossom become protocol modules (`nmp-nwc`, `nmp-nip17`, `nmp-blossom`).
- **`view-catalog.md`:** reframed as the catalog of *reference Nostr extension modules* shipped with the framework (Profile, Contacts, Timeline, Thread, Reactions, Conversation, ...). Apps can use them, ignore them, or replace them. They are not in `nmp-core`.

## What survives intact

The reactive machinery and platform shadow doctrine are independent of who owns the noun:

- ADR-0001 (composite dependency keys): applies to any view module's `Dependencies` declaration.
- ADR-0002 (per-view delta budget): applies to any view module's emitted deltas.
- ADR-0003 (working-set memory): applies to the kernel's hot/cold split regardless of module count.
- ADR-0004 (allocation measurement): applies to the kernel.
- ADR-0005 (domain-keyed platform shadow): applies to any view module; per-module wrapper API is generated, not hand-written.
- ADR-0007 (diagnostics bridge): applies to any extension's relay status, action ledger entries, domain records, and capability reports.

The doctrines from `product-spec.md` §1.5 (D1 best-effort rendering, D2 negentropy first, D3 outbox automatic, D4 single writer per fact, D5 snapshots bounded by what's open) survive intact and apply across all modules.

## Consequences

- **Smaller kernel, larger ecosystem surface.** `nmp-core` shrinks substantially. The ecosystem grows: `nmp-nip01`, `nmp-nip02`, `nmp-nip17`, `nmp-nip25`, `nmp-nip29`, `nmp-nip65`, `nmp-nip77`, `nmp-blossom`, `nmp-nwc`, `nmp-cashu` become first-class protocol modules.
- **Codegen is critical-path infrastructure.** Without `nmp gen modules` producing per-app concrete enums and platform wrappers, every app reinvents the EventBridge pattern. The codegen tool ships in v1.
- **Phase 1a takes longer.** ADR-0008's 8-week estimate grows to roughly 12–15 weeks. The kernel substrate (1a.1) and tiny fixture module land before the first Nostr-shaped extension module. Twitter clone follows on top.
- **Two fixture apps prove the boundary in v1.** A tiny non-Nostr-shaped fixture (e.g., a TODO/notes module with an app-local identity) lands in 1a.1; the Twitter clone is the first Nostr-shaped extension module. Together they prove the kernel works in both directions.
- **The proof slices from the proposal (Highlighter-lite, Personal-coach-lite, TENEX-lite, Podcast-lite) become post-v1 demonstrations.** They are not v1 deliverables. They are evidence the boundary is right.
- **Future protocol-spec evolution is cleaner.** A new NIP (say NIP-100) becomes a new crate, not a `nmp-core` patch.

## Acceptance criteria for the boundary

Verified by:

1. The kernel-substrate prototype (Phase 1a.1) ships with a tiny non-Nostr fixture module (one DomainModule + one ViewModule + one ActionModule) that compiles, runs on desktop iced, and contains no Nostr concepts.
2. The Twitter clone (Phase 1a.2 onward) is implemented entirely as extension modules over `nmp-core` + protocol modules. `nmp-core` does not gain any of: `Profile`, `Timeline`, `Thread`, `Reactions`, `Conversation`, `Tweet`, `Compose` types or actions.
3. A future hypothetical Highlighter-lite module can be added without changes to `nmp-core`. (Not built in v1; the design must support it.)

## Alternatives considered

- **Keep current spec; punt extension boundary to v2.** Rejected — shipping v1 with the wrong abstraction creates a major-version migration within a year. Worse than ~4 extra weeks of design now.
- **Make `AppAction` / `ViewSpec` extensible via downcast / `Box<dyn Any>`.** Rejected — kills type safety at the FFI boundary; degrades into stringly-typed dispatch.
- **Vendor in app-specific modules as cargo features on a monolithic `nmp` crate.** Rejected — explosive feature matrix; entangles compilation; doesn't solve UniFFI typing.
- **String-keyed registry at runtime (open question 1's "type-erased" option).** Decided against in ADR-0010 in favor of generated app enum for compile-time safety. See that ADR for full reasoning.

## Open questions resolved here vs deferred

| Proposal open question | Resolution |
|---|---|
| 1. Generated app enum vs type-erased registry | Generated app enum — see ADR-0010 |
| 2. Declarative vs Rust migrations for domain-module schemas | Resolved in `kernel-substrate.md` §3 — Rust migrations with a small declarative index API |
| 3. UniFFI for app-level enums across crates | Resolved in `kernel-substrate.md` §5 — per-app `nmp-app-<name>` codegen crate owns the FFI exposure |
| 4. Protocol modules as crates or features | Separate crates (`nmp-nip01`, `nmp-nip17`, ...). Sharper boundaries; explicit dep graph; testable in isolation |
| 5. Minimum v1 extension API before social-client proof app | The 1a.1 substrate (5 trait families + codegen for one fixture) is the minimum. Twitter clone consumes it from 1a.2 onward |
