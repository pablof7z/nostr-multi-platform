# NoteActionsRow Gallery Extraction — Preflight Contract

> **Status**: Preflight only. This doc defines the extraction contract so the
> actual work is mechanical and low-risk. Extraction is explicitly post-v1
> (#997, phase:post-v1). Do **not** implement the extraction until F-08
> (registry) and post-v1 action/dispatch stability land.
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

Ordered by dependency: each must be done before the next can ship.

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
depend on app-level kernel handles (doc: `crates/nmp-cli/registry/registry.toml`
component contract: "Components must not import runtime, C ABI/JNI, or kernel
handles directly").

**Fix**: Remove the `model: KernelModel?` parameter. Replace every dispatch
call with a callback:
- `model.react(card.id, "❤")` → `onLike?()`
- `model.repost(card.id, card.authorPubkey)` → `onRepost?()`
- `model.zapNote(...)` → `onZap?()`
- `model.publishNote(content, card.id)` inside reply dialog → replace with
  `onReply?()` (host presents compose screen).

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
The `authorLnurl: String?` parameter controls whether the zap button is
rendered enabled (non-nil) or muted (nil). The callback carries no arguments;
the caller already has the eventId, authorPubkey, and lnurl at the call site.

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

### B9: Decide ThreadNoteRow.swift fate

**Why**: `ThreadNoteRow.swift:109-143` is a manually-maintained copy of the
same button set. After extraction, the thread view should call
`NostrNoteActionsRow` directly (at the cost of accepting the `reply/repost/like`
layout which currently uses `HStack(spacing: 28)` instead of `HStack(spacing: 0)
+ Spacer()`). Alternatively, it can remain divergent with a clear doc comment.

**Recommendation**: thread view adopts the registry component with an optional
`spacing` parameter or by using a modifier at the call site. This is a
follow-up task gated on the extraction landing, not a blocker for the registry
component itself. Document this here so the extraction PR author knows to file
a follow-up issue.

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
    /// Pre-extracted from the author's kind:0 `lud16`/`lud06` keyed
    /// profile sidecar (Rust side). Nil means "no lightning address" →
    /// zap button renders muted/disabled so layout stays stable.
    public let authorLnurl: String?
    /// Relation counts from the kernel projection. Nil = not yet loaded.
    public let counts: NoteRelationCountsWire?

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
    /** Pre-extracted from author kind:0 lud16/lud06 by caller. Null = muted zap. */
    authorLnurl: String? = null,
    /** Relation counts from the kernel projection. Null = not yet loaded. */
    counts: NoteRelationCountsWire? = null,
    onReply: (() -> Unit)? = null,
    onRepost: (() -> Unit)? = null,
    onLike: (() -> Unit)? = null,
    /** Fired only when authorLnurl != null AND this callback is non-null. */
    onZap: (() -> Unit)? = null,
)
```

### 5.3 Cross-platform parity requirements

The following behaviors must be identical between iOS and Android after
extraction (mirrors the identicon parity constraint from the user-avatar
work):

| Behavior | Target |
|----------|--------|
| Button order | Reply, Repost, Like, Zap (left to right) |
| Zap visibility | Hidden (muted icon placeholder) when `authorLnurl == nil/null` OR `onZap == nil/null` |
| Like state | Optimistic UI: icon fills and accent-colors on first tap; tapping again is a no-op (idempotent guard) |
| Counts display | Only shown when `> 0`; omitted when nil/loading or zero |
| Like color | System accent (`.tint`) on iOS; `MaterialTheme.colorScheme.primary` on Compose |
| Haptics | iOS: `UIImpactFeedbackGenerator` in the component (platform capability); Android: optional `HapticFeedback` via `LocalHapticFeedback` |

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

1. **B1** — Define `NoteRelationCountsWire` (standalone component). Adds two files
   (Swift + Kotlin), zero Chirp changes.
2. **B2 + B3** (iOS, parallelisable with B5+B6) — Internalize `likeTapped`
   state; replace `showReply` binding with `onReply` callback. One-file change
   (`NoteRowView.swift`) touches only the call site.
3. **B4** (iOS) — Swap `ChirpColor.accent` for `Color.accentColor`. One-line
   change, zero behavior change (Chirp sets `.tint(ChirpColor.accent)` at root).
4. **B5 + B6** (Android) — Remove `KernelModel` and `ChirpEventCard` params;
   add callbacks. Move `ZapAmountDialog` to `TimelineScreen` (caller).
5. **B7** (both) — Align zap UX: callback-only, host-owned dialog. Android gets
   `ZapSheet.kt` or equivalent at the app layer.
6. **B8** (Android) — Make composable `public`.
7. **Move + rename**: copy cleaned iOS `NoteActionsRow` to the registry source
   tree as `swiftui/note-actions-row/NostrNoteActionsRow.swift`; copy cleaned
   Android composable to `compose/note-actions-row/NostrNoteActionsRow.kt`.
   Update `registry.swiftui.toml` and `registry.compose.toml`.
8. **Chirp wiring**: replace in-file definitions with `nmp add component`
   installations; update `NoteRowView.swift` and `NoteActions.kt` call sites.
9. **B9 follow-up**: file a separate issue to migrate `ThreadNoteRow` to use
   `NostrNoteActionsRow`.

Steps 2–3 and 4–6 are parallelisable across iOS and Android worktrees (disjoint
files). Steps 7–9 are sequential.
