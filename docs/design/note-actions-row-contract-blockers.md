# NoteActionsRow Contract Blockers

This file holds the detailed blocker rationale for
[note-actions-row-contract.md](note-actions-row-contract.md). It is part of
the design contract, not a parallel roadmap or release tracker.

## B1: Define `NoteRelationCountsWire` as a registry-owned type

`NoteRelationCounts` is Chirp-app-private. A gallery component cannot import
it, so the registry needs its own mirror that any NMP app can reference without
depending on Chirp internals.

Preferred ownership is a small shared component
(`swiftui/note-relation-counts` + `compose/note-relation-counts`) because count
projection can be reused by future action or stats surfaces.

```swift
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

Acceptance: `swiftui/note-relation-counts` and `compose/note-relation-counts`
install without importing Chirp-specific types.

## B2: Internalize `likeTapped` state (iOS only)

`@Binding var likeTapped: Bool` externalizes optimistic animation state to
`NoteRowView`. A registry component should own this ephemeral UI state because
the parent does not need to observe it.

Fix: change it to `@State private var likeTapped = false` inside
`NoteActionsRow` / `NostrNoteActionsRow`, then remove the parent state and
`likeTapped: $likeTapped` call-site argument.

Acceptance: `NoteRowView` no longer declares or passes `likeTapped`, and the
like animation still fires correctly on first tap.

## B3: Replace `showReply` binding with `onReply`

`@Binding var showReply: Bool` causes the component to toggle a parent-owned
flag so the parent's `.sheet` presents `ComposeView`. A gallery component must
raise a pure callback and let the host decide how to present compose.

Fix:

```swift
NoteActionsRow(
    ...,
    onReply: { showReply = true },
    ...
)
```

Acceptance: the row has no `@Binding` parameters, and reply still presents
from `NoteRowView`.

## B4: Replace `ChirpColor.accent` with `Color.accentColor`

`ChirpColor.accent` is app-level theme state. Registry components must not
import app theme types. `Color.accentColor` resolves through the host's tint;
Chirp already sets `.tint(ChirpColor.accent)` at the app root.

Acceptance: the extracted component imports no `ChirpColor` or Chirp theme
type.

## B5: Decouple Android `NoteActionsSummary` from `KernelModel`

The composable directly calls `model.react`, `model.repost`, `model.zapNote`,
and `model.publishNote`. Registry components must not depend on app-level
kernel handles.

The component contract in `docs/cli.md:164-166` says components are pure
renderers and do not fetch, retry, cache, route, or decide policy. The callback
boundary in `docs/cli.md:170-172` says user actions leave components through
callbacks/renderers while the embedding app decides navigation and capability
execution.

This also follows `docs/aim.md` §2 commandment #4: native is rendering plus
capability execution, not domain policy. Doctrine D7 in
`docs/builder-guide/03-doctrine-d0-d8.md` is enforced by
`cargo test -p nmp-testing --test doctrine_lint_smoke`.

Fix mappings:

| Current call | Target |
|--------------|--------|
| `model.react(card.id, "❤")` | `onLike?()` |
| `model.repost(card.id, card.authorPubkey)` | `onRepost?()` |
| `model.zapNote(...)` | `onZap?()` after B7 moves amount UI |
| `model.publishNote(content, card.id)` | `onReply?()` after B10 moves reply UI |

Acceptance: the component imports no `KernelModel`, `SocialActions`, JNI/FFI
handle, or kernel handle type.

## B6: Replace `ChirpEventCard` with scalar inputs

`NoteActionsSummary(card: ChirpEventCard?, ...)` couples the component to a
Chirp-specific generated FlatBuffer type. The registry version accepts
`eventId`, `authorPubkey`, `counts`, and `zapEnabled`.

Acceptance: the extracted file imports no `ChirpEventCard`.

## B7: Unify zap UX contract across platforms

Current platforms disagree:

| Platform | Current behavior |
|----------|------------------|
| iOS | `onZap(eventID, authorPubkey, lnurl)` fires; parent presents `ZapAmountSheet`. |
| Android | Component owns `ZapAmountDialog` and calls `model.zapNote` on confirm. |

The target contract is callback-only. Components must not own the amount
picker or dispatch zaps; amount choice and NWC/wallet interception are app
policy.

```swift
public let zapEnabled: Bool
public var onZap: (() -> Void)?
```

```kotlin
zapEnabled: Boolean = true,
onZap: (() -> Unit)? = null,
```

`zapEnabled` is supplied by the host from Rust-owned or Rust-derived state. The
component does not receive `authorLnurl`; `onZap` carries no LNURL. If
`zapEnabled` is false, zap renders disabled/muted and does not fire. If true,
the host presents amount UI and dispatches `nmp.nip57.zap`; Rust still fails
closed if no LNURL exists at dispatch time.

Android migration: remove `ZapAmountDialog` from `NoteActions.kt` and move it
to app-level UI such as `TimelineScreen` / `ZapSheet.kt`.

Acceptance: no dialog or sheet is owned inside the registry component on
either platform.

## B8: Make `NoteActionsSummary` public

The Android composable is currently `internal`. Registry components must be
public after installation.

Acceptance: the exported function is `public fun NostrNoteActionsRow(...)`.

## B9: Decide `ThreadNoteRow.swift` fate

`ThreadNoteRow.swift:109-143` is a reduced manually-maintained copy with
Reply/Repost/Like only. Adopting the registry component adds zap and counts to
the focused thread row, so it needs a product follow-up when extraction starts.

Acceptance: track the follow-up in GitHub before changing focused-thread
behavior.

## B10: Relocate Android reply dialog to the host

`NoteActions.kt:100-111` owns an inline `ComposeNoteDialog` that calls
`model.publishNote(content, card.id)` on confirm. This is the same violation
class as B7.

Fix: delete `ComposeNoteDialog` from the registry component. The component
fires `onReply?()`. The host owns compose/reply presentation and dispatch,
mirroring iOS `NoteRowView`.

Acceptance: `NoteActions.kt` contains no `ComposeNoteDialog` and no
`model.publishNote` call.

## B11: Render-parity golden gate

The current platforms differ in render style, button order, like behavior,
counts display, zap gating, and dialog ownership. Extraction must add a
failing-on-drift parity gate analogous to the #2268 identicon tests.

Required assertions:

- same logical button set and order;
- same enabled/disabled state from `zapEnabled`;
- same count visibility rule;
- optimistic-like state is idempotent;
- vendored source matches registry source if Chirp keeps installed copies.

Acceptance: drift in button order, button set, enabled state, or vendored
source breaks CI.

## B12: Pin the accessibility contract

iOS already exposes labels and identifiers for Zap, Reply, Repost, and Like.
Android's current `RelationActionLabel` is clickable text without equivalent
semantics.

Target guarantees on both platforms:

| Action | Label | Test identifier |
|--------|-------|-----------------|
| Reply | `Reply` | `note-action-reply` |
| Repost | `Repost` | `note-action-repost` |
| Like | `Like` | `note-action-like` |
| Zap | `Zap` | `note-action-zap` |

Buttons must expose button role/trait (`Button` on SwiftUI, Compose semantics
role and click label on Android).

Acceptance: UI tests can find each action by the same logical identifier on
both platforms.
