# ADR-0030 — UniFFI vs C-ABI: the two-surface binding decision

- **Status:** Accepted
- **Date:** 2026-05-23
- **Revisits:** `docs/aim.md` §5 (UniFFI named as the binding strategy at
  lines 60, 171, 185, 203–204, 242) and `docs/plan/m14-uniffi.md:1-24` (the
  open migration milestone).
- **Related:** ADR-0009 (app-extension kernel boundary — snapshots cross FFI
  as JSON), ADR-0010 (generated app enum vs type-erased registry — the home
  of `nmp-codegen`), ADR-0025 (Marmot bespoke FFI cluster — named exception),
  ADR-0027 (unified `ActionModule` trait — established the "Rust-only seam,
  defer UniFFI" precedent for action registration).

## Context

`docs/aim.md` names **UniFFI** as the framework's
binding strategy. The aim doc says so in five places:

- §2 (line 60) — the reference layout in `docs/aim.md` includes a `uniffi-bindgen/`
  binding generator crate.
- §4.14 (line 171) — `<framework> init` scaffolds the per-platform shells
  on top of UniFFI bindings.
- §5 (lines 185, 203–204) — the workspace shape lists
  `<framework>-ffi: UniFFI scaffolding. FfiApp object, AppReconciler callback
  interface, uniffi::Record/Enum derives on state types`, and the layout
  carries `bindings/swift/`, `bindings/kotlin/`, `bindings/typescript/`
  directories for *generated UniFFI* bindings checked in.
- §7 (line 242) — Open design question 3 names UniFFI as the substrate the
  reactive cross-FFI subscription protocol must adapt.

`docs/plan/m14-uniffi.md` keeps UniFFI on the milestone roadmap (M14, Arc 3)
with an exit gate "iOS app builds and runs against UniFFI-generated
bindings; no raw C FFI in the app target."

The current code says something different. NMP ships a hand-rolled C-ABI:

- **Write/register surface.** `crates/nmp-core/src/ffi/` contains **~46
  `#[no_mangle] pub extern "C"`** symbols (workspace-wide tally: 56 once
  `nmp-app-chirp` host shims and the Marmot cluster from ADR-0025 are
  counted). Examples: `nmp_app_new`, `nmp_app_sign_in_nsec`
  (`crates/nmp-core/src/ffi/identity.rs:21,35,46,92,103,125,141,152`),
  `nmp_app_wallet_pay_invoice` (`crates/nmp-core/src/ffi/wallet.rs:29,45,84`),
  `nmp_app_set_update_callback` (`crates/nmp-core/src/ffi/mod.rs:1251`).
- **Read/snapshot surface.** `ios/Chirp/Chirp/Bridge/KernelBridge.swift`
  carries 42 top-level `Decodable` struct mirrors of kernel projection types
  between lines 680 and 1988 — **~1,308 LoC of hand-maintained Swift** that
  duplicates the shape of every JSON projection emitted by
  `kernel.make_update()` (`KernelUpdate`, `SnapshotProjections`,
  `GroupChatSnapshot`, `DmInboxSnapshot`, `ZapsAggregateSnapshot`,
  `BunkerHandshake`, `TimelineItem`, `KernelMetrics`, …). Each new projection
  field requires a coordinated Rust+Swift edit; a typo or rename silently
  drops the field via `keyNotFound` and the host renders a stale view.
- **What codegen does today.** `crates/nmp-codegen/src/generate.rs:19-29`
  emits eight Rust files per app (`Cargo.toml`, `lib.rs`, `action.rs`,
  `update.rs`, `envelope.rs`, `view_spec.rs`, `capability.rs`, `domain.rs`,
  `ffi.rs`) and **zero Swift, Kotlin, or TypeScript files**. There is no
  Swift/Kotlin/TS emitter inside `nmp-codegen/src/` — only Rust scaffolding.

The gap between the aim doc and the code has stood for the entire project's
lifetime. No ADR has named the gap or made a decision about it. This ADR
closes that.

## The two-surface insight

The prior framing — "C-ABI vs UniFFI, pick one" — is a false dichotomy. The
binding layer is not one surface; it is **two**, and they have different
problems with different solutions.

### Surface 1 — Write/register (the FFI verbs)

The ~46 `#[no_mangle]` symbols are how the host **calls into** the kernel:
constructors (`nmp_app_new`), setters (`nmp_app_set_update_callback`),
identity operations (`nmp_app_sign_in_nsec`), and the generic dispatch
entrypoint (`nmp_app_dispatch_action`). The Swift side wraps them in a
typed Bridge object that hides the raw pointers, length-prefixed strings,
and `*const c_void` callback contexts.

**This is UniFFI's actual sweet spot.** UniFFI was designed to eliminate
exactly this code: it generates the Rust `#[no_mangle]` shim *and* the
Swift/Kotlin wrapper from one `uniffi::Record` / `uniffi::Object`
declaration. The work that lives in `KernelBridge`'s pointer-wrangling code
disappears.

### Surface 2 — Read/snapshot (the Decodable mirrors)

The ~1,308 LoC of Swift `Decodable` structs are the host's parser for the
JSON snapshot emitted by `kernel.make_update()`. ADR-0009 fixed snapshot
shape as **JSON across the FFI boundary** — that is a deliberate
architectural choice (host-extensibility via the `projections` map, schema
evolution by adding optional fields, no UniFFI codegen on the snapshot
critical path because snapshots are emitted at ≤60 Hz and serialized once).

**UniFFI does not help this surface.** Even after a full UniFFI migration,
the snapshot would still be JSON (per ADR-0009), and the host would still
need a `Decodable` type for every projection. UniFFI's `uniffi::Record`
would generate the *record shape* — but the projection record shape is not
the bottleneck. The bottleneck is that *the records have no generator at
all today*: every field is typed by hand in `KernelBridge.swift`, every
rename is a coordinated cross-language commit, and every drift goes silent
via `keyNotFound`.

The read-surface problem is **not** "we chose C-ABI." The read-surface
problem is **"`nmp-codegen` has no Swift emitter."**

### Why this matters for the decision

Conflating the two surfaces would say "we are behind on UniFFI; M14 will
fix it." That sentence is half-true (the write surface) and half-false (the
read surface — M14 would not touch it, because UniFFI does not touch JSON
snapshot parsing). Treating them separately surfaces the actually-blocking
debt (the codegen omission) and lets us defer the actually-deferrable work
(the write-surface migration) without conflating the two timelines.

This is the same shape ADR-0027 reached for the unified `ActionModule`
trait: registration is "Rust-only — UniFFI deferred until there is a stable
serialization for `ActorCommand`." The write surface here inherits that
posture; the read surface diverges because JSON *already is* a stable
serialization.

## Decision

### (a) Write/register surface — keep C-ABI; defer UniFFI to M14

The ~46 `#[no_mangle] pub extern "C"` symbols in
`crates/nmp-core/src/ffi/` remain as-is. The UniFFI migration named in
`docs/plan/m14-uniffi.md` is **not abandoned** — it is the planned end-state
— but it is **not the bottleneck** today.

Rationale:

1. **The FFI verb surface is fluid.** Direction reviews #34 (and the
   subsequent ABI-freeze proposal) flagged that C-ABI churn is real, but the
   churn is mostly *consolidation under `dispatch_action`*, not new
   incremental verbs. Migrating to UniFFI now would freeze the surface at
   its current shape and then immediately need to be unfrozen as
   `dispatch_action` absorbs more verbs (reviews #31, #67, #69 on the
   bypass-to-dispatch ratio).
2. **Single composition root.** Per ADR-0025 and ADR-0010, NMP ships one
   binary with one app crate; there is no out-of-tree C consumer that
   benefits from a stable extern "C" surface. The cost of "hand-rolled
   C-ABI" is paid once, in `KernelBridge`'s pointer-wrangling code — not by
   downstream consumers.
3. **UniFFI carries a build-system cost.** UniFFI requires its own bindgen
   pipeline, xcframework integration, and generated-bindings-as-checked-in-code
   discipline. M14's exit gate names these explicitly. Paying that cost while
   the write surface is still consolidating is poor sequencing.
   (`nmp gen modules --check` was deleted by ADR-0046; the CI gate is now
   `just gen bindings` + diff check.)
4. **ADR-0027 precedent.** The unified `ActionModule` trait already decided
   the dispatch seam is "Rust-only; no useful C-ABI shape" because
   `Self::Action` and `ActorCommand` are Rust types with no stable C
   representation. The write surface inherits that posture: today's
   `nmp_app_dispatch_action(action_json: *const c_char)` is the C bridge to
   the Rust-only seam, and that's fine.

What is *not* deferred:

- New write-surface FFI symbols must be justified against the same bar
  reviews #67 / #69 set: a new `nmp_app_*` C-ABI symbol must either
  (a) route through `dispatch_action` or (b) carry a one-line note in the
  PR description naming why it cannot.
- The Marmot bespoke cluster (ADR-0025) remains the named, bounded
  exception. No second cluster.

### (b) Read/snapshot surface — generate the host's typed decoders in `nmp-codegen`

The hand-mirrored read-surface types are replaced by Swift code emitted
from `nmp-codegen` against the projection schema, so a renamed/added field
regenerates the host parser instead of being mirrored by hand.

> **Realization note (current).** This decision shipped, but the *input
> contract* it decodes changed under it. The original sketch (a Swift JSON
> `Decodable` emitter writing to a checked-in `bindings/swift/` path) was
> overtaken by ADR-0037/ADR-0044: the read surface is now a **typed
> FlatBuffer sidecar** (`SnapshotFrame.typed_projections`), not a JSON
> `payload:Value` tree. So the emitter that actually shipped is
> `crates/nmp-codegen/src/swift_typed_decoders.rs` — it generates the
> mechanical part of every typed-FlatBuffer-sidecar decoder (envelope
> lookup by `key`+`schema_id`, `getRoot` into the `flatc --swift` reader
> struct), with a per-key `TypedProjectionGlue` static mapping the reader
> to the Chirp domain type. No `bindings/swift/` directory was ever
> created; the generated decoders live in the codegen output tree and the
> Chirp host imports them. The hand-mirror retirement happened along the
> typed-decoder path rather than as a single deletion of a fixed
> `KernelBridge.swift` line range.

The emitter lives alongside `crates/nmp-codegen/src/generate.rs` (per
ADR-0010, this is the codegen seam). It is **not** UniFFI — it is
`nmp-codegen` doing the read-surface decoder job, while the *write* surface
waits for M14 to do the UniFFI-proper job.

## Consequences

### Positive

- **The aim-doc gap is closed by a named decision, not by silence.** Future
  reviews stop relitigating "why don't we use UniFFI?" — the answer is "we
  will, on the write surface, at M14; the read surface is a different
  problem with a different (smaller, sooner) fix."
- **The hand-mirror drift hazard becomes a build-time error.** A renamed
  field in a Rust projection regenerates the Swift decoder; a mismatched
  Swift consumer fails to compile rather than silently dropping the field.
- **`nmp-codegen` earns its keep.** Today it emits 8 Rust files and zero
  Swift. After (b), it emits the read-surface contract for every host
  platform — the same emitter scaffolding extends naturally to Kotlin
  `@Serializable` and TypeScript interfaces (the `bindings/kotlin/` and
  `bindings/typescript/` slots that aim.md §5 lines 203–204 reserved).
- **M14 stays clean.** When the write-surface UniFFI migration lands, it
  no longer carries the responsibility for "also fix the read surface" —
  the read surface is already typed by codegen, and UniFFI plugs into the
  write surface alone. Smaller M14 = sooner M14.
- **Doctrine alignment.** Aim.md §6 doctrine #4 ("Replaceable-event
  invariants enforced on insert") and the broader "make broken
  applications structurally impossible" thesis extends to the binding
  layer: a field-rename silently dropping a projection is exactly the
  class of bug doctrine #4 forbids at the protocol layer; the codegen
  emitter forbids it at the binding layer.

### Negative

- **The write surface remains hand-rolled.** Adding a new FFI verb means
  writing the `#[no_mangle]` shim, the C header entry, and the Swift
  Bridge wrapper by hand. The cost is paid until M14. Mitigation: the
  per-PR bar above (route through `dispatch_action` by default).
- **Two codegen targets to maintain.** The Swift emitter introduced by (b)
  is a parallel emitter to the Rust scaffolding emitter that already
  exists in `nmp-codegen/src/generate.rs`. Both target the same manifest
  and should share manifest-parsing code, but the emission templates are
  distinct. This is structural complexity in `nmp-codegen` that we accept.
- **A generated read-surface decoder set is new.** It costs a CI gate
  (`just gen bindings` + diff check against the projection schema) and a
  small amount of repo discipline. The cost is paid once. (No
  `bindings/swift/` directory was created; see the realization note above.)
- **Codegen output must be deterministic.** The M14 exit gate already
  names this for the Rust scaffolding ("repeated runs produce byte-identical
  output"); the read-surface emitter inherits the same constraint. A
  non-deterministic emitter would make the CI gate flake.

### Compatibility

- No FFI ABI change. The write-surface symbols are unchanged.
- No snapshot schema change. ADR-0009's JSON-snapshot decision is the
  load-bearing prerequisite for this ADR (the snapshot shape is *already*
  the codegen contract; the emitter just renders the Swift parser for it).
- The hand-mirrored read-surface types are retired as the generated typed
  decoders land; the write-surface pointer-wrangling in `KernelBridge.swift`
  is untouched.

## Out of scope

- **Kotlin and TypeScript emitters.** Aim.md §5 lines 203–204 reserve
  `bindings/kotlin/` and `bindings/typescript/` directories for the same
  pattern. They are obvious follow-ons once the Swift emitter pattern is
  validated. This ADR does not commit to a sequence; the Swift emitter is
  the immediate priority because the iOS host is the only platform shell
  in flight (ADR-0009, ADR-0010).
- **WASM `nmp-wasm` parity.** Direction review #74 named `nmp-wasm` as
  having zero `nmp-core` dependency; that is a separate structural bug
  not addressed here. When `nmp-wasm` re-anchors on `nmp-core`, it will
  decode the same JSON snapshot — and the TypeScript emitter from the
  follow-on above is the natural read-side target.
- **UniFFI's `uniffi::Record` for snapshots.** A future ADR could choose
  to express the snapshot record types in `uniffi::Record` rather than a
  bespoke codegen. The decision here is intentionally narrower: ship the
  Swift emitter against the existing JSON snapshot, do not block on the
  UniFFI migration. If M14 chooses to swap the emitter's *input* from
  "projection manifest" to "`uniffi::Record` declarations," the *output*
  (Swift `Decodable` structs) is unchanged from the host's POV.
- **The Marmot bespoke FFI cluster.** ADR-0025 is the named exception;
  this ADR does not modify it.

## References

- `docs/aim.md` §2 (line 60), §4.14 (line 171), §5 (lines 185, 203–204),
  §7 (line 242) — UniFFI as the named binding strategy.
- `docs/plan/m14-uniffi.md:1-24` — UniFFI migration milestone.
- ADR-0009 — snapshot-as-JSON (the load-bearing prerequisite for the
  read-surface codegen approach).
- ADR-0010 — generated app enum vs type-erased registry (the home of
  `nmp-codegen`).
- ADR-0025 — Marmot bespoke FFI cluster (named exception, unchanged).
- ADR-0027 — unified `ActionModule` trait (precedent: Rust-only seam,
  UniFFI deferred because action types have no stable C representation).
- Direction reviews #67, #69 — the bypass-to-dispatch ratio motivating the
  "consolidate before freeze" sequencing of the write surface.
- Direction reviews #71, #74, #76, #78 — the standing observation that
  `nmp-codegen` emits no Swift today and that the resulting hand-mirror
  in `KernelBridge.swift` is the primary velocity drag on the iOS host.
