# ADR 0008: Twitter-clone on iOS as the Phase 1a demo target

**Date:** 2026-05-17
**Status:** accepted
**Supersedes (in part):** ADR-0006 (in choice of demo target only — discipline preserved)
**Relates to:** ADR-0005 (domain-keyed shadow), ADR-0007 (diagnostics bridge)

## Context

ADR-0006 chose a deliberately narrow demo target — kind:0 profile metadata on desktop iced — as the Phase 1a vertical slice. The point was risk reduction: prove the architecture works in code with the fewest variables, no FFI, one view kind, read-only.

The framework's value proposition is "build a complete cross-platform Nostr app with little platform code." A minimal avatar demo doesn't exercise that proposition. A real iOS app that anyone in the ecosystem recognizes — login, timeline, compose, profile, like — does.

The right way to manage the added scope is to keep ADR-0006's walking-skeleton discipline but make each step bigger: six sub-phases, each a walking skeleton in its own right, each adding exactly one architectural ingredient on top of working code.

## Decision

The Phase 1a demo target is **a simple Twitter-clone iOS app pulling from primal's relay**. Functionality at the end of 1a:

- Open the app (no login required) → see a real, lively timeline.
- Tap a post to see the thread.
- Tap an author to see their profile + recent posts.
- Log in with a private key (nsec or generated) to enable writes.
- Compose a new note.
- Like a post.
- Reply to a post.
- (Optionally) switch the timeline source from "seed discovery" to "your own follows" once logged in.
- Pulls from `wss://relay.primal.net` (single hardcoded relay; outbox routing comes in 1b).
- Persists across app restart (LMDB).

**Seed-driven timeline as the default read mode.** The unauthenticated timeline is not "this user's follows" (there is no user yet); it's the **union of follows of a small set of hardcoded seed dev accounts**. The app opens `Contacts` views for each seed, unions their follow lists into an `authors` set (typically several hundred to a few thousand pubkeys), and opens a `Timeline` view with that author set. This gives the demo real breadth from the first launch — fresh content from across the Nostr social graph, not a contrived test feed. It also stress-tests the framework on a realistic author-set size from sub-phase 1a.3 onward.

ADR-0006's discipline survives intact: walking skeleton, one ingredient per sub-phase, runtime evidence over modeled budgets, harness gates on every layer. ADR-0006's specific recommendation (desktop iced as the slice) is replaced by **iOS as the demo target, desktop iced as a non-shipping diagnostic reference target alongside.**

## Slice-blocker decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary demo platform | iOS (SwiftUI) | The platform that justifies the framework's existence; cross-platform mobile is the hard problem. |
| Reference platform | Desktop iced (rlib-linked, no FFI) | Maintained alongside iOS as a UniFFI-free diagnostic target — if something breaks on iOS but works on desktop, it's an FFI/Xcode issue, not architecture. |
| Relay | `wss://relay.primal.net` | Well-supported, NIP-77 capable, stable. |
| Profile slice test pubkey (1a.1–1a.2) | TBD — recommend the repo owner (pablof7z) or fiatjaf for a known-stable kind:0. | Stable kind:0 needed to exercise the placeholder→refined transition on a real account. |
| Seed dev accounts for timeline discovery (1a.3+) | TBD — suggested starter set: fiatjaf, jb55, pablof7z. ~3 seeds gives several hundred to a few thousand followed authors via union. | Their follow lists union into the demo timeline's author set. Real breadth, fresh content, no contrived feed, realistic author-set size for the planner. |
| FFI tool | UniFFI (per spec §3) | Already specified; this commits to it. |
| Demo app working name | TBD — needs choosing before 1a.0; suggestions: `nmpchirp`, `nmp-demo`, `chirp`. Framework name (`nmp` working) stays per `aim.md` §7.7. | The demo is the artifact people will see; needs an identity. |
| Storage backend | In-memory for 1a.1–1a.2; LMDB from 1a.3 | LMDB is real enough to expose persistence bugs; in-memory keeps the first two sub-phases focused on actor+FFI. |

## Sub-phase plan

Each sub-phase is itself a walking skeleton — running code at every checkpoint, not deferred integration. Each layers exactly one new architectural ingredient.

### 1a.0 — Skeleton (3–5 days)

- Cargo workspace per spec §4.1 crate roster.
- `nmp-core` actor scaffolding: single OS thread + flume channel + tokio runtime + empty event loop.
- Empty `AppState { rev }`, empty `AppAction`, empty `AppUpdate { FullState, ViewBatch, SideEffect }`.
- `nmp-testing` minimal with snapshot helpers.
- `justfile` recipes: `rust-build-host`, `test`, `fmt`.
- Nix flake.

**Exit gate.** `cargo test --workspace` passes; `cargo fmt --all -- --check` passes; the empty actor starts and stops cleanly.

### 1a.1 — Desktop Profile slice (~1 week)

The original ADR-0006 slice. Desktop iced binary; in-memory `EventStore` with kind:0 supersession + composite reverse index `(kind, author)`; minimal `Profile` view kind; manually-written `useProfile(pubkey)` wrapper for iced; one WebSocket via `nostr-sdk` to primal.

Includes ADR-0007 diagnostics minimum: one `RelayStatus` row, one `LogicalInterestStatus::Profile { pubkey }`, one `WireSubscriptionStatus` for the kind:0 REQ.

**Exit gate.** Avatar renders immediately with shortened-npub placeholder; updates in place to real picture/name when kind:0 arrives; mount/unmount 100 times rapidly shows correct refcount + grace-period behavior; firehose-bench `live` runs `cold_start` and a slice version of `profile_thrashing` against primal with measured (not modeled) numbers within budgets.

### 1a.2 — iOS port of the Profile slice (~2 weeks)

Same Profile-only feature, ported to iOS.

- `nmp-ffi` crate with `FfiApp` (`uniffi::Object`), `AppReconciler` callback interface, generated Swift bindings checked into `bindings/swift/`.
- iOS app shell: XcodeGen `project.yml`, SwiftUI entry point, `AppManager` (`@Observable`) implementing `AppReconciler`, `KeychainCapability` minimal (placeholder; full credential storage in 1a.4).
- xcframework build pipeline in `justfile`: `just ios-rust`, `just ios-gen-swift`, `just ios-xcframework`, `just run-ios`.
- iOS-specific `useProfile` wrapper using `@Observable` + property-wrapper pattern, refcounted per pubkey per ADR-0005.

**Exit gate.** iOS simulator launches; avatar component renders against primal exactly as on desktop; cross-platform consistency check (same scripted action sequence on desktop and iOS produces byte-identical `AppState` JSON at each checkpoint); UniFFI bindings regenerate deterministically.

### 1a.3 — LMDB + Contacts + seed-driven Timeline (~1.5 weeks)

Persistence, the first multi-kind view, and seed-driven discovery.

- Storage backend abstraction: `Box<dyn EventStore>` swap from in-memory to LMDB. LMDB schema design (key encoding, secondary indexes, kind:5 tombstones, watermarks placeholder for 1b).
- Contacts view kind (parsed kind:3 follow list) per `view-catalog.md` §9. **Multiple Contacts views open simultaneously — one per seed dev account.** Their follow lists union into the timeline's author set.
- Timeline view kind with fat `TimelineItem` payload per `view-catalog.md` §4. Spec is `{ kinds: [1, 6], authors: <union of seeds' follows>, limit: 200 }`. Subscribes to kind:1 / 6 filtered by that author union; pre-formats display fields via projections.
- iOS shell: home timeline screen (seed-driven by default), tap-to-profile navigation.
- Projection cache for `author_display`, `author_picture`, `author_nip05` per `reactivity.md` §6.
- Seed-set bootstrap action: on app launch, open `Contacts` views for each seed pubkey; once the contact lists land (or after a short timeout), open the `Timeline` view with the unioned author set.

**Exit gate.** Cold launch with primed LMDB renders the seed-driven timeline in ≤ 1.5s, showing fresh content from hundreds of authors followed by the seeds; tap an author → profile screen → back works; kind:0 arriving mid-scroll updates all author rows in place per doctrine D1; updating one seed's kind:3 mid-session re-resolves the timeline's author set without manual intervention; reactivity-bench `--standard` continues to pass at the larger author-set size; firehose-bench `live` for `sustained_firehose` (running at the real seed-author scale, not modeled) lands within budgets.

### 1a.4 — Login + Signer + Compose (~1.5 weeks)

The first write path.

- Signer trait shape with local-key implementation (raw nsec).
- `ActionCx` with event_store, signer, publisher, active_account fields.
- `AppAction::AddAccountPrivateKey { nsec_or_ncryptsec, passphrase }` action.
- `AppAction::ActivateAccount { pubkey }` action.
- `KeychainCapability` real implementation: encrypted nsec storage via iOS Keychain (app-private access group for now; shared group for NSE in Phase 5).
- `AppAction::SendNote { content, reply_to, mentions }` action with full atomicity (publish to primal + insert locally as one actor message; rollback on either side failing).
- iOS shell: login screen (nsec input, "generate new key" button); compose sheet; "send" button.
- **Timeline source switch.** Logged-in users get an optional "Home" mode that uses the active account's own kind:3 follow list as the timeline's author set, replacing the seed-driven set. UI toggle between "Discover" (seed-driven) and "Home" (your follows). Unauthenticated users only see "Discover."

**Exit gate.** Open app unauthenticated → see seed-driven timeline. Log in with existing nsec → "Home" mode available; switching to it re-resolves the timeline to your own follows; compose a note → publish appears on primal (verified externally) → also appears in your own timeline immediately; action atomicity test passing; switching back to "Discover" restores the seed-driven view without re-fetch (view warmth absorbs it).

### 1a.5 — Reactions + Thread + Reply (~1 week)

The interaction loop.

- Reactions view kind per `view-catalog.md` §6 with NIP-25 emoji normalization.
- Thread view kind per `view-catalog.md` §5 with reply-marker handling and orphan support.
- `AppAction::React { target, emoji }` action.
- `AppAction::SendNote` extended to handle `reply_to: Option<EventCoord>` (reply marker per NIP-10).
- iOS shell: like button on each timeline item; tap-to-thread navigation; thread screen with nested replies; reply composer.

**Exit gate.** Like a note → reaction count increments locally + on primal; tap a note → see thread tree → tap a reply to see its own thread; reply to a note → it appears as a child of the original in the thread view; thread orphan-handling correctness verified by injecting out-of-order replies.

### 1a.6 — Profile screen + polish (~1 week)

Cross-view composition and the demo polish.

- Profile screen: composes `Profile` view + author-filtered `Timeline` view into one screen.
- Pull-to-refresh: forces planner re-fetch on the active timeline.
- Pagination on scroll: cursor-advancing actions per `view-catalog.md` §4.
- Error states: relay disconnected toast; signing failure toast.
- Diagnostics screen: opens `ViewSpec::NetworkDiagnostics` per ADR-0007; shows RelayStatus + active LogicalInterestStatus + WireSubscriptionStatus.
- App icon, launch screen, minimal visual polish.

**Exit gate.** The full demo flow works as a real iOS app: log in → see timeline → tap profile → see notes → tap a note → see thread → reply → like → compose → see your own post. Persists across app restart. Works against primal under realistic load (use firehose-bench `live` to confirm budgets hold).

## What still needs design before each sub-phase

| Sub-phase | Design gap |
|---|---|
| 1a.0 | None — workspace setup is well-specified. |
| 1a.1 | None — Profile is fully specified in `view-catalog.md` §3. Pick the test pubkey + app name. |
| 1a.2 | UniFFI workflow concrete recipe; iOS `AppManager` reference pattern (50 lines of Swift). Both mostly mechanical. Optional: `keychain-rs` integration plan. |
| 1a.3 | **LMDB schema design doc.** Real design work — key encoding, indexes, tombstones. Pre-1a.3 deliverable. |
| 1a.4 | **Signer trait shape + ActionCx fields** (currently sketched). **Login UX flow** (single screen vs onboarding wizard). **KeychainCapability iOS access groups** (app-private for now, named group placeholder for NSE). Pre-1a.4 deliverable. |
| 1a.5 | NIP-10 reply-marker handling tests; React action subtleties (debounce double-tap, optimistic UI). Smaller; design in-phase. |
| 1a.6 | Pagination cursor semantics under primal's real EOSE behavior; diagnostics screen layout (read-only renderer over ADR-0007 records). |

## Total effort

Approximately **8 weeks for one experienced developer**, less with prior Rust + iOS experience, more if UniFFI / Xcode toolchain bite hard (they will at least once).

Each sub-phase has running code at its exit gate. There is no point at which the project is "partially built but not runnable." A reviewer can pull at any sub-phase boundary, run `just run-ios`, and see a real app.

## Desktop reference target discipline

The desktop iced binary built in 1a.1 stays alive throughout 1a.2 through 1a.6. It links the rlib directly (no UniFFI). It is not a shipping product, but it acts as a non-FFI reference:

- After each sub-phase, the desktop binary supports the same feature.
- When a behavior is right on desktop but wrong on iOS → UniFFI / SwiftUI / build issue.
- When a behavior is wrong on both → actor / store / planner issue.

This collapses "is it the architecture or is it the toolchain?" debugging. Cost: ~20% of UI work since the iced shell needs to grow alongside SwiftUI. Benefit: catastrophic debugging shortcut.

## Consequences

- **The first runtime evidence is a real iOS app**, not a desktop avatar. Demoable to the Nostr community.
- **Phase 1a takes ~8 weeks** instead of ~2. The trade is a fundamentally more compelling result.
- **Phase 1b becomes "expand to multi-relay + outbox + full view kinds + other platforms"** rather than "expand to anything beyond Profile." Some Phase 2 (sync engine, negentropy) and Phase 3 (multi-account, more signer kinds) work also reasonably moves earlier.
- **iOS toolchain risk is front-loaded.** UniFFI surprises, xcframework build, Xcode versioning — all happen in 1a.2. If they don't resolve cleanly, we know early and the desktop reference target is the fallback validation path.
- **Diagnostics from ADR-0007 are first-class** in 1a.1 onward, not bolted on later.
- **No phase ends with a non-runnable system.** Every checkpoint produces a real binary.

## Alternatives considered

- **Keep ADR-0006 as-is (desktop avatar only).** Rejected — the demo doesn't show the framework's actual value proposition.
- **Skip the desktop reference target.** Rejected — loses the UniFFI-vs-architecture debugging shortcut.
- **Start with iOS directly (skip 1a.1 desktop slice).** Rejected — UniFFI noise in the very first runtime would conflate architecture bugs with toolchain bugs.
- **Build a Mastodon clone instead.** Considered — Mastodon's social graph + threads are similar but the protocol is wrong target. Nostr-shaped data + Nostr-shaped relay behavior is what we're proving.
- **Build a DM-first messenger (Pika-style).** Rejected for this slice — NIP-17 + gift-wrapped + NSE adds significant Phase 5 work to the critical path. Better to demo public-event shapes first; messenger pattern follows naturally.

## Validation

- Manual demo at each sub-phase exit.
- Cross-platform consistency between desktop reference and iOS at each sub-phase.
- `reactivity-bench --standard --fail-on-gate` continues to pass throughout.
- `firehose-bench live` runs more scenarios with measured numbers as the adapters expand (1a.1: cold_start + profile_thrashing slice; 1a.3: + sustained_firehose; 1a.4: + multi_account stub; 1a.6: most of the scenario set).
- An external developer pulls at 1a.6 and writes a new screen (e.g., a hashtag timeline) in ≤ 1 hour using only the docs — the LLM-friendliness test from `product-spec.md` §3.4.
