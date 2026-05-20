# TEA / Rust Multiplatform Guideline Review

> Date: 2026-05-20.
> Scope: review external TEA/Rust-multiplatform lessons against NMP's RMP doctrine and decide what belongs in repo guidance.

## Verdict

The pasted recommendations are mostly directionally sound, but need translation for NMP.

| Recommendation | Verdict for NMP |
|---|---|
| Co-locate Model / Update / View around a central type or page | Adopt. Do not split the repo into global role folders. |
| Accept 2000+ LOC cohesive files | Reject. Elm tolerates this better than agent-driven Rust repos; NMP's 300/500 LOC rule stays binding. |
| Flat state until a sub-domain is genuinely large | Adopt. Keep actor/router flat; split only self-contained modules. |
| Iced-style message mapping between screens | Adopt as the Rust/desktop and generated-wrapper mental model. |
| Capability bridges report facts, never policy | Already doctrine D7; strengthen it in `AGENTS.md`. |
| Time-travel / replay from action history | Adopt as a design direction, but do not add a framework-magic C-bullet without an ADR and test. D9 already carries the deterministic-clock seam. |
| Full snapshots first; granular updates only after profiling | Already doctrine; restate in `AGENTS.md` because agents keep reaching for deltas too early. |

## Source-backed lessons

### Elm

The official Elm guide recommends modules built around a central type, with page modules containing `Model`, `init`, `update`, `view`, and helpers. It explicitly warns against splitting into separate `Model`, `Update`, and `View` modules. Source: <https://guide.elm-lang.org/webapps/structure.html>.

The Elm modules guide and FAQ repeat the same principle: organize around coherent domain data structures, not abstract technical roles. Sources: <https://guide.elm-lang.org/webapps/modules>, <https://faq.elm-community.org/>.

Richard Feldman's public training material gives the pragmatic scaling rule: if `view`, `Model`, or `update` gets painfully large, subdivide that part into smaller helpers without changing the overall architecture. Source: <https://frontendmasters.com/assets/resources/richardfeldman/elm-slides-day2.pdf>.

### Iced

Iced is the closest mainstream Rust TEA implementation. Its docs show `update`, `view`, and `Message` as the core triplet, declarative `Subscription`s, and screen composition through nested messages plus `Task::map`, `Element::map`, and `Subscription::map`. Source: <https://docs.rs/iced/latest/iced/>.

Iced also documents purity for replay/time-travel: timed applications receive an `Instant` in update logic so identical message history can produce identical state. Source: <https://docs.iced.rs/src/iced/application/timed.rs.html>.

### Crux

Crux is the closest architectural analogue to NMP's multiplatform goal. It splits a Rust core from native shells, keeps behavior in Rust, uses message-based boundaries, represents side effects as data requested by the core and executed by the shell, and generates typed interfaces for Swift/Kotlin/TypeScript. Source: <https://redbadger.github.io/crux/latest_master/>.

This validates NMP's stricter D7 position: native capability code is a shell adapter, not a policy layer.

### Native UI ecosystems

Android Compose's official architecture docs frame UI as immutable state plus events: state flows down, events flow up, and the UI should not mutate state outside event handlers. Source: <https://developer.android.com/develop/ui/compose/architecture>.

Swift's Composable Architecture converges on the same primitives: state, action, reducer, effects, store, composition, and deterministic tests with controlled dependencies. Source: <https://github.com/pointfreeco/swift-composable-architecture>.

For NMP, these sources support using native SwiftUI/Compose only as renderers and event emitters. They do not justify native business logic.

### Dioxus / Dioxus-TEA

`dioxus-tea` proves TEA can be layered into Dioxus, but Dioxus itself is primarily a component/signal framework. It is useful prior art, not the model NMP should optimize around. Sources: <https://docs.rs/dioxus-tea>, <https://docs.rs/dioxus/latest/>.

### Production Elm

Rakuten's roughly 100k-line Elm deployment supports the claim that pure functional UI plus types can scale, but it also highlights ecosystem costs and the need to build missing libraries. Source: <https://www.infoq.com/news/2021/10/elm-lessons-learnt-production/>.

For NMP, the response is not to make `nmp-core` huge. It is to keep extension modules, generated typed app crates, and native capability adapters as deliberate escape hatches.

## Adopted guidance

1. Co-locate TEA units by owner: feature, view module, protocol module, app module, or central domain type.
2. Respect the local LOC ceiling even when Elm sources tolerate long cohesive files.
3. Keep native shells dumb: render snapshots, emit user events, execute capabilities.
4. Preserve replayability: nondeterminism enters through explicit messages, capability results, or injected seams.
5. Keep full snapshots as the correctness baseline; deltas need profiling evidence and lossless catch-up semantics.
