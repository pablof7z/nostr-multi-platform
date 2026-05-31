# Proposal: C3 at the framework level — generated render-identity for scaffolded SwiftUI rows + `@Observable` observation granularity

> **Status:** Draft proposal (design only — no implementation in this PR).
> **Author tooling:** Opus design → Sonnet adversarial review → reconciled against live source.
> **Scope:** Layer 1 (row render-identity) is the v1-eligible deliverable; Layer 2 (`@Observable`) is specified in full but classified **post-v1**.

This document is a design proposal, not a plan-of-record. The temporal queue
(what is in-flight / queued) stays in `docs/BACKLOG.md` and `WIP.md` per the
repository's planning-discipline rule. The PR sequence in §6 is the proposed
work; nothing here is committed to a milestone until BACKLOG records it.

---

## 1. Summary

The C3 idle-re-render fix currently lives **only in the Chirp app**
(`ios/Chirp/Chirp/Bridge/TimelineItem+RenderIdentity.swift`, added by PR #880).
A new app scaffolded on NMP re-renders every row on every ≤4Hz snapshot tick
unless its author re-derives that pattern by hand. This proposal moves the
**rendered-field projection** into the framework so it is generated, not
hand-rolled:

1. **Codegen** (`crates/nmp-codegen/src/swift.rs`) gains a per-type
   `render_identity_fields` flag that emits a `rendersIdentically(_:)` method and
   a `RenderIdentifiable` marker conformance on flagged row structs.
2. **The CLI SwiftUI component registry** (`crates/nmp-cli/registry/swiftui/*`)
   consumes that protocol via a generic `.equatable()` row wrapper, so scaffolded
   list views short-circuit idle body re-evaluation.
3. **Chirp** deletes its hand-written extension and consumes the generated
   member like any other app.

**Honest scope boundary (per review).** Generated code is inert without a
consumer that calls it. This proposal generates the *predicate* (`rendersIdentically`)
for every row type and generates the *consumer wrapper* only for registry
components; **hand-written views still must opt in** to `.equatable()`. It does
**not**, by itself, reduce the measured ~4Hz idle re-render on Chirp — that
reduction was already shipped by #880 and is *preserved* here, not improved. The
remaining idle-invalidation up the view tree is owned by Layer 2 (post-v1). See
§3 and §5 for the precise decomposition.

This upholds the framework promise — "make it nearly impossible to build a broken
Nostr app" — by extending "broken" to include "re-renders the whole tree at
idle," for the scaffolded path.

---

## 2. Problem & current state

**C3 metric:** ~4 full-tree SwiftUI body re-evaluations/sec at idle on Chirp iOS
(`docs/wiki/ios-swiftui-idle-re-render.md`). The Rust actor pushes a binary
FlatBuffers snapshot at ≤4Hz; `KernelModel.apply(result:)`
(`ios/Chirp/Chirp/Bridge/KernelModel.swift:641`) reassigns the single
`@Published private(set) var snapshot: KernelUpdate?` slot
(`KernelModel.swift:47`) on **every** tick. Because the view-facing accessors are
*computed* through `snapshot?.x` (e.g. `items`, `metrics`, `relayStatuses`,
`modularTimeline` at `KernelModel.swift:46–52`), a single `objectWillChange`
fires per tick and SwiftUI re-evaluates every view that touches `model`.

**What PR #880 did — Chirp only:**

- Added `ios/Chirp/Chirp/Bridge/TimelineItem+RenderIdentity.swift` — a
  hand-written `func rendersIdentically(to other: TimelineItem) -> Bool`
  comparing the 13 rendered fields: `id, authorPubkey, authorDisplayName,
  authorPictureUrl, authorLnurl, content, contentPreview, createdAt, isRepost,
  kind, navTargetId, repostInnerContent, relayCount`.
- `private struct TimelineListView: View, Equatable`
  (`ios/Chirp/Chirp/Features/HomeFeedView.swift:204`), with a `nonisolated static
  func ==` (`HomeFeedView.swift:226–232`):

  ```swift
  nonisolated static func == (lhs: TimelineListView, rhs: TimelineListView) -> Bool {
      lhs.roots == rhs.roots
          && lhs.nextCursor == rhs.nextCursor
          && lhs.items.count == rhs.items.count
          && zip(lhs.items, rhs.items).allSatisfy { $0.rendersIdentically(to: $1) }
          && lhs.mentionProfiles == rhs.mentionProfiles
  }
  ```

  and `.equatable()` applied at `HomeFeedView.swift:128`.
- 6 unit tests in `ios/Chirp/ChirpTests/IdleReRenderTests.swift` (1 positive, 5
  negative controls), testing the pure `rendersIdentically` function — see
  BACKLOG **PD-042** for why the `TimelineListView`-construction tests were
  dropped (the struct is `private`, unreachable from `@testable import Chirp`).

**Why Chirp-only is insufficient.** `nmp init <app>` emits **only a Rust core
crate** — no `.swift` files. SwiftUI views ship separately via `nmp add
component` (`crates/nmp-cli/src/component.rs:25`), which `fs::write`s registry
sources **verbatim** (`component.rs:104–114`) — they are *copied source, not
codegen output*. The registry row views — e.g.
`crates/nmp-cli/registry/swiftui/relay-list/NostrRelayList.swift:99`
(`ForEach(relays) { relay in … }`) — use **zero** `.equatable()`. A developer
scaffolding a new app inherits the broken-at-idle pattern.

**The data types are already fine.** `TimelineItem` is generated as `Decodable,
Equatable, Identifiable, Hashable, Sendable`
(`ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift:162`). Crucially,
the generated `TimelineItem` carries *exactly* the 13 fields that
`rendersIdentically` compares — **there are no non-rendered fields** (no
`rawEventJSON`, no `decodedAt`) on it. So for `TimelineItem` *today*, Swift's
auto-synthesized `==` is already behaviorally equivalent to the hand-written
`rendersIdentically`. The gap is the **view wrappers + pattern enforcement**, and
(Layer 2) observation granularity.

---

## 3. Design principle

### Where the fix belongs

| Seam | Reaches | Verdict |
|---|---|---|
| `crates/nmp-codegen/src/swift.rs` (generated data types) | every app, every type, CI-byte-gated | **primary** — emit the render-identity predicate here |
| `crates/nmp-cli/registry/swiftui/*` (scaffolded views) | every `nmp add component` install | **secondary** — consume the predicate via `.equatable()` |
| Chirp shell (`HomeFeedView.swift`, `TimelineItem+RenderIdentity.swift`) | Chirp only | **delete/migrate** — stop being the source of truth |
| doc/lint only | enforcement, not generation | **insufficient alone** — flags a *missing* fix; cannot *make* a scaffold correct |

The predicate belongs **primarily in codegen** — the generated type is the one
artifact in 100% of apps and is already CI-byte-gated
(`.github/workflows/codegen-drift.yml`). The registry is the **consumption**
seam. Chirp becomes a consumer like any other app. A standalone doc-lint is
deferred (there is no SwiftUI doctrine-lint gate today; it is net-new
infrastructure and only flags hand-written views, which are out of scope here).

### What generated code can and cannot do (review correction)

The Opus draft framed this as "every app render-correct by construction." The
Sonnet review correctly flagged that as an **overclaim**. The precise truth:

- **Generated:** the `rendersIdentically(_:)` predicate on every flagged row
  type (reaches all apps).
- **Generated *only for registry components*:** the `.equatable()` wrapper that
  *calls* the predicate.
- **Still hand-written, still must opt in:** any bespoke view (including Chirp's
  own `TimelineListView`). PR-3 only *repoints* Chirp's existing call; it does
  not generate Chirp's wrapper.

So the C3 *metric reduction* is delivered by #880 (already shipped) plus Layer 2
(post-v1). PRs 1–3 deliver the **framework generalization** of the row
predicate, so a *scaffolded* app gets the row-level half for free. These two
things must not be conflated when reviewing the perf impact of PRs 1–3: **expect
no Chirp perf delta from PRs 1–3.**

### Synthesized full-field `Equatable` vs narrowed `rendersIdentically`

**Decision: emit a narrowed `rendersIdentically`; do not rely on synthesized
`==` for the row diff.** Keep the (free, already-emitted) synthesized `Equatable`
conformance — it is used elsewhere — but the *row diff* calls `rendersIdentically`.

Justification:

- The task brief's premise that `rawEventJSON`/`decodedAt` churn at idle is
  **inaccurate for this repo** — `TimelineItem` has no such fields, so for
  `TimelineItem` *today* narrowed == synthesized. **But the generalization must
  not bank on that coincidence:** the framework cannot guarantee future row types
  won't carry non-rendered fields (cursors, decode timestamps, server ordinals).
  Synthesized `Equatable` ties row identity to *every* `let`; the moment a
  non-rendered field is added, idle re-renders silently return. A declared
  projection is forward-safe.
- The real C3 churn is **not inside the row** — it is `KernelMetrics` (`bytesRx`,
  timing) at the *outer view level*, which is exactly why the brief forbids a
  whole-`KernelUpdate` equality guard (`KernelUpdate.metrics` changes every tick,
  so `if update == snapshot { return }` never fires). The narrowed
  `rendersIdentically` is the row-level half; Layer 2's per-concern slots are the
  outer-view half. Neither is served by full-field synthesized equality.

Which fields count as "rendered" is **not derivable from the JSON schema** —
`TypeEntry` carries no per-field display metadata (verified:
`crates/nmp-core/src/codegen_schema.rs:78–100`). So the rendered-field set is
**declared as host-side policy** in the codegen registry. Absent a declaration,
the safe default is "not a row type → no method emitted," which degrades to the
existing synthesized-`Equatable` behavior — never to *incorrect* (stale-row)
behavior.

---

## 4. Layer 1 implementation — full detail

### 4.1 Add a per-type render-identity declaration (two `TypeEntry` structs)

There are **two** `TypeEntry` structs and both must change (one-sided = silently
inert):

**Producer** — `crates/nmp-core/src/codegen_schema.rs:78` (`#[derive(Serialize)]`,
`&'static` fields, feature-gated `codegen-schema`):

```rust
// AFTER (add one field to the producer TypeEntry)
#[derive(Serialize)]
pub struct TypeEntry {
    pub rust_path: &'static str,
    pub swift_name: &'static str,
    pub id_field: Option<&'static str>,
    pub conformances: &'static [&'static str],
    /// Host-rendered fields for this row type, in stable declared order.
    /// Empty = not a row type (no `rendersIdentically` emitted). These are
    /// RUST snake_case names; the emitter camelCases them.
    pub render_identity_fields: &'static [&'static str],
    pub schema: Value,
}
```

Set it on the genuine row type `TimelineItem` in `dump_pilot_schemas()`
(`codegen_schema.rs:214`):

```rust
TypeEntry {
    rust_path: "nmp_core::kernel::types::TimelineItem",
    swift_name: "TimelineItem",
    id_field: Some("id"),
    conformances: &["Decodable", "Equatable", "Hashable", "Sendable"],
    render_identity_fields: &[
        "id", "author_pubkey", "author_display_name", "author_picture_url",
        "author_lnurl", "content", "content_preview", "created_at",
        "is_repost", "kind", "nav_target_id", "repost_inner_content",
        "relay_count",
    ],
    schema: schema_value::<TimelineItem>(),
},
```

> **Field-list order = byte output.** Use the declared `Vec`/slice order, never a
> `HashSet`. The order above matches #880's `rendersIdentically` for review
> familiarity; any deterministic order is acceptable as long as the regenerated
> file is committed.

The other 7 pilot types get `render_identity_fields: &[]` — they are
status/aggregate types, not rows. (Note: `RelayEditRow` in the codegen pilot is a
*different symbol* from the registry's hand-written `NostrRelayEditRow`; see §4.4.
Do **not** flag the codegen `RelayEditRow` expecting it to benefit the registry.)

> **D0 doctrine.** These are Rust field names only (no nip-noun substrings), so
> `nmp-core` stays D0-clean (verified against
> `crates/nmp-testing/bin/doctrine-lint/rules/d0.rs`). Always run
> `cargo test -p nmp-testing --test doctrine_lint_smoke`.

**Consumer** — `crates/nmp-codegen/src/swift.rs:37`. Mirror the field with
`#[serde(default)]` (the exact precedent set by `id_field` at `swift.rs:41`), so
old schema documents decode unchanged and **no `SUPPORTED_DOCUMENT_VERSION` bump
is required**:

```rust
struct TypeEntry {
    rust_path: String,
    swift_name: String,
    #[serde(default)]
    id_field: Option<String>,
    conformances: Vec<String>,
    #[serde(default)]
    render_identity_fields: Vec<String>,
    schema: TypeSchema,
}
```

### 4.2 Emit the marker conformance + the method (`swift.rs` `render_type`)

**Conformance** — insert `RenderIdentifiable` into the `BTreeSet` near
`swift.rs:341` (mirroring the auto-`Identifiable` pattern), and add it at a
**fixed position** in the ordered allowlist (`swift.rs:362–366`):

```rust
// near swift.rs:341
if entry.id_field.is_some() {
    conformances.insert("Identifiable".to_string());
}
if !entry.render_identity_fields.is_empty() {
    conformances.insert("RenderIdentifiable".to_string());
}

// swift.rs:362 — allowlist AND emit order
let conformances: Vec<&str> = [
    "Decodable", "Equatable", "RenderIdentifiable",
    "Identifiable", "Hashable", "Sendable",
]
.into_iter()
.filter(|c| conformances.contains(*c))
.collect();
```

**Method** — emit after the `id` accessor block (ends `swift.rs:409`) and before
the closing `out.push_str("}\n")` (`swift.rs:422`). `TimelineItem`'s `id_field`
is literally `"id"`, so the `if id_field != "id"` guard (`swift.rs:403`) skips the
computed accessor; the method is emitted right after the field block. Drive the
comparison list from `entry.render_identity_fields` in declared order, reusing
`snake_to_camel` (`swift.rs:515`):

```rust
if !entry.render_identity_fields.is_empty() {
    let comparisons: Vec<String> = entry
        .render_identity_fields
        .iter()
        .map(|f| {
            let c = snake_to_camel(f);
            format!("self.{c} == other.{c}")
        })
        .collect();
    out.push('\n');
    out.push_str("    /// Render-identity diff: true IFF every host-rendered\n");
    out.push_str("    /// field matches. Used by SwiftUI `.equatable()` row wrappers\n");
    out.push_str("    /// to skip body re-evaluation on non-visible (idle) ticks.\n");
    out.push_str("    public func rendersIdentically(_ other: Self) -> Bool {\n");
    out.push_str(&format!("        {}\n", comparisons.join("\n            && ")));
    out.push_str("    }\n");
}
```

**Resulting generated Swift for `TimelineItem`** (replaces the hand-written
extension):

```swift
public struct TimelineItem: Decodable, Equatable, RenderIdentifiable, Identifiable, Hashable, Sendable {
    public let authorDisplayName: String?
    // ... all 13 fields ...
    public let relayCount: UInt32

    public func rendersIdentically(_ other: Self) -> Bool {
        self.id == other.id
            && self.authorPubkey == other.authorPubkey
            // ... 13 rendered fields, declared order ...
            && self.relayCount == other.relayCount
    }
}
```

> **Signature change.** #880 uses `rendersIdentically(to:)`; the generated form
> is `rendersIdentically(_ other: Self)` (matches the marker protocol's `Self`
> requirement). The Chirp call site (`HomeFeedView.swift:230`) and tests change
> to the unlabeled form in PR-3. **Conformance/body coupling is intentional and
> enforced by construction:** the `if !is_empty()` gate emits the conformance and
> the body together — emitting one without the other is a compile error.

### 4.3 The marker protocol (hand-written, once per delivery channel)

Not emitted by `render_type` — a static support file:

- iOS shell: `ios/Chirp/Chirp/Bridge/RenderIdentifiable.swift` (new).
- CLI registry: a shared support file under `crates/nmp-cli/registry/swiftui/`
  delivered with row components (new registry component, e.g. `render-identity`).

```swift
/// A value type whose render-relevant fields can be compared cheaply so
/// SwiftUI `.equatable()` row wrappers skip body re-evaluation at idle.
public protocol RenderIdentifiable {
    func rendersIdentically(_ other: Self) -> Bool
}
```

### 4.4 Registry scaffold — consume via `.equatable()` (review-corrected)

**Critical correction from the Sonnet review.** The registry's row type is
`NostrRelayEditRow`, **hand-written inside the registry `.swift` file itself**
(`crates/nmp-cli/registry/swiftui/relay-list/NostrRelayList.swift:15`:
`public struct NostrRelayEditRow: Codable, Identifiable, Equatable`). It is a
*separate symbol* from the codegen'd `RelayEditRow` in
`KernelTypes.generated.swift`. Flagging the codegen `RelayEditRow` does **not**
make `NostrRelayEditRow` conform to `RenderIdentifiable`. Therefore:

- PR-2 must **hand-author** `RenderIdentifiable` conformance +
  `rendersIdentically(_:)` directly on `NostrRelayEditRow` in the registry file
  (registry rows are copied source, not codegen output).
- The codegen `RelayEditRow` row-flag is **not** added in PR-1 (it buys nothing
  for the registry, and the registry is its only would-be consumer). Only
  `TimelineItem` is flagged in PR-1.

The `ForEach` (`NostrRelayList.swift:99`) wraps the row in a generic
equatable container that diffs on `rendersIdentically`, ignoring the row's
closure (`onRelayTap: ((NostrRelayEditRow) -> Void)?` at line 76 is non-Equatable
— this is exactly why a plain `EquatableView` over the whole row fails and we diff
on the model only):

```swift
// BEFORE (NostrRelayList.swift:99)
ForEach(relays) { relay in
    RelayRow(relay: relay, /* ... */ onTap: { ... })
}

// AFTER
ForEach(relays) { relay in
    EquatableRow(model: relay) { model in
        RelayRow(relay: model, /* ... */ onTap: { ... })
    }
    .equatable()
}
```

with a small generic helper in the registry support file:

```swift
struct EquatableRow<Model: RenderIdentifiable, Content: View>: View, Equatable {
    let model: Model
    @ViewBuilder let content: (Model) -> Content
    var body: some View { content(model) }
    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.model.rendersIdentically(rhs.model)
    }
}
```

Apply the same treatment to `content-minimal/NostrMinimalContentView.swift`.
**Skip** `content-view/NostrContentView.swift` — its `ForEach` uses positional
`id: \.offset` identity, where `.equatable()` without stable model ids risks
incorrect diffing (flagged in findings; out of scope here).

> **Registry versioning.** Editing a registry `.swift` changes its
> `source_sha256` baseline (`component.rs` lock; `nmp.components.lock`). Bump the
> component version in `crates/nmp-cli/registry/registry.toml` so `nmp update
> component` surfaces the change cleanly rather than as a conflict.

### 4.5 `nmp init` scaffold

`nmp init` emits no SwiftUI today (Rust crate only). We **do not** add a SwiftUI
template to `init` — the registry is the correct delivery channel. Stated
explicitly so the gap is not silently assumed closed: a from-`init` app with
**zero components** has no SwiftUI to fix; enforcement begins at the first
`nmp add component`.

### 4.6 Regenerate the committed Swift (drift gate)

Any `render_type` byte change fails `.github/workflows/codegen-drift.yml`
(`check_swift`, `swift.rs:562`) until regenerated. **The same PR must** run:

```
cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas \
  | cargo run -p nmp-codegen -- gen swift --stdin \
      --out ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift
```

and commit the regenerated `KernelTypes.generated.swift` (and
`apps/fixture/nmp-app-fixture` only if it carries pilot types — confirm against
`fixture_zero_diff.rs`).

---

## 5. Layer 2 (`@Observable`) — full detail + v1-vs-post-v1 decision

### v1 decision: **post-v1.**

Justified from the canonical temporal files: `docs/plan.md` v1 exit criteria gate
on BACKLOG §1 violations, §4 v1-blockers, §3 pending decisions, and the
second-app spike. `@Observable`/C3-Layer-2 appears in **none** of them. The single
C3 BACKLOG entry, **PD-042**, is a *decided/closed* code-review fix to a Swift
test, not a v1 blocker. Layer 1, by contrast, rides the existing codegen pilot
and the CI-gated emitter, and closes the framework-promise gap for scaffolded
apps without touching v1 exit criteria — so it is proposed as v1-eligible.

> **Self-flagged uncertainty:** `docs/architecture/crate-boundaries.md` §5 was
> not opened during this design. The "codegen is build-time; native
> render-identity is post-v1 rendering" layering is inferred from `AGENTS.md` +
> `docs/plan.md`. **Confirm against the boundaries spec before writing the ADR.**

### Migration shape (implementation-spec, sequenced later)

**iOS 17 min-target: already satisfied** — `ios/Chirp/project.yml:9` and
`Chirp.xcodeproj/project.pbxproj:763,862` set
`IPHONEOS_DEPLOYMENT_TARGET = 17.0`. No bump needed; `@Observable` is available.

**The substantive work is NOT the macro — it is decomposing the single
`snapshot` slot.** `@Observable` alone does nothing here: every accessor reads
through `snapshot?.x`, and `apply` rewrites the whole `snapshot` every tick
(`KernelModel.swift:641`), so per-property tracking still sees one mutation per
tick. The fix is per-concern stored sub-objects:

1. `KernelModel.swift:42` — `final class KernelModel: ObservableObject,
   NostrProfileHost` → `@Observable final class KernelModel: NostrProfileHost`.
2. Remove all `@Published` (lines 47, 54, 58, 59, 60, 63, 70, 71, 72, 93, 170 —
   14 occurrences total).
3. **Decompose** into nested `@Observable` sub-objects held as `let` on
   `KernelModel`: `FeedModel`, `ProfileModel`, `MetricsModel`, `RelayModel`. In
   `apply` (`:641`), write each **conditionally** — assign only when the decoded
   sub-value differs (requires `Equatable` on the sub-snapshot types). Repoint the
   computed accessors from `snapshot?.x` to the matching sub-object.
4. **Isolate every-tick diagnostics** — `snapshotCount`/`lastSnapshotAt` and the
   per-frame `appMetrics` updates tick on *every* frame. Move them onto
   `MetricsModel`, read **only** by `DiagnosticsView`. This is what makes a
   metrics-only tick invalidate `DiagnosticsView` and nothing else.
5. **Call-site blast radius (~25 files, atomic):** `ChirpApp.swift`
   `@StateObject`→`@State`, `.environmentObject`→`.environment`; the
   `@EnvironmentObject` reads → `@Environment(KernelModel.self)`. Two-way binding
   sites (`$model.visibleLimit`/`$model.emitHz`, `KernelModel.swift:71–72`) need a
   local `@Bindable var model = model`. Test sites that construct `KernelModel()`
   drop `ObservableObject` expectations only.

**Critical constraint honored:** do **NOT** add a whole-`KernelUpdate`
`if update == snapshot { return }` guard — `KernelUpdate.metrics` changes every
tick, so it would never fire. Change-gating must be **per concern** (feed slot
only when feed changed), never whole-snapshot.

**Doctrine flag.** A prior PR deliberately collapsed everything behind one
`snapshot` slot as a "single source of truth" for the thin-shell doctrine
(`KernelModel.swift:22–33`). Splitting into per-concern slots may *appear* to
reintroduce Swift-side cached state. It must be framed as a **view-observation
partition of the same data** (no new business state), and **needs an ADR**
(`docs/decisions/0041-*.md` — next free number) before implementation.
Recommended de-risking: do the storage decomposition **first while still on
`ObservableObject`** (manual `objectWillChange.send()` per changed concern),
validate the per-concern invalidation hypothesis, **then** apply `@Observable`
mechanically.

---

## 6. PR sequence

**PR-1 — Codegen: emit render-identity (framework). [v1-eligible]**
- Scope: emit `rendersIdentically(_:)` + `RenderIdentifiable` conformance on
  flagged row types (only `TimelineItem` flagged); regenerate committed Swift.
- Files: `crates/nmp-core/src/codegen_schema.rs` (producer `TypeEntry` field +
  `TimelineItem` entry), `crates/nmp-codegen/src/swift.rs` (consumer `TypeEntry`
  + `render_type`), `crates/nmp-codegen/src/swift/tests.rs` (literal updates if
  any pilot fixture gains the field — `#[serde(default)]` keeps existing fixtures
  inert), regenerated `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift`.
- Tests: `cargo test -p nmp-codegen`, then always `cargo test -p nmp-testing
  --test doctrine_lint_smoke`.
- Merge gate: codegen-drift CI green (regenerated file committed); new
  `tests/swift_render_identity.rs` passing (§7); D0 clean.

**PR-2 — Registry + protocol: scaffold consumes the pattern (framework). [v1-eligible]**
- Scope: add `RenderIdentifiable.swift` support file (registry component + iOS
  shell copy); hand-author `RenderIdentifiable`/`rendersIdentically` on the
  registry's own `NostrRelayEditRow`; wrap registry rows in
  `EquatableRow(...).equatable()`; bump the component version.
- Files: `crates/nmp-cli/registry/swiftui/relay-list/NostrRelayList.swift`,
  `.../content-minimal/NostrMinimalContentView.swift`, new registry support file,
  `ios/Chirp/Chirp/Bridge/RenderIdentifiable.swift`,
  `crates/nmp-cli/registry/registry.toml`.
- Tests: `cargo test -p nmp-cli`, then `cargo test -p nmp-testing --test
  doctrine_lint_smoke`.
- Merge gate: registry hash-lock updated; component installs cleanly. Depends on
  PR-1 only for the *iOS shell* protocol parity; the registry conformance is
  self-contained (hand-written), so PR-2 is technically independent of PR-1 for
  the registry leg.

**PR-3 — Chirp cleanup: delete the hand-written extension (consumer). [v1-eligible]**
- Scope: delete `ios/Chirp/Chirp/Bridge/TimelineItem+RenderIdentity.swift`;
  repoint `HomeFeedView.swift:230` from `rendersIdentically(to: $1)` to
  `rendersIdentically($1)` (the generated unlabeled form); update
  `ChirpTests/IdleReRenderTests.swift` to the generated signature.
- Tests: Xcode build / `ChirpTests` locally (note: ChirpTests is **not** in CI —
  no `xcodebuild test` step, per PD-042) + `doctrine_lint_smoke`.
- Merge gate: Chirp builds; `IdleReRenderTests` green; no dangling reference to
  the deleted file. Depends on PR-1.

**PR-4 — `@Observable` migration (post-v1, separate track).**
- Scope per §5; **ADR first** (`docs/decisions/0041-*.md`). Sub-sequenced: (4a)
  storage decomposition on `ObservableObject`, (4b) `@Observable` macro +
  call-site migration.
- Merge gate: ADR accepted; not gated on v1.

---

## 7. Measurement / proof

**Primary CI-enforceable leg — Rust codegen test** (new file in its own
`tests/swift_render_identity.rs`, ≤300 LOC per file-size doctrine — do **not**
append to the 363-LOC `swift_codegen_regression.rs`):

```rust
use nmp_codegen::swift::render_swift; // pub fn, swift.rs:171; pub mod, lib.rs

const ROW_DOC: &str = r#"{
  "version": 1,
  "types": [{
    "rust_path": "x::SampleRow",
    "swift_name": "SampleRow",
    "id_field": "id",
    "conformances": ["Decodable", "Equatable", "Hashable", "Sendable"],
    "render_identity_fields": ["id", "display_name", "relay_count"],
    "schema": { "type": "object",
      "properties": {
        "id": {"type": "string"},
        "display_name": {"type": "string"},
        "relay_count": {"type": "integer", "format": "uint32"}
      },
      "required": ["id", "display_name", "relay_count"] }
  }]
}"#;

#[test]
fn row_type_emits_render_identity_member_and_conformance() {
    let out = render_swift(ROW_DOC).expect("renders");
    assert!(out.contains("public struct SampleRow:"));
    assert!(out.contains("RenderIdentifiable"));
    assert!(out.contains("public func rendersIdentically(_ other: Self) -> Bool"));
    assert!(out.contains("self.id == other.id"));
    assert!(out.contains("self.displayName == other.displayName"));
    assert!(out.contains("self.relayCount == other.relayCount"));
}

#[test]
fn non_row_type_emits_no_render_identity() {
    let doc = ROW_DOC.replace(
        r#""render_identity_fields": ["id", "display_name", "relay_count"],"#, "");
    let out = render_swift(&doc).expect("renders");
    assert!(out.contains("public struct SampleRow:")); // still emitted
    assert!(!out.contains("rendersIdentically"));       // but no member
    assert!(!out.contains("RenderIdentifiable"));       // and no marker
}
```

> **Proxy limitation (same caveat as `swift_codegen_regression.rs`):** this
> proves the generated **text** carries the construct, not that SwiftUI skips
> re-renders.

**Secondary byte-lock:** `fixture_zero_diff.rs` plus the regenerated
`KernelTypes.generated.swift` pin the exact output across the tree.

**Behavioral leg (Swift, un-gated):** keep/extend
`ChirpTests/IdleReRenderTests.swift` to call the **generated** `rendersIdentically`.
Per PD-042, ChirpTests is **not** wired into CI, so the Rust codegen test is the
only CI-enforceable proof — stated explicitly, not conflated.

---

## 8. Workflow execution plan

- **Opus (this doc):** design + architectural calls (narrowed-vs-synthesized,
  codegen-primary seam, post-v1 Layer 2). No code.
- **Haiku (per PR, in a git worktree):** implements one PR from §6 to spec; runs
  **only** the scoped commands (`cargo test -p nmp-codegen` / `-p nmp-cli`, then
  always `cargo test -p nmp-testing --test doctrine_lint_smoke`); regenerates
  committed Swift in the same PR; never `cargo test --workspace`.
- **Sonnet (review gate, before each merge):** verifies determinism (regenerated
  files committed, no `HashSet` iteration in emitted lists), D0 cleanliness of
  `codegen_schema.rs`, file-size ceiling on the new test file, signature
  consistency (`rendersIdentically(_:)` across codegen + Chirp + registry), the
  `NostrRelayEditRow`-is-hand-written correction, and that no whole-snapshot
  equality guard was introduced.
- **Measure (after each merge):** PR-1/PR-2 — codegen-drift +
  `swift_render_identity.rs` green. PR-3 — Chirp builds, `IdleReRenderTests`
  green, extension deleted. PR-4 — Xcode profile showing metrics-only ticks
  invalidate only `DiagnosticsView`.

Loop mapping: PR-1 → PR-2 → PR-3 is the v1-eligible framework loop (each: Haiku
implement → Sonnet review → merge → measure). PR-4 is a separate post-v1 loop
gated on its ADR.

---

## 9. Risks & non-goals

- **Determinism / drift gate (highest):** any `render_type` byte change fails
  `.github/workflows/codegen-drift.yml`. Mitigation: regenerate
  `KernelTypes.generated.swift` in the **same** PR. The `rendersIdentically` list
  **must** iterate the declared slice/`Vec` — never a `HashSet`/`HashMap`.
- **Two-struct / two-crate / feature-gate trap:** the flag must be added to
  **both** the consumer `TypeEntry` (`swift.rs:37`, `#[serde(default)]`) **and**
  the producer `TypeEntry` (`codegen_schema.rs:78`) and set in
  `dump_pilot_schemas()`. `codegen_schema.rs` compiles only under
  `--features codegen-schema`.
- **Which tests actually trip (review correction):** the 8-entry order-pin
  (`codegen_schema.rs:238`, `pilot_document_has_eight_entries_in_stable_order`)
  asserts the exact ordered **name vector** — adding `render_identity_fields` does
  **not** change names/order, so it does **not** trip. The header-literal test in
  `swift/tests.rs` does **not** trip either, because its fixture lacks
  `render_identity_fields` and `#[serde(default)]` keeps the new behavior inert.
  Only the regenerated `KernelTypes.generated.swift` and the new
  `swift_render_identity.rs` are affected. (The Opus draft's "existing emitter
  tests will trip" was inaccurate.)
- **Conformance/body coupling:** `RenderIdentifiable` requires
  `rendersIdentically(_:)`; the `if !render_identity_fields.is_empty()` gate ties
  them together — never split.
- **D0 doctrine-lint:** new `codegen_schema.rs` values are Rust field names only;
  run `doctrine_lint_smoke` (D-gates trip silently otherwise).
- **The whole-snapshot-equality trap (Layer 2):** explicitly do **not** add
  `if update == snapshot { return }` — `KernelUpdate.metrics` churns every tick.
- **Backward compat:** the new consumer field is `#[serde(default)]`, so old
  schema documents decode unchanged; no `SUPPORTED_DOCUMENT_VERSION` bump.

**Non-goals / what the scaffold cannot cover:**
1. Hand-written views that don't use the registry get **no** automatic fix — only
   a future doc/lint can flag those (deferred).
2. `content-view/NostrContentView.swift`'s positional `id: \.offset` `ForEach` is
   intentionally **excluded** from `.equatable()` (unstable identity → incorrect
   diffing).
3. `nmp init` with zero components emits no SwiftUI — nothing to make correct
   until `nmp add component`.
4. The Rust codegen test proves *presence of the construct in generated text*,
   not runtime re-render suppression; the behavioral proof (`IdleReRenderTests`)
   is currently un-gated in CI per PD-042.
5. **PRs 1–3 do not change Chirp's measured C3.** #880 already shipped Chirp's
   row-level fix; PRs 1–3 generalize it to the scaffold. The remaining idle
   invalidation up the tree is owned by Layer 2 (post-v1).

---

## Appendix — verified ground-truth anchors

| Claim | Anchor (verified this session) |
|---|---|
| Single `@Published snapshot` slot, computed accessors | `ios/Chirp/Chirp/Bridge/KernelModel.swift:42,47,46–52,641` |
| Chirp consumer `==` + `.equatable()` | `ios/Chirp/Chirp/Features/HomeFeedView.swift:128,204,226–232` |
| Hand-written extension (13 fields, `to:` label) | `ios/Chirp/Chirp/Bridge/TimelineItem+RenderIdentity.swift:21,31` |
| Generated `TimelineItem` = exactly 13 fields, no internal fields | `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift:162–176` |
| Emitter conformance allowlist + auto-Identifiable | `crates/nmp-codegen/src/swift.rs:339–343,362–366,402–422` |
| Consumer `TypeEntry`, `#[serde(default)]` precedent | `crates/nmp-codegen/src/swift.rs:37–45` |
| Producer `TypeEntry` + `dump_pilot_schemas` | `crates/nmp-core/src/codegen_schema.rs:78–100,128–223` |
| 8-entry order-pin asserts exact name vector | `crates/nmp-core/src/codegen_schema.rs:238,250–263` |
| Registry rows copied verbatim (not codegen) | `crates/nmp-cli/src/component.rs:25,104–114` |
| Registry `NostrRelayEditRow` is hand-written, has closure | `crates/nmp-cli/registry/swiftui/relay-list/NostrRelayList.swift:15,76,99` |
| iOS min target 17.0 (`@Observable` available) | `ios/Chirp/project.yml:9`, `Chirp.xcodeproj/project.pbxproj:763,862` |
| Byte-diff proof harness style | `crates/nmp-codegen/tests/fixture_zero_diff.rs` |
| PD-042: ChirpTests not in CI; test the pure fn | `docs/BACKLOG.md:724–748` |
| Next free ADR number | `docs/decisions/0040-*` → 0041 |
