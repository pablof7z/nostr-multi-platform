# NoteActionsRow Contract Blockers

This file holds the detailed blocker rationale for
[note-actions-row-contract.md](note-actions-row-contract.md). It is part of the
design contract, not a parallel roadmap.

## B1: Do not create a shared count wire

The rejected design tried to make a reusable social-count payload for reply,
reaction, repost, and zap numbers. That recreates the relation-bucket problem
at the UI registry boundary.

Fix: each host builds independent action items from the concept-owned read that
knows how to query and interpret that action. The registry component receives
only renderer facts: icon, optional count, loading flag, enabled flag, callback,
and accessibility label.

Acceptance: the extracted component installs without a count-wire dependency,
and no shared reply/reaction/repost/zap payload is introduced.

## B2: Remove app kernel handles from components

Registry components must not depend on app-level kernel handles or generated
app models. User actions leave through callbacks; the embedding app decides
dispatch, navigation, and capability execution.

Acceptance: the component imports no app model, JNI/FFI handle, or kernel
handle type.

## B3: Replace app-specific card inputs

The current Android component accepts a generated app card type. The registry
component receives a list of renderer-only action items instead.

Acceptance: the extracted component imports no app card type.

## B4: Keep reply and zap presentation in the host

The component must not own compose dialogs, zap amount pickers, wallet policy,
or reply dispatch. It raises `onTap`; the host presents sheets/dialogs and
dispatches through the app-wired Rust action.

Acceptance: no reply or zap dialog/sheet is owned inside the registry
component on either platform.

## B5: Pin render and accessibility parity

Extraction must add a failing-on-drift parity gate analogous to the identicon
source/render checks.

Required assertions:

- same logical button set and order for the supplied action list;
- disabled actions do not fire callbacks;
- numeric badges render only when a count is present and non-zero;
- loading state does not introduce visible placeholder text;
- accessibility labels and test identifiers match the supplied action item.

Acceptance: drift in order, enabled state, count visibility, labels, or vendored
source breaks CI.

## B6: Decide focused-thread behavior before broadening actions

The focused-thread row currently renders a smaller action set. Installing the
registry component there may add actions that are not currently visible.

Acceptance: track product behavior in GitHub before changing focused-thread
actions.
