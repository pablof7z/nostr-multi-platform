# NoteActionsRow Gallery Extraction - Preflight Contract

> **Status**: Preflight only. Updated by #2508: action rows must not depend on
> any shared social-count wire, feed-card metric bundle, or reusable relation
> bucket. Extraction remains post-v1 (#997, phase:post-v1).
>
> **Related**: GitHub issue #997; blocker details:
> [note-actions-row-contract-blockers.md](note-actions-row-contract-blockers.md).

## Current Location

| Platform | File | Symbol |
|----------|------|--------|
| iOS/SwiftUI | `apps/chirp/ios/Chirp/Components/NoteRowView.swift` | `NoteActionsRow` |
| Android/Compose | `apps/chirp/android/app/src/main/java/org/nmp/android/ui/NoteActions.kt` | `NoteActionsSummary` |
| iOS divergent copy | `apps/chirp/ios/Chirp/Components/ThreadNoteRow.swift` | inline action `HStack` |

## Ownership Rule

The action row is a renderer. It does not fetch, classify tags, inspect Nostr
kinds, aggregate social facts, or own protocol policy.

The host composition builds each visible action from the concept owner that
knows that action:

- replies/comments: the replies owner opens the target-specific reply read;
- reactions: `nmp-nip25` opens the target-specific reaction read;
- reposts: `nmp-nip18` opens the target-specific repost read;
- zaps: `nmp-nip57` opens the target-specific zap read when zap support is
  installed;
- bookmarks, mutes, pins, or app-specific markers: the crate or app module that
  defines that concept owns the read.

Those reads may expose counts, loading state, and teardown for their own
concept. There is no cross-protocol summary object and no registry count-wire
component.

## Target Component Shape

The extracted component receives already-composed action items. It renders
icons, optional numeric badges, enabled state, and accessibility metadata, then
raises callbacks.

SwiftUI sketch:

```swift
public struct NostrNoteActionItem: Identifiable, Equatable {
    public let id: String
    public let icon: NostrNoteActionIcon
    public let accessibilityLabel: String
    public let count: UInt64?
    public let isLoading: Bool
    public let isEnabled: Bool
    public let onTap: (() -> Void)?
}

public struct NostrNoteActionsRow: View {
    public let actions: [NostrNoteActionItem]
}
```

Compose sketch:

```kotlin
data class NostrNoteActionItem(
    val id: String,
    val icon: NostrNoteActionIcon,
    val accessibilityLabel: String,
    val count: ULong? = null,
    val isLoading: Boolean = false,
    val isEnabled: Boolean = true,
    val onTap: (() -> Unit)? = null,
)

@Composable
fun NostrNoteActionsRow(actions: List<NostrNoteActionItem>)
```

The component may ship default icon presets for common actions, but those are
visual presets only. They are not a protocol ownership surface.

## Zap Enablement

Zap availability is host supplied. The component does not receive `authorLnurl`,
parse `lud06`/`lud16`, or decide zapability from profile metadata.

When zaps are enabled in a host composition, the host owns amount selection and
dispatches the app-wired zap action. Rust still fails closed if no LNURL exists
at dispatch time. In v1 defaults, zap publishing remains disabled because the
default composition does not register a zap publisher.

## Extraction Gates

Before extraction lands:

1. Remove component dependencies on app kernel handles.
2. Replace app-specific card inputs with renderer-only action items.
3. Keep reply and zap dialogs/sheets in the host, not the component.
4. Pin button order, enabled state, count visibility, and accessibility labels
   with render/parity tests.
5. Decide focused-thread behavior before adding actions that are not currently
   rendered there.
