---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - podcast-player
  - snapshot-codec
  - push-path
supersedes: []
related_claims: []
source_lines:
  - 3993-4011
captured_at: 2026-06-12T00:59:06Z
---

# Episode: Podcast-player latent push-path bug — same defect class as NMP #1084

## Prior State

podcast-player's Android `SnapshotCodec.decodeEnvelope` decodes the envelope `v` directly as `PodcastSnapshot`, and `MainActivity`'s steady-state loop replaces the whole snapshot with the result

## Trigger

During the v0.4.0 migration audit, the agent discovered that `v` was never the bare `PodcastUpdate` shape on real wire frames (v0.2.9: whole KernelSnapshot with podcast data nested under `projections["podcast.snapshot"]`; v0.4.0: Tier-3 fields only). All `PodcastSnapshot` fields default, so a push frame can replace the UI snapshot with an empty-library value.

## Decision

Flagged in PR #382 notes but not patched (Android files dirty from another session). Same defect class as NMP #1084 (Android completely dark).

## Consequences

- Push-path frames can silently empty the podcast library UI
- The `SnapshotCodecTest` push fixture encodes an assumption the wire never satisfied — same class as NMP #1084
- Rust-side pull path (`nativePodcastSnapshot`) is safe — only the Kotlin push-path is affected

## Open Tail

- Fix the push-path decoder in podcast-player when the Android files are clean

## Evidence

- transcript lines 3993-4011

