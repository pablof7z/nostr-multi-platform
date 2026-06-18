# Codex Review — Issue #1493 P1: Remove presentation formatting from publish-outbox projection

**PR**: #1568 (`fix/1493-p1-publish-outbox`)
**Date**: 2026-06-18
**Reviewer**: Marcus Webb (agent)
**Status**: All 23 CI checks PASS

---

## Summary

Removed all iOS SF Symbol names, English presentation strings, and formatted labels from the
`nmp-core` kernel's `publish_outbox` and `outbox_summary` projections. This is the headline
violation in issue #1493 (architecture-exception audit, P1 presentation-formatting lane).

The violation: `publish_event_title(kind)`, `publish_event_system_image(kind)`,
`publish_event_preview(kind, content)`, `publish_outbox_status_label(status)`,
`publish_outbox_relay_status_label(status)`, `publish_outbox_attempt_label(attempt)`,
`outbox_summary_title(total)`, and `outbox_summary_subtitle(...)` all lived in `nmp-core`
— the platform-neutral kernel — and pushed iOS SF Symbol strings and English prose onto the
wire format, in direct violation of aim.md §2 and doctrine §4.4.

---

## Changes Made

### Kernel (nmp-core)

**Removed functions** from `crates/nmp-core/src/kernel/publish_outbox.rs`:
- `publish_event_title(kind) -> String` — English kind names
- `publish_event_system_image(kind) -> String` — SF Symbol names
- `publish_event_preview(kind, content) -> String` — prose preview with "[Encrypted]" labels
- `publish_outbox_status_label(status) -> String` — English status text
- `publish_outbox_relay_status_label(status) -> String`
- `publish_outbox_attempt_label(attempt) -> String`
- `outbox_summary_title(total) -> String`
- `outbox_summary_subtitle(total, sending, retrying, queued, failed) -> String`

**Wire type changes** (`crates/nmp-core/src/kernel/types.rs`):
- `PublishOutboxItem`: removed `title`, `preview`, `status_label`, `system_image`; added `content: String`
- `PublishOutboxRelay`: removed `status_label`, `attempt_label`
- `OutboxSummarySnapshot`: removed `title`, `subtitle`

**Identity state** (`crates/nmp-core/src/kernel/identity_state.rs`):
- `PublishQueueEntry`: removed `title: String` field

**Publish engine** (`crates/nmp-core/src/kernel/publish_engine.rs`):
- Removed the two call sites that computed `title: publish_event_title(signed.unsigned.kind)`

### FlatBuffers Wire Format

**Schemas updated**:
- `crates/nmp-core/schema/publish_outbox.fbs`: removed `title`, `preview`, `status_label`, `system_image` from `PublishOutboxItem`; added `content`; removed `status_label`, `attempt_label` from `PublishOutboxRelay`
- `crates/nmp-core/schema/outbox_summary.fbs`: removed `title`, `subtitle` from `OutboxSummarySnapshot`

**Generated Rust bindings** regenerated with flatc 25.12.19.

**Generated Swift bindings** regenerated with flatc 25.12.19 (not hand-edited — a prior attempt
at hand-editing failed the Swift flatc drift CI gate because it omitted trailing spaces in the
`public static var id` return expression that flatc emits).

**Glue code** in `publish_outbox_fb.rs` and `outbox_summary_fb.rs` updated to remove the deleted
fields from encode/decode paths.

**Tests** in `publish_outbox_fb_tests.rs` and `outbox_summary_fb_tests.rs` updated accordingly.

### iOS Shell (Chirp)

`ios/Chirp/Chirp/Bridge/KernelActionTypes.swift`:
- `PublishOutboxItem`: removed `title`, `statusLabel`, `systemImage`, `preview`; added `content: String`
- `PublishOutboxRelay`: removed `statusLabel`, `attemptLabel`
- `OutboxSummary`: removed `title`, `subtitle`

`ios/Chirp/Chirp/Bridge/TypedProjectionGlue.swift`:
- Updated decode paths to use new wire shape

`ios/Chirp/Chirp/Features/NotificationsView+OutboxRow.swift`:
- Added shell-layer computed properties on `PublishOutboxItem`:
  - `var kindTitle: String` — switches on `kind` to produce English label
  - `var iconName: String` — switches on `kind` to produce SF Symbol name (now CORRECTLY in the shell)
  - `var previewText: String` — derives prose from `content` and `kind`
  - `var statusLabel: String`
- Added shell-layer computed properties on `PublishOutboxRelay`:
  - `var statusLabel: String`
  - `var attemptLabel: String`

`ios/Chirp/Chirp/Features/NotificationsView.swift`:
- `summarySection` now computes `summaryTitle`/`summarySubtitle` from raw count fields

`ios/Chirp/ChirpTests/TypedPublishRelayDecoderTests.swift`:
- Tests updated to use new wire API and assert on shell-computed properties

### TUI Shell (chirp-tui)

`apps/chirp/chirp-tui/src/feature_snapshot_typed.rs`:
- Updated `outbox` mapping to compute `title`, `status_label`, `preview` from raw `kind`/`content`/`status`
- Updated `outbox_summary` mapping to compute `title`/`subtitle` from raw counts
- Added private helper functions: `outbox_kind_title`, `outbox_status_label`, `outbox_relay_status_label`, `outbox_preview`, `outbox_summary_title`, `outbox_summary_subtitle`

`apps/chirp/chirp-tui/src/feature_snapshot_json.rs`:
- Updated `outbox_from()` and `relay_lines_from()` for same computation from raw fields
- Added `outbox_summary_from()` (new — previously used generic `summary_from()` which only read `title`/`subtitle` keys that no longer exist on the wire)
- Added `json_outbox_*` helpers

`apps/chirp/chirp-tui/src/feature_snapshot.rs`:
- Updated import and usage to `outbox_summary_from`

---

## Findings

### Clean

1. **Doctrine §4.4 satisfied**: Zero presentation-formatting functions remain in `nmp-core`'s
   projection builders. The D19 doctrine-lint gate (which bans `crate::display::` and
   `format_timestamp` in projection files) continues to pass; the removed functions were more
   subtle violations (English strings embedded in match arms rather than `crate::display::` calls).

2. **Wire backward-compat**: Fields were removed from FlatBuffers tables (which are forward/backward
   compatible for new readers of old messages) but since the projection output is live-computed
   (not persisted), there is no backward-compat concern here.

3. **All drift gates pass**: Swift flatc drift, Rust flatc drift, Kotlin flatc drift, TypeScript
   flatc drift all green. The Kotlin gates pass because no Kotlin bindings existed for these
   schemas (Android uses `action_stages` projection, not `publish_outbox` directly).

4. **Shell symmetry**: iOS and TUI now independently compute the same display strings. No shared
   centralized display table — each shell owns its own presentation language, which is the correct
   architecture (shells can diverge: TUI might use ASCII art, a future Kotlin shell would use its
   own idioms).

5. **`content` field added to wire**: The raw `content: String` from `PublishOutboxItemRow` is now
   propagated to the wire, giving shells the data they need to compute previews. Encrypted kinds
   (4, 44, 1059) are handled at the shell layer with `[Encrypted]` substitution.

6. **`format_relay_reason()` / `format_relay_reasons()` retained**: These format relay-selection
   rationale strings (e.g., "NIP-65 write relay"), which are protocol-level facts, not
   presentation-layer formatting. Correct to keep.

7. **`publish_outbox_status()` retained**: Returns canonical status enum strings used for policy
   decisions (can_retry logic), not English prose for humans. Correct to keep.

### Observations (no action required)

1. The `PublishHistoryLine.title` field on the TUI's `feature_snapshot.rs` (line 150) still has
   the comment "Pre-formatted kind label" — this is a shell-layer struct, so it is correct to
   have a `title` field there. The mapping in `feature_snapshot_typed.rs` (line ~278,
   `title: row.title` for `PublishHistoryLine`) pulls from `PublishQueueEntryRow.title` which
   still exists on the wire via `publish_queue.fbs`. This is the SETTLED history queue (not the
   live outbox) and was not in scope for this PR.

2. The `publish_queue.fbs` schema and `PublishQueueEntryRow.title` field were intentionally left
   unchanged. Future work could apply the same treatment to the settled history projection.

---

## Verdict

**SHIP.** The core violation is resolved. All CI gates pass. The shell layers now own all
presentation decisions. The wire carries raw data only.
