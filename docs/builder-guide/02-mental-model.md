# 02 — Mental model: kernel + 5 trait families

*Status: SHIPS · Audience: both · Read after [01](01-what-nmp-is.md).*

If you remember one thing: **NMP is a Nostr-native app kernel with first-class
extension modules — not a framework with closed built-ins.** The kernel knows
*how* to run a reactive Nostr client. It does not know *what* a Profile, an
Episode, a Highlight, or a TODO is. Those nouns live in modules you write.

This section gives you the four-layer stack, the five trait families in one
paragraph each, the no-app-nouns-in-kernel rule, what crosses FFI, and a
concrete "where does X live?" map. It is the map for the whole guide.

## The 4-layer stack

ADR-0009 (`docs/decisions/0009-app-extension-kernel-boundary.md:44-62`) fixes
four layers with strict ownership. Built from the bottom up:

```
┌──────────────────────────────────────────────────────────────────────┐
│ PLATFORM SHELL          ios/NmpStress (SwiftUI, 1,375 LOC, live)       │
│                         ios/NmpPodcast · ios/NmpHighlighter (Step 0)   │
│  owns: rendering, OS handle execution, generated wrappers              │
│  D5 ► consumes ONE bounded JSON snapshot; no policy nouns              │
└────────────────────────────────▲───────────────────────────────────────┘
                                  │ raw C JSON-over-string (UniFFI = M14)
┌────────────────────────────────┴───────────────────────────────────────┐
│ GENERATED FFI CRATE     nmp-codegen output (per-app `nmp-app-<name>`)   │
│  owns: concrete AppAction / AppUpdate / ViewSpec enums + wrappers       │
│  D6 ► no Result<T,E> crosses here; envelopes only                      │
└────────────────────────────────▲───────────────────────────────────────┘
                                  │ ModuleRegistry composition
        ┌─────────────────────────┼──────────────────────────┐
┌───────┴──────────┐  ┌───────────┴───────────┐  ┌────────────┴─────────┐
│ APP CORE CRATES   │  │ NMP PROTOCOL MODULES   │  │  (more app cores)    │
│ apps/podcast/      │  │ nmp-nip29 (groups)     │  │ fixture-todo-core    │
│  podcast-core      │  │ nmp-nip42 (auth)       │  │  (non-Nostr proof)   │
│  podcast-audio …   │  │ nmp-nip77 (sync)       │  │                      │
│ D0 ► MAY hold app  │  │ nmp-signers (identity) │  │ D0 ► app nouns OK    │
│      nouns         │  │ D0 ► protocol nouns ONLY│  │                     │
└───────┬──────────┘  └───────────┬───────────┘  └────────────┬─────────┘
        └─────────────────────────┼──────────────────────────┘
┌────────────────────────────────┴───────────────────────────────────────┐
│ nmp-core KERNEL    actor · EventStore · planner · subs · publish        │
│                    + 5 substrate trait families + codegen + diagnostics │
│  D0 ► ZERO app nouns. ZERO protocol nouns. Generic infrastructure only. │
│  D4 ► one writer per fact (the actor) — never the platform              │
└──────────────────────────────────────────────────────────────────────────┘
```

The six real shipped crates are labelled in their layer above:
`nmp-core` (kernel), `nmp-nip29` / `nmp-nip42` / `nmp-nip77` / `nmp-signers`
(protocol modules), `apps/podcast/podcast-core` + `fixture-todo-core` (app
cores). `nmp-codegen` produces the generated FFI crate; `ios/NmpStress` is the
live shell.

### Doctrine callouts on the diagram

- **D0 (kernel/extension boundary).** The dividing line *is* this section.
  `nmp-core` provides generic infrastructure only — actor runtime, verified
  event store, planner, publish pipeline, signer plumbing, the five trait
  registries (`0009-app-extension-kernel-boundary.md:22-62`). It contains
  **no** `Profile`/`Timeline`/`Episode`/`Highlight`/`Project` types. The rule:
  *if shipping your app requires adding a domain noun to `nmp-core`, the
  boundary is wrong and the kernel changes — never the app.*
- **D4 (single writer per fact).** Exactly one component owns each fact. The
  actor inside the kernel is that writer. The platform shell never mutates
  state; it renders snapshots and dispatches actions.
- **D5 (snapshots bounded by what's open).** What crosses up to the shell is
  one bounded JSON snapshot scoped to currently-open views — not the whole
  store. The shell holds no source-of-truth state.

## The 5 trait families in one paragraph each

`nmp-core` defines five extension trait families
(`crates/nmp-core/src/substrate/mod.rs:1-79`). An extension crate implements
one or more; the kernel runtime knows only that a module conforms to a trait
and contributes to the generated per-app enums.

- **`DomainModule`** — durable records that are *not* Nostr events: drafts,
  settings, transcripts, weight logs. The kernel owns storage, migrations,
  indexes; the module owns record meaning. Empty `ingest_kinds()` = pure
  app-local store; protocol crates override it to claim Nostr kinds.
- **`ViewModule`** — typed reactive projections. You declare a `Spec`,
  `Payload`, `Delta`, `Key`, `State` and the dependency keys you care about;
  the kernel feeds you `on_event_*` callbacks and you emit deltas. This is the
  only sanctioned way state reaches the UI.
- **`ActionModule`** — durable workflows on the action ledger. A `start` →
  validated `ActionPlan`, then a `reduce` step machine driven by capability
  results, relay acks, timeouts. Survives restarts.
- **`CapabilityModule`** — typed native fact reports. The module declares a
  request/result pair and a callback-interface name; native code *reports
  facts*, it never decides policy (D7).
- **`IdentityModule`** — signer scopes beyond "the active Nostr account":
  `HumanAccount`, `AppLocal`, `ExternalSigner`, `Ephemeral`. Identity lives in
  `nmp-signers`, never `nmp-core`.

Full signatures, lifecycles, and a "which trait?" decision tree are in
[05 — Kernel substrate](05-substrate-traits.md).

## The no-app-nouns-in-kernel rule

This is D0 restated operationally. Before adding a type to `nmp-core`, ask:
*is this generic Nostr-client infrastructure, or is it a noun some specific
app cares about?* `VerifiedEvent`, `CompiledPlan`, `InsertOutcome` are
infrastructure. `Episode`, `Highlight`, `Project`, `Group` are nouns —
protocol nouns go in `nmp-nip*` crates, app nouns in app-core crates. The live
proof that the boundary holds in both directions: `fixture-todo-core` exercises
all five families with zero Nostr concepts, and `nmp-nip29` adds 13 domains /
7 views / 15 actions of group machinery while `nmp-core` gains exactly *one*
generic seam (the relay-pin routing lane) and zero group nouns.

## What crosses FFI (and what does not)

| Crosses FFI | Stays in Rust |
|---|---|
| One JSON snapshot per emit (D5) | The EventStore + every `VerifiedEvent` |
| Dispatched `AppAction` variants | Action ledger, step machines |
| `CapabilityRequest` / `CapabilityEnvelope` | Planner, subscription pool, signer keys |
| `rev: u64` monotonic guard | All policy / retry / routing decisions |

No `Result<T,E>` crosses the boundary (D6) — failures arrive as data inside
the snapshot or as capability envelopes. Today the wire is raw C
JSON-over-string (`crates/nmp-core/src/ffi.rs`); the UniFFI migration is M14
(see [27 — Discrepancies](27-discrepancies.md)).

## "Where does X live?" — concrete map

| Noun | Lives in | Why |
|---|---|---|
| `VerifiedEvent`, `CompiledPlan` | `nmp-core` | generic Nostr infra |
| `Signer`, `IdentityScopeKind` | `nmp-signers` | identity is a protocol module (D0) |
| NIP-29 `GroupId`, group views | `nmp-nip29` | protocol noun (`crates/nmp-nip29/src/lib.rs:11-19`) |
| NIP-77 sync reconciler | `nmp-nip77` | protocol noun |
| Podcast `Episode`, feed records | `apps/podcast/podcast-core` | app noun (`apps/podcast/podcast-core/src/lib.rs:1-2`) |
| `TodoRecord` | `fixture-todo-core` | app noun (non-Nostr proof) |
| SwiftUI list cell, OS audio handle | `ios/NmpStress` / shell | rendering / OS execution |

The single test of correctness: a hypothetical Highlighter module can be added
with **zero changes to `nmp-core`** (ADR-0009 acceptance criterion 3).

## Anti-patterns

1. **Putting `Highlight` / `Episode` / `Project` in `nmp-core`.** This is the
   exact abstraction error ADR-0009 exists to forbid — it turns the kernel
   into "a junk drawer of every consumer's domain concepts." App nouns go in
   app-core crates; protocol nouns in `nmp-nip*` crates.
2. **Conflating a `ViewModule` with platform UI components.** A `ViewModule`
   is a Rust reactive projection that emits a typed payload. It is not a
   SwiftUI `View`, not a Compose `@Composable`. The shell renders the
   payload; it does not contain the projection logic.
3. **Bypassing `ViewModule` to render raw events in SwiftUI.** Decoding
   `kind:1` JSON in Swift re-implements the kernel's reactive contract in the
   shell, duplicates state ownership (D4 violation), and breaks D5 bounding.
   Every read goes through a `ViewModule` snapshot.
4. **Adding a 6th trait family without an ADR.** The five families are a
   closed architectural contract adopted by ADR-0009 + `kernel-substrate.md`.
   A new family is a kernel change that requires its own ADR, not an ad-hoc
   trait dropped into `substrate/`.

## Deliverables

- **ASCII stack diagram** (above) with the six shipped crates labelled in
  their layer and D0/D4/D5 callouts.
- **"Where does X live?" map** (above) — paste it next to any PR that adds a
  new type and answer the column "why" before merging.

See also: [03 — Doctrine D0–D8 end-to-end](03-doctrine-d0-d8.md) ·
[05 — Kernel substrate — the 5 trait families](05-substrate-traits.md) ·
[15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
