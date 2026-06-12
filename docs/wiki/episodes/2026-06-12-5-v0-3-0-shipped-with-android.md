---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: product
status: active
subjects:
  - android-dark-fix
  - v0.3.0-skip-advisory
  - typed-frame-wire
supersedes: []
related_claims: []
source_lines:
  - 3846-3920
captured_at: 2026-06-12T00:32:21Z
---

# Episode: v0.3.0 Shipped with Android Completely Dark (V-116)

## Prior State

v0.3.0 introduced a typed-frame wire for snapshot updates, but Android's KernelUpdateFrameDecoder was not rebuilt for it.

## Trigger

Issue #1084 / V-116 identified that Android consumers received zero snapshot updates on v0.3.0 — the decoder emitted nothing because it didn't understand the new wire format.

## Decision

Android KernelUpdateFrameDecoder fully rebuilt from Tier-3 typed fields + sidecars (#1092). Android consumers must skip v0.3.0 and pin v0.4.0 directly. This is the headline item in the v0.4.0 CHANGELOG.

## Consequences

- v0.3.0 is effectively a skip release for Android
- Golden fixture test added to prevent silent regression of Android frame decoding
- v0.4.0 CHANGELOG carries an explicit 'Android consumers must skip v0.3.0' advisory

## Open Tail

*(none)*

## Evidence

- transcript lines 3846-3920

