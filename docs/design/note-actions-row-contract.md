# NoteActionsRow Gallery Extraction — Preflight Contract

> **Status**: Preflight only. This doc defines the extraction contract so the
> actual work is predictable and gated. The two platforms are materially
> divergent today, so extraction is not a pure move: Android needs an icon
> reskin, two dialog relocations, optimistic-like behavior, and a render-parity
> gate. Extraction is explicitly post-v1 (#997, phase:post-v1). Do not implement
> it until F-08 (registry) and post-v1 action/dispatch stability land.
>
> **Zap contract**: the target registry surface uses `zapEnabled: Bool` /
> `zapEnabled: Boolean`. The component does not receive `authorLnurl`, parse
> LNURL metadata, or pass LNURL through the zap callback.
>
> **Related**: GitHub issue #997; blocker details:
> [note-actions-row-contract-blockers.md](note-actions-row-contract-blockers.md).

---

## 1. Current Location (per platform)

| Platform | File | Symbol | Lines |
|----------|------|--------|-------|
| iOS/SwiftUI | `apps/chirp/ios/Chirp/Components/NoteRowView.swift` | `NoteActionsRow` | 285-408 |
| Android/Compose | `apps/chirp/android/app/src/main/java/org/nmp/android/ui/NoteActions.kt` | `NoteActionsSummary` | 1-212 |
| iOS divergent copy | `apps/chirp/ios/Chirp/Components/ThreadNoteRow.swift` | inline action `HStack` | 109-143 |

`ThreadNoteRow.swift` manually repeats the same icon/order contract as
`NoteActionsRow`, but omits zap and counts. Migrating it later is therefore a
product behavior addition, not just deduplication.

---

## 2. Current Contract (as-is)

### 2.1 iOS — `NoteActionsRow`

```swift
struct NoteActionsRow: View {
    let item: NoteRowModel
    let authorLnurl: String?
    let relationCounts: NoteRelationCounts?

    let onLike: (String) -> Void
    var onRepost: ((String, String) -> Void)? = nil
    var onZap: ((String, String, String) -> Void)? = nil

    @Binding var likeTapped: Bool
    @Binding var showReply: Bool
}
```

`NoteRelationCounts` is Chirp-app-private and mirrors the kernel projection:

```swift
struct NoteRelationCounts: Decodable, Equatable, Sendable {
    let replies: RelationCount
    let reactions: RelationCount
    let reposts: RelationCount
    let zaps: RelationCount
}
```

### 2.2 Android — `NoteActionsSummary`

```kotlin
@Composable
internal fun NoteActionsSummary(card: ChirpEventCard?, model: KernelModel?)
```

The current composable reads `card.relationCounts`, dispatches through
`model.react`, `model.repost`, `model.zapNote`, and `model.publishNote`, and
owns local reply/zap dialogs.

### 2.3 Current dispatch path

| Action | iOS symbol | Android symbol | ActionModule key |
|--------|------------|----------------|------------------|
| Like / React | `kernel.react(targetEventID:reaction:)` | `model.react(card.id, "❤")` | `nmp.nip25.react` |
| Repost | `kernel.repost(eventID:authorPubkey:)` | `model.repost(card.id, card.authorPubkey)` | `nmp.nip18.repost` |
| Zap | `kernel.zap(...)` | `model.zapNote(...)` | `nmp.nip57.zap` |
| Reply | reply sheet, compose path | `model.publishNote(content, card.id)` | `nmp.nip01.publish_note` |

iOS callbacks are wired in `HomeFeedView.swift:131-141`; kernel calls go
through `KernelModel+Commands.swift:91-98` and exit through the byte doorway.

---

## 3. Display-Separation Check

Current iOS is display-clean but zap-shape coupled: the parent resolves
`model.profileCard(forPubkey:)?.lnurl` before injection, and the row receives
only `item.authorPubkey` plus `authorLnurl`. The target contract removes that
LNURL from the component boundary in favor of host-supplied `zapEnabled`.

Current Android does not resolve profile display data inside the composable.
Its extraction blockers are structural coupling to `KernelModel` and
`ChirpEventCard`, plus dialog ownership.

Counts already arrive from the Rust FlatBuffer projection
(`nmp_nip01_NoteRelationCounts`); no display resolution belongs inside the
component.

---

## 4. Coupling / Blocker Checklist

The extraction has twelve blockers. B1-B8 are decouplings, B10-B12 close the
visual/behavior/a11y gaps surfaced by parity review, and B9 is a follow-up.
Stable details are in
[note-actions-row-contract-blockers.md](note-actions-row-contract-blockers.md).

| ID | Area | Required outcome |
|----|------|------------------|
| B1 | counts wire | Define registry-owned `NoteRelationCountsWire`; no Chirp-private count type in the component. |
| B2 | iOS state | Internalize `likeTapped`; no optimistic animation binding leaks to parent. |
| B3 | iOS reply | Replace `showReply` binding with `onReply`; host presents compose. |
| B4 | iOS theme | Replace `ChirpColor.accent` with `Color.accentColor`. |
| B5 | Android dispatch | Remove `KernelModel`; actions leave via callbacks only. |
| B6 | Android inputs | Replace `ChirpEventCard` with scalar inputs plus counts wire. |
| B7 | Zap UX | Component fires `onZap`; host owns amount UI and dispatch. |
| B8 | Android visibility | Export public `NostrNoteActionsRow`. |
| B9 | Thread row | File a follow-up before adding zap/counts to thread view. |
| B10 | Android reply UX | Move reply dialog and `model.publishNote` to the host. |
| B11 | parity | Add render/behavior golden gate and source-parity gate where vendored. |
| B12 | a11y | Pin shared labels, button roles, and test identifiers. |

---

## 5. Target Registry Contract

### 5.1 SwiftUI — `swiftui/note-actions-row`

Registry id: `swiftui/note-actions-row`; version: `0.1.0`; dependencies:
none, or `swiftui/note-relation-counts` if counts split out.

```swift
/// Social action bar for a Nostr note: reply / repost / like / zap.
///
/// Pure renderer: dispatches nothing itself. All actions surface through
/// callbacks; the host app owns dispatch and sheet/dialog presentation.
public struct NostrNoteActionsRow: View {
    public let eventID: String
    public let authorPubkey: String
    public let counts: NoteRelationCountsWire?

    /// Host/Rust-derived zapability. The component does not inspect LNURL.
    public let zapEnabled: Bool

    public var onReply: (() -> Void)?
    public var onRepost: (() -> Void)?
    public var onLike: (() -> Void)?
    /// Fired only when `zapEnabled == true` and this closure is non-nil.
    public var onZap: (() -> Void)?
}
```

### 5.2 Compose — `compose/note-actions-row`

Registry id: `compose/note-actions-row`; version: `0.1.0`; dependencies:
none, or `compose/note-relation-counts` if counts split out.

```kotlin
/**
 * Social action bar for a Nostr note: reply / repost / like / zap.
 *
 * Pure renderer: dispatches nothing itself. All actions surface through
 * callbacks; the host app owns dispatch and dialog presentation.
 */
@Composable
public fun NostrNoteActionsRow(
    eventId: String,
    authorPubkey: String,
    counts: NoteRelationCountsWire? = null,
    /** Host/Rust-derived zapability. The component does not inspect LNURL. */
    zapEnabled: Boolean = true,
    onReply: (() -> Unit)? = null,
    onRepost: (() -> Unit)? = null,
    onLike: (() -> Unit)? = null,
    /** Fired only when zapEnabled and this callback is non-null. */
    onZap: (() -> Unit)? = null,
)
```

### 5.3 Cross-platform parity — current vs target

| Behavior | iOS current | Android current | Target |
|----------|-------------|-----------------|--------|
| Render style | SF Symbol icons | Plain text labels | Platform icons |
| Button order | Reply, Repost, Like, Zap | Reply, React, Repost, Zap | Reply, Repost, Like, Zap |
| Like term | Like | React | One canonical Like action |
| Optimistic like | fill, accent, spring, idempotent | dispatches every tap | fill/accent first-tap behavior on both |
| Counts display | only when `> 0` | includes `"..."` loading text | only when `> 0`; no loading text |
| Haptics | iOS haptic | none | iOS keeps; Android optional |
| Zap gating | LNURL-derived shell gate | shown, Rust fails closed | `zapEnabled` host/Rust-derived gate |
| Zap amount UX | host sheet | component dialog | host-owned |
| Reply UX | host sheet | component dialog | host-owned |

Render parity must be locked by a golden test analogous to the #2268 identicon
gate.

### 5.4 Zap Enablement Contract

The target surface is `zapEnabled: Bool` / `zapEnabled: Boolean`, supplied by
the host from Rust-owned or Rust-derived state. The component does not accept
`authorLnurl`, parse `lud06`/`lud16`, or decide zapability from profile
metadata.

When `zapEnabled == true`, the zap button is enabled and `onZap` may fire. The
host owns the amount picker and dispatches `nmp.nip57.zap`; Rust still fails
closed if no LNURL exists at dispatch time. When `zapEnabled == false`, the
button renders disabled/muted and does not fire `onZap`.

This keeps LNURL out of both native shells and removes the current iOS
`profileCard.lnurl` lookup from the component call path.

---

## 6. Registry Manifest Entries

When extraction lands, these manifest blocks go into
`crates/nmp-cli/registry/registry.swiftui.toml` and
`crates/nmp-cli/registry/registry.compose.toml`.

### SwiftUI

```toml
[[components]]
id = "swiftui/note-relation-counts"
version = "0.1.0"
target = "swiftui"
description = "NoteRelationCountsWire for reply/reaction/repost/zap counts."
[[components.files]]
source = "swiftui/note-relation-counts/NoteRelationCountsWire.swift"
target = "Components/NostrNote/NoteRelationCountsWire.swift"
role = "source"

[[components]]
id = "swiftui/note-actions-row"
version = "0.1.0"
target = "swiftui"
description = "Pure social action bar for reply, repost, like, and zap."
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
description = "NoteRelationCountsWire for reply/reaction/repost/zap counts."
[[components.files]]
source = "compose/note-relation-counts/NoteRelationCountsWire.kt"
target = "Components/NostrNote/NoteRelationCountsWire.kt"
role = "source"

[[components]]
id = "compose/note-actions-row"
version = "0.1.0"
target = "compose"
description = "Pure social action bar for reply, repost, like, and zap."
dependencies = ["compose/note-relation-counts"]
[[components.files]]
source = "compose/note-actions-row/NostrNoteActionsRow.kt"
target = "Components/NostrNote/NostrNoteActionsRow.kt"
role = "source"
```

---
## 7. Ordered Extraction Sequence

When F-08 (registry) is ready and this preflight is approved:

1. Define `NoteRelationCountsWire` as standalone Swift and Kotlin component
   files.
2. On iOS, internalize `likeTapped`, replace `showReply` with `onReply`, and
   use `Color.accentColor`.
3. On Android, remove `KernelModel`/`ChirpEventCard`, add scalar inputs and
   callbacks, and move zap/reply dialogs to the host.
4. Reskin Android from text labels to Material icons, fix button order, add
   optimistic-like local behavior, and drop loading text.
5. Make the Android composable public.
6. Pin the a11y contract and add render/behavior parity tests.
7. Move cleaned implementations to the registry ids in §6 and install them
   back into Chirp.
8. File the ThreadNoteRow follow-up if the product wants zap and counts in
   focused thread rows.

Steps 2 and 3 can happen in parallel in separate worktrees. Steps 6 and 7 are
sequential because the parity gate must cover the final installed component.
