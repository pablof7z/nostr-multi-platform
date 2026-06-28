# NoteActionsRow Gallery Extraction — Preflight Contract

> **Status**: Preflight only. This doc defines the extraction contract so the
> actual work is *predictable and gated*, not a leap. Note: the two platforms
> are materially divergent today (text-vs-icon rendering, button order,
> optimistic-like behavior, dialog ownership), so the extraction is **not** a
> pure mechanical move — it requires an Android reskin, two dialog
> relocations, new optimistic-like behavior, and a render-parity gate (§5.3,
> B10–B12). Extraction is explicitly post-v1 (#997, phase:post-v1). Do **not**
> implement the extraction until F-08 (registry) and post-v1 action/dispatch
> stability land, and not before the §5.4 `authorLnurl` decision has owner
> sign-off.
>
> **Related**: GitHub issue #997.

---

## 1. Current Location (per platform)

| Platform | File | Symbol | Lines |
|----------|------|---------|-------|
| iOS/SwiftUI | `apps/chirp/ios/Chirp/Components/NoteRowView.swift` | `NoteActionsRow` | 285–408 |
| Android/Compose | `apps/chirp/android/app/src/main/java/org/nmp/android/ui/NoteActions.kt` | `NoteActionsSummary` | full file (1–212) |
| iOS (divergent copy) | `apps/chirp/ios/Chirp/Components/ThreadNoteRow.swift` | inline action HStack | 109–143 |

The thread-view action bar (ThreadNoteRow.swift:109-143) is a manually-kept
copy of NoteActionsRow — same icons, same order, same comment calling it
identical — but it is NOT a call to NoteActionsRow. Extraction fixes this
structural duplication.

---

## 2. Current Contract (as-is)

### 2.1 iOS — `NoteActionsRow`

```swift
struct NoteActionsRow: View {
    // ── Inputs ──────────────────────────────────────────────────────
    let item: NoteRowModel          // uses: item.id, item.authorPubkey only
    let authorLnurl: String?        // pre-extracted by parent from profileCard.lnurl
    let relationCounts: NoteRelationCounts?

    // ── Callbacks ────────────────────────────────────────────────────
    let onLike: (String) -> Void                     // (eventID) -> Void
    var onRepost: ((String, String) -> Void)? = nil  // (eventID, authorPubkey) -> Void
    var onZap: ((String, String, String) -> Void)? = nil  // (eventID, authorPubkey, lnurl)

    // ── External state bindings (coupling: see §4) ────────────────────
    @Binding var likeTapped: Bool   // owned by parent NoteRowView @State
    @Binding var showReply: Bool    // owned by parent; parent's .sheet presents ComposeView
}
```

`NoteRelationCounts` is a Chirp-app type (`TimelineBlock.swift:282`):

```swift
struct NoteRelationCounts: Decodable, Equatable, Sendable {
    let replies: RelationCount    // enum: .known(UInt64) | .loading
    let reactions: RelationCount
    let reposts: RelationCount
    let zaps: RelationCount
}
```

### 2.2 Android — `NoteActionsSummary`

```kotlin
@Composable
internal fun NoteActionsSummary(card: ChirpEventCard?, model: KernelModel?) {
    // reads card.relationCounts directly
    // dispatches via model.react(), model.repost(), model.zapNote(),
    //   model.publishNote()
    // owns local dialog state: showReplyDialog, showZapDialog, zapAmountText
    // zap shows inline AlertDialog with preset sats buttons
}
```

### 2.3 Current dispatch path

Both platforms dispatch through `KernelModel` → FFI C ABI:

| Action | iOS symbol | Android symbol | ActionModule key |
|--------|-----------|----------------|-----------------|
| Like / React | `kernel.react(targetEventID:reaction:)` | `model.react(card.id, "❤")` | `nmp.nip25.react` |
| Repost | `kernel.repost(eventID:authorPubkey:)` | `model.repost(card.id, card.authorPubkey)` | `nmp.nip18.repost` |
| Zap | `kernel.zap(...)` | `model.zapNote(...)` | `nmp.nip57.zap` |
| Reply | reply sheet (no direct dispatch — compose path) | `model.publishNote(content, card.id)` | `nmp.nip01.publish_note` |

iOS callbacks are wired in `HomeFeedView.swift:131-141`; the kernel calls go
through `KernelModel+Commands.swift:91-98` and exit through the byte doorway
(`ActionBuilders.kt:83-106` on Android shows the FlatBuffer encoding).

---

## 3. Display-Separation Check

**iOS NoteActionsRow**: compliant. The component receives `item.authorPubkey`
as a raw hex string and `authorLnurl` as a plain `String?`. It renders no
display name, no avatar, no profile display resolution. The parent `NoteRowView`
resolves `model.profileCard(forPubkey:)?.lnurl` before injection. No
`display::` helpers are touched inside the component.

**Android NoteActionsSummary**: structurally coupling concern is the
`KernelModel` injection (see §4 blocker B5), not display helpers. The
component does not resolve profile display data directly either.

**NoteRelationCounts**: counts arrive pre-resolved from the Rust
FlatBuffer projection (`nmp_nip01_NoteRelationCounts`). No display
resolution occurs inside the component.

---

## 4. Coupling / Blocker Checklist

Twelve blockers (B1–B12). B1–B8 are decouplings; B10 (reply-dialog
relocation), B11 (render-parity gate), and B12 (a11y contract) close the gaps
the parity audit surfaced; B9 is a post-extraction follow-up. Numbering is
kept stable for cross-references; the *execution* order is §7.

### B1: Define `NoteRelationCountsWire` as a registry-owned type

**Why**: `NoteRelationCounts` (`TimelineBlock.swift:282`) is Chirp-app-private.
A gallery component cannot import it. The registry needs its own mirror that
any NMP app can reference without depending on Chirp internals.

**Where it lives**: Either as a small shared component
(`swiftui/note-relation-counts` + `compose/note-relation-counts`) or bundled
inside `note-actions-row`'s file set. The standalone option is preferable
because relation counts are reused by any future action/stats surface.

**Shape**:

```swift
// SwiftUI
public struct NoteRelationCountsWire: Decodable, Equatable, Sendable {
    public let replies: NoteRelationCount
    public let reactions: NoteRelationCount
    public let reposts: NoteRelationCount
    public let zaps: NoteRelationCount
}
public enum NoteRelationCount: Decodable, Equatable, Sendable {
    case known(UInt64)
    case loading
    public var value: UInt64? { ... }
}
```

```kotlin
// Compose
data class NoteRelationCountsWire(
    val replies: NoteRelationCount,
    val reactions: NoteRelationCount,
    val reposts: NoteRelationCount,
    val zaps: NoteRelationCount,
)
sealed class NoteRelationCount {
    data class Known(val count: ULong) : NoteRelationCount()
    object Loading : NoteRelationCount()
    val value: ULong? get() = (this as? Known)?.count
}
```

**Chirp wiring**: The existing Chirp `NoteRelationCounts` becomes a
typealias or is updated in-place (one struct definition, no breakage elsewhere
since it's only used in NoteRowView + TypedHomeFeedDecoder).

**Acceptance criterion**: `swiftui/note-relation-counts` component installs
cleanly without importing any Chirp-specific type.

---

### B2: Internalize `likeTapped` state (iOS only)

**Why**: `@Binding var likeTapped: Bool` externalizes the optimistic-UI
animation state to the parent `NoteRowView`. A registry component must own
its own ephemeral UI state. The parent does not need to observe it.

**Fix**: Change to `@State private var likeTapped = false` inside
`NoteActionsRow` (or `NostrNoteActionsRow` after rename). Remove the
`@State private var likeTapped = false` from `NoteRowView` and drop the
`likeTapped: $likeTapped` binding at the call site (`NoteRowView.swift:116`).

**Acceptance criterion**: `NoteRowView` no longer declares or passes
`likeTapped`. The like animation still fires correctly on first tap.

---

### B3: Replace `showReply` binding with `onReply: () -> Void` callback (iOS only)

**Why**: `@Binding var showReply: Bool` causes the component to toggle a
parent-owned flag so the PARENT's `.sheet` presents `ComposeView`. This
couples the component to the parent's sheet machinery. A gallery component
must raise a pure callback and let the host decide how to present the compose
surface.

**Fix**: Replace `@Binding var showReply: Bool` with `var onReply: (() -> Void)?`.
In `NoteRowView`, the call site becomes:
```swift
NoteActionsRow(
    ...
    onReply: { showReply = true },
    ...
)
```
The `@State private var showReply` and `.sheet` stay in `NoteRowView`.

**Acceptance criterion**: `NoteActionsRow` has no `@Binding` parameters.
The compose sheet still appears from `NoteRowView` when reply is tapped.

---

### B4: Replace `ChirpColor.accent` with `Color.accentColor` (iOS only)

**Why**: `ChirpColor.accent` is an app-level static
(`apps/chirp/ios/Chirp/Theme/ChirpTheme.swift:9`). Registry components must
not import app-level theme types. The system-semantic replacement is
`Color.accentColor`, which resolves to whatever the host's `.tint(...)` is
set to. Chirp already sets `.tint(ChirpColor.accent)` at the app root
(`ChirpApp.swift:32`), so the switch is transparent.

**Fix**: In `NoteActionsRow.likeButton`, change:
```swift
.foregroundStyle(likeTapped ? ChirpColor.accent : .secondary)
```
to:
```swift
.foregroundStyle(likeTapped ? Color.accentColor : .secondary)
```

**Acceptance criterion**: No import of `ChirpColor` or any Chirp-module type
inside the extracted component file.

---

### B5: Decouple Android `NoteActionsSummary` from `KernelModel`

**Why**: The composable directly calls `model.react(...)`, `model.repost(...)`,
`model.zapNote(...)`, `model.publishNote(...)`. Registry components must not
depend on app-level kernel handles. The real doctrine source is the component
contract in `docs/cli.md:164-172`:

> "Components are pure renderers. They do not fetch, retry, cache, route, or
> decide policy. Apps hydrate display models … Component packages must not
> import runtime, C ABI/JNI/WASM, worker, or kernel handles directly."

and the thin-shell conformance gate in
`docs/builder-guide/21-framework-magic.md:133-134`:

> "The doctrine smoke gate enforces the negative side: component packages must
> not import runtime, ABI/JNI/WASM, worker, or kernel handles directly."

(The doctrine-lint smoke gate, `cargo test -p nmp-testing --test
doctrine_lint_smoke`, enforces this negative side.)

**Fix**: Remove the `model: KernelModel?` parameter. Replace every dispatch
call with a callback:
- `model.react(card.id, "❤")` → `onLike?()`
- `model.repost(card.id, card.authorPubkey)` → `onRepost?()`
- `model.zapNote(...)` → `onZap?()` (zap dialog relocates per B7)
- `model.publishNote(content, card.id)` → `onReply?()` (reply dialog relocates
  per B10; see that blocker — this is not a one-line swap, the inline
  `ComposeNoteDialog` must move to the host).

The `ChirpEventCard` parameter is also replaced with scalar inputs (see B6).

**Acceptance criterion**: `NoteActionsSummary` imports no `KernelModel`,
`SocialActions`, or any JNI/FFI handle type.

---

### B6: Replace `ChirpEventCard` parameter with scalar inputs (Android only)

**Why**: `NoteActionsSummary(card: ChirpEventCard?, ...)` couples the component
to a Chirp-specific generated FlatBuffer type. The gallery version must accept
plain scalars.

**Fix**: Replace with `eventId: String`, `authorPubkey: String`, and
`counts: NoteRelationCountsWire?` (from B1). The caller (`TimelineScreen` or
equivalent) projects the card fields at the call site.

**Acceptance criterion**: No `ChirpEventCard` import in the extracted file.

---

### B7: Unify zap UX contract across platforms

**Why**: The platforms differ fundamentally:
- iOS: `onZap(eventID, authorPubkey, lnurl)` fires; parent presents a zap
  amount sheet (`ZapAmountSheet`) as a separate swipe-up overlay.
- Android: `NoteActionsSummary` owns an inline `ZapAmountDialog` (AlertDialog
  with preset sats buttons), presenting it in-place and calling
  `model.zapNote(...)` on confirm.

The gallery contract must specify one behaviour. The correct target is the
**iOS pattern** (callback, app-owned presentation) for the following reasons:
(1) components must not own dialogs that dispatch to the kernel; (2) the
amount picker is app policy, not component policy; (3) future NWC / wallet
connectors may intercept the zap path between the callback and the actual
dispatch.

**Target**:
```swift
// SwiftUI
var onZap: (() -> Void)?   // lnurl confirmed by caller; tap fires this
```
```kotlin
// Compose
val onZap: (() -> Unit)? = null
```
Whether the zap button renders enabled or muted is governed by the §5.4
design decision (recommended: a host-supplied `zapEnabled: Bool`, not shell
lnurl parsing). The callback carries no arguments; the caller already has the
eventId, authorPubkey, and any lnurl it needs at the call site.

**Android migration**: Remove `ZapAmountDialog` from `NoteActions.kt` and
move it to the Chirp app layer (e.g., into a `ZapSheet.kt` composable owned
by `TimelineScreen`). Wire `onZap = { showZapDialog = true }` at the call
site.

**Acceptance criterion**: No dialog or sheet is owned inside the registry
component on either platform. The `ZapAmountDialog` exists only in app-level
code.

---

### B8: Make `NoteActionsSummary` public (Android only)

**Why**: It is currently `internal`. Registry components must be `public` so
any host app can import them after installation.

**Acceptance criterion**: `public fun NostrNoteActionsRow(...)` (renamed per
§5 naming).

---

### B10: Relocate Android reply dialog to the host

**Why**: `NoteActions.kt:100-111` owns an inline `ComposeNoteDialog` that calls
`model.publishNote(content, card.id)` on confirm. This is the same class of
violation as the zap dialog (B7): a registry component must not own a dialog
that dispatches to the kernel. The B5 mapping "`model.publishNote → onReply?()`"
is therefore **not** a one-line swap — the dialog and its publish call must
move out.

**Fix**: Delete `ComposeNoteDialog` from `NoteActions.kt`. The component fires
`onReply?()`. The host (e.g. `TimelineScreen`) owns the compose/reply surface
and the `model.publishNote(...)` dispatch — mirroring how iOS already presents
`ComposeView` from `NoteRowView`'s `.sheet`.

**Acceptance criterion**: `NoteActions.kt` contains no `ComposeNoteDialog` and
no `model.publishNote` call; reply is a pure callback on both platforms.

---

### B11: Render-parity golden gate (mandatory)

**Why**: §5.3 documents real visual/behavioral divergence (text-vs-icon,
button order, optimistic-like). This is exactly the drift class that #2268
closed for the identicon by adding cross-platform golden tests
(`IdenticonGoldenTests.swift` + `IdenticonGoldenTest.kt`) and the
`ComponentVendorDriftGateTest` source-parity gate. Without an equivalent gate,
the two `NostrNoteActionsRow` implementations will re-diverge.

**Fix**: Add a render/behavior parity gate analogous to #2268:
- A golden/snapshot assertion that the same inputs (eventID, counts,
  zapEnabled) produce the same logical layout — button set, order, enabled
  state, and which counts are shown — on iOS (XCTest) and Android (JUnit).
- If the registry ships a Chirp vendored copy, a source-parity gate
  (`ComponentVendorDriftGateTest` style) asserting the vendored file matches
  the registry SSOT.

**Acceptance criterion**: a failing-on-drift test exists on both platforms;
button order / set / enabled-state divergence breaks CI.

---

### B12: Pin the accessibility contract on both platforms

**Why**: iOS already exposes `.accessibilityLabel` and
`.accessibilityIdentifier` on each action (`NoteRowView.swift:349-394`:
`note-zap-button`, "Zap", "Reply", "Repost", "Like"). Android's
`RelationActionLabel` (`NoteActions.kt:183-199`) is a bare `clickable` `Text`
with no semantics, role, or test identifier. Extraction must not lose the iOS
a11y surface, and Android must gain it.

**Fix**: Define the a11y contract the registry component guarantees on both
platforms:
- Accessibility label per action: "Reply", "Repost", "Like", "Zap".
- Button role/trait (iOS `.isButton` via `Button`; Compose
  `Modifier.semantics { role = Role.Button }` / `onClickLabel`).
- Stable test identifiers (iOS `accessibilityIdentifier`, Compose
  `Modifier.testTag`) — e.g. `note-action-reply/repost/like/zap` — used by the
  B11 parity tests.

**Acceptance criterion**: both platforms expose identical labels, button
roles, and test identifiers; a UI test can find each action by a shared
identifier.

---

### B9: Decide ThreadNoteRow.swift fate (follow-up, not a blocker)

**Why**: `ThreadNoteRow.swift:109-143` is a manually-maintained copy — but a
*reduced* one: it has only **Reply, Repost, Like** with **no zap button and no
counts** (it renders icon-only `threadActionLabel`s with no count text).
Adopting `NostrNoteActionsRow` there is therefore a **behavior addition** (zap
affordance + relation counts appear in the thread view), not just spacing
reconciliation. Layout also differs (`HStack(spacing: 28)` vs `spacing: 0` +
`Spacer()`).

**Recommendation**: thread view adopts the registry component once it lands,
which intentionally *adds* zap + counts to the focused-note row (a product
decision — confirm it's wanted). Needs either an optional `spacing` parameter
or a call-site layout modifier, plus host wiring for the new zap/counts. This
is a follow-up issue gated on the extraction landing, **not** a blocker for the
registry component itself. File the follow-up when extraction begins.

---

## 5. Target Registry Contract

### 5.1 SwiftUI — `swiftui/note-actions-row`

Registry id: `swiftui/note-actions-row`
Version: `0.1.0`
Dependencies: none (counts wire bundled in component or in
`swiftui/note-relation-counts`)

Public surface:

```swift
/// Social action bar for a Nostr note: reply / repost / like / zap.
///
/// Pure renderer — dispatches nothing itself. All actions surface through
/// callbacks; the host app owns dispatch and any sheet/dialog presentation.
///
/// Display-separation: receives only raw scalar inputs (event ID, pubkey,
/// lnurl string). No profile resolution happens inside this component.
public struct NostrNoteActionsRow: View {
    // ── Inputs ─────────────────────────────────────────────────────
    public let eventID: String
    public let authorPubkey: String
    /// Relation counts from the kernel projection. Nil = not yet loaded.
    public let counts: NoteRelationCountsWire?
    /// Controls zap-button enablement. PROVISIONAL — see §5.4 DESIGN DECISION.
    /// Recommended (option a): `zapEnabled: Bool` (host-supplied, Rust-derived
    /// zapability; no lnurl in the shell). Current iOS shape is
    /// `authorLnurl: String?`; whether to keep it is flagged for owner
    /// sign-off because it changes this signature and the zap-visibility rule.
    public let zapEnabled: Bool   // provisional; or `authorLnurl: String?` per §5.4

    // ── Callbacks ───────────────────────────────────────────────────
    /// Tapping reply fires this; host is responsible for presenting the
    /// compose / reply surface.
    public var onReply: (() -> Void)?
    /// Tapping repost fires this; host dispatches nmp.nip18.repost.
    public var onRepost: (() -> Void)?
    /// Tapping like fires this; host dispatches nmp.nip25.react.
    public var onLike: (() -> Void)?
    /// Tapping zap fires this (only shown when authorLnurl != nil AND
    /// this closure is non-nil). Host presents amount picker and
    /// dispatches nmp.nip57.zap on confirm.
    public var onZap: (() -> Void)?
}
```

### 5.2 Compose — `compose/note-actions-row`

Registry id: `compose/note-actions-row`
Version: `0.1.0`
Dependencies: none (or `compose/note-relation-counts`)

Public surface:

```kotlin
/**
 * Social action bar for a Nostr note: reply / repost / like / zap.
 *
 * Pure renderer — dispatches nothing itself. All actions surface through
 * callbacks; the host app owns dispatch and any dialog presentation.
 *
 * Display-separation: receives only raw scalar inputs (event ID, pubkey,
 * lnurl string). No profile resolution happens inside this composable.
 */
@Composable
public fun NostrNoteActionsRow(
    eventId: String,
    authorPubkey: String,
    /** Relation counts from the kernel projection. Null = not yet loaded. */
    counts: NoteRelationCountsWire? = null,
    /** Controls zap-button enablement. See §5.4 DESIGN DECISION — this field's
     *  shape (String? lnurl vs Bool zapEnabled) is unresolved and flagged for
     *  owner sign-off. */
    zapEnabled: Boolean = true,
    onReply: (() -> Unit)? = null,
    onRepost: (() -> Unit)? = null,
    onLike: (() -> Unit)? = null,
    /** Fired only when zapEnabled AND this callback is non-null. */
    onZap: (() -> Unit)? = null,
)
```

> **Note**: the SwiftUI surface in §5.1 currently shows `authorLnurl: String?`.
> Whether both platforms expose `authorLnurl` (shell-side gating) or a
> platform-neutral `zapEnabled: Bool` (host/Rust decides zapability) is an
> open design decision — see **§5.4**. The signatures above are provisional on
> that decision.

### 5.3 Cross-platform parity — Current (divergent) vs Target

**The two platforms are materially divergent today.** Extraction is NOT a
pure mechanical move: reaching a single registry component requires reskinning
one platform's visuals and adding new behavior to Android. This table makes the
real deltas explicit (each row that differs is a concrete change, not a
rename).

| Behavior | iOS current | Android current | Target | Delta |
|----------|-------------|-----------------|--------|-------|
| Render style | **SF Symbol icons** (`bubble.left`, `arrow.2.squarepath`, `heart`, `bolt`) — `NoteRowView.swift:303-354` | **Plain TEXT labels** ("Reply", "React", "Repost", "Zap") — `NoteActions.kt:189` `RelationActionLabel` | Icons (SF Symbols ↔ Material icons) | **Android must be reskinned from text to icons.** Largest single gap. |
| Button order | Reply, **Repost, Like**, Zap | Reply, **React, Repost**, Zap (swapped) — `NoteActions.kt:74-91` | Reply, Repost, Like, Zap | Android reorders React/Repost. |
| Like / React label | "Like" (heart) | "React" (❤ glyph) | One canonical term + icon | Naming + glyph reconciliation. |
| Optimistic like | Guards `!likeTapped`, fills `heart.fill`, accent color, spring scale — `NoteRowView.swift:364-376` | Dispatches **every** tap, no local state, no animation — `NoteActions.kt:77-80` | Optimistic: fill+accent on first tap, idempotent no-op after | **NEW Android behavior** (local state + idempotency + animation). |
| Counts display | Shown only when `> 0`; `"..."`-free | Shown as `"$label $count"`, prints `"..."` when null — `NoteActions.kt:190` | Shown only when `> 0`; no loading text | Android drops the `"..."` placeholder. |
| Like color | `ChirpColor.accent` → target `Color.accentColor` (B4) | `MaterialTheme.colorScheme.primary` | System accent per platform | Already each-platform-idiomatic; OK. |
| Haptics | `UIImpactFeedbackGenerator` in component | none | iOS keeps haptic; Android optional `LocalHapticFeedback` | Android may add haptic (optional). |
| Zap gating | `onZap` + `authorLnurl != nil` → enabled; else muted placeholder — `NoteRowView.swift:338-360` | Zap shown **unconditionally**, muted styling, fails closed in Rust — `NoteActions.kt:52-54,86-91` | **See §5.4 — open decision** | Resolution depends on §5.4. |
| Zap amount UX | Callback → host presents `ZapAmountSheet` | Component owns inline `ZapAmountDialog` — `NoteActions.kt:113-179` | Callback-only, host-owned (B7) | Android moves dialog to host. |
| Reply UX | Callback → host presents `ComposeView` sheet | Component owns inline `ComposeNoteDialog` — `NoteActions.kt:100-111` | Callback-only, host-owned (B10) | Android moves dialog to host. |

Render parity must be locked by a golden test (see **B11**), the direct
analogue of the #2268 identicon source-parity gate.

### 5.4 DESIGN DECISION — `authorLnurl` shape (owner sign-off required)

The platforms disagree on where zapability is decided, and this is **not** a
detail to settle silently:

- **iOS** passes `authorLnurl: String?` into the component; the shell resolved
  it via `model.profileCard(forPubkey:)?.lnurl` before injection. The component
  hides/mutes the zap button when lnurl is nil.
- **Android** deliberately carries **no** `authorLnurl`. Its doc comment is
  explicit (`NoteActions.kt:52-54`): *"the recipient `lnurl` is resolved
  kernel-side from the author's kind:0 … and a missing LN address fails closed
  in Rust rather than in the shell."* Zap is shown unconditionally.

Mandating `authorLnurl: String?` on the Android surface (as the first draft of
this doc did) would **push lnurl resolution into the Android shell** — a
thin-shell regression — and **change Android zap-visibility behavior** (from
always-shown to conditionally-muted). Per the thin-shell doctrine
(`docs/cli.md:164-172`: components "do not … decide policy"; the shell does not
parse metadata), the shell deciding zapability from a parsed lud16/lud06 is the
wrong layer.

**Options:**

- **(a) Rust-fail-closed (RECOMMENDED).** The component exposes a
  platform-neutral `zapEnabled: Bool` (default `true`), and the *host* (not the
  component, not raw metadata parsing in the shell) supplies it from a
  Rust-derived zapability fact. When `zapEnabled == true` the zap button is
  enabled and `onZap` fires; Rust still fails closed if no lnurl exists at
  dispatch time. This keeps lnurl out of both shells, matches the existing
  Android posture, and removes the iOS shell's `profileCard.lnurl` lookup
  (a small thin-shell improvement on iOS too). The host may compute
  `zapEnabled` from the same keyed-profile sidecar the kernel already
  produces — no new shell parsing.
- **(b) iOS-style shell gating.** Keep `authorLnurl: String?` on both
  platforms; Android gains an lnurl lookup in the shell. Rejected as a
  thin-shell regression; listed only for completeness.

**Recommendation: (a).** It is the more doctrine-correct option and the
smaller net change (it *removes* shell lnurl handling rather than adding it).
**Flagged for owner sign-off** — this changes the public component signature
(§5.1/§5.2) and a user-visible zap-visibility rule, so it should not be settled
by the extraction PR author alone.

---

## 6. Registry Manifest Entries

When the extraction lands, these blocks go into
`crates/nmp-cli/registry/registry.swiftui.toml` and
`crates/nmp-cli/registry/registry.compose.toml` respectively.

### SwiftUI

```toml
[[components]]
id = "swiftui/note-relation-counts"
version = "0.1.0"
target = "swiftui"
description = "NoteRelationCountsWire — reply/reaction/repost/zap count projection wire type shared by note-actions-row and any future stats surface."
[[components.files]]
source = "swiftui/note-relation-counts/NoteRelationCountsWire.swift"
target = "Components/NostrNote/NoteRelationCountsWire.swift"
role = "source"

[[components]]
id = "swiftui/note-actions-row"
version = "0.1.0"
target = "swiftui"
description = "Social action bar for a Nostr note — reply, repost, like (NIP-25), and zap (NIP-57). Pure renderer: all actions fire callbacks; host owns dispatch and sheet/dialog presentation."
dependencies = ["swiftui/note-relation-counts"]
[[components.files]]
source = "swiftui/note-actions-row/NostrNoteActionsRow.swift"
target = "Components/NostrNote/NostrNoteActionsRow.swift"
role = "source"
[[components.files]]
source = "swiftui/note-actions-row/Examples/NostrNoteActionsRowPreview.swift"
target = "Components/NostrNote/Examples/NostrNoteActionsRowPreview.swift"
role = "example"
```

### Compose

```toml
[[components]]
id = "compose/note-relation-counts"
version = "0.1.0"
target = "compose"
description = "NoteRelationCountsWire — reply/reaction/repost/zap count projection wire type shared by note-actions-row and any future stats surface."
[[components.files]]
source = "compose/note-relation-counts/NoteRelationCountsWire.kt"
target = "Components/NostrNote/NoteRelationCountsWire.kt"
role = "source"

[[components]]
id = "compose/note-actions-row"
version = "0.1.0"
target = "compose"
description = "Social action bar for a Nostr note — reply, repost, like (NIP-25), and zap (NIP-57). Pure renderer: all actions fire callbacks; host owns dispatch and dialog presentation."
dependencies = ["compose/note-relation-counts"]
[[components.files]]
source = "compose/note-actions-row/NostrNoteActionsRow.kt"
target = "Components/NostrNote/NostrNoteActionsRow.kt"
role = "source"
```

---

## 7. Ordered Extraction Sequence

When F-08 (registry) is ready and this preflight is approved:

0. **§5.4 sign-off** — resolve the `authorLnurl` vs `zapEnabled` decision with
   the owner BEFORE coding; it fixes the public signature (§5.1/§5.2) and a
   zap-visibility rule. Blocks all surface-shaping steps.
1. **B1** — Define `NoteRelationCountsWire` (standalone component). Adds two files
   (Swift + Kotlin), zero Chirp changes.
2. **B2 + B3** (iOS) — Internalize `likeTapped` state; replace `showReply`
   binding with `onReply` callback. One-file change (`NoteRowView.swift`) at
   the call site.
3. **B4** (iOS) — Swap `ChirpColor.accent` for `Color.accentColor`. One-line
   change, zero behavior change (Chirp sets `.tint(ChirpColor.accent)` at root).
4. **B5 + B6** (Android) — Remove `KernelModel` and `ChirpEventCard` params;
   add callbacks.
5. **B7 + B10** (both/Android) — Relocate the zap dialog AND the reply dialog
   to the host (`TimelineScreen` / `ZapSheet.kt`); component fires callbacks
   only. This is the largest Android change — not mechanical.
6. **Android reskin** (Android) — convert text labels → Material icons, fix
   button order (React/Repost), add optimistic-like local state + idempotency +
   animation, drop the `"..."` loading text. This is NEW behavior, gated by B11.
7. **B8** (Android) — Make composable `public`.
8. **B12** (both) — Pin the a11y contract (labels, button roles, shared test
   identifiers) used by the parity tests.
9. **B11** (both) — Add the render/behavior parity golden gate + (if vendored)
   the source-parity drift gate, #2268-style. Must be green before move.
10. **Move + rename**: copy cleaned iOS `NoteActionsRow` to
    `swiftui/note-actions-row/NostrNoteActionsRow.swift`; cleaned Android
    composable to `compose/note-actions-row/NostrNoteActionsRow.kt`. Update
    `registry.swiftui.toml` and `registry.compose.toml`.
11. **Chirp wiring**: replace in-file definitions with `nmp add component`
    installations; update `NoteRowView.swift` and `NoteActions.kt` call sites.
12. **B9 follow-up**: file a separate issue to migrate `ThreadNoteRow` (note:
    that adds zap + counts to the thread view — confirm it's wanted).

Steps 2–3 (iOS) and 4–6 (Android) are parallelisable across worktrees (disjoint
files). Steps 8–11 are sequential. **This is not a mechanical move**: the
Android reskin (step 6), the two dialog relocations (step 5), and the new
optimistic-like behavior are substantive — the parity gate (step 9) is what
keeps the result honest.
