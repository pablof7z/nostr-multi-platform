---
type: episode-card
date: 2026-06-26
session: ae3e7b5b-75e8-4018-8d1a-ce05f7d4654a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ae3e7b5b-75e8-4018-8d1a-ce05f7d4654a.jsonl
salience: architecture
status: active
subjects:
  - uniffi-migration
  - android-app-loop-binding
  - m14-0
supersedes: []
related_claims: []
source_lines:
  - 182-489
  - 490-490
  - 584-589
  - 618-637
captured_at: 2026-06-26T10:56:47Z
---

# Episode: M14-0 Android app-loop UniFFI migration: feasibility validated after blocker assessment reversal

## Prior State

UniFFI was the strategic long-term direction for M14's write/register binding surfaces (ADR-0030, #2125), but applicability to the Android app-loop lane remained unvalidated due to apparent constraints around callback registration, quiescence coordination, and pointer lifetime.

## Trigger

Exploration agent investigated the app-loop FFI surface and identified apparent blockers—UniFFI's lack of support for callback registration across the FFI boundary, complex quiescence-gate synchronization, pointer lifetime representation, and opaque byte slice semantics—recommending continued use of JNI instead, which risked invalidating the M14-0 plan.

## Decision

The blocker assessment was based on outdated UniFFI knowledge. Modern UniFFI (0.28.3+) natively supports callback trait interfaces (foreign impl), Arc<Self> object lifecycles with constructors/methods, and Vec<u8> byte passing. Codex design gate confirmed no feasibility blocker (AppHandle object facade, UpdateSink callback interface, quiescence via existing Condvar gate). Proceed with M14-0 UniFFI migration for Android app-loop.

## Consequences

- First production slice of M14 post-v1 migration in motion (Sonnet agent implementing in feat/2129-android-uniffi-apploop worktree)
- 9 app-loop JNI entry points deleted; replaced with UniFFI-generated Kotlin bindings from new .udl contract
- KernelBridge.kt refactored from hand-written JNI facade to UniFFI object facade (AppHandle wrapper, UpdateSink callback interface)
- D6/D8 doctrine constraints (dispatch=record/never-throws, push-based/quiescence) now enforceable via UniFFI type system, not JNI best-practices discipline
- 59 files changed: 2675 insertions(+), 3122 deletions (code simplification: JNI deletion offset by generated binding expansion)
- Establishes pattern for remaining M14 lanes post-v1 (lifecycle, binding, capability FFI → UniFFI)
- Validates that UniFFI is suitable for high-throughput callback and quiescence-coordinated patterns in the ecosystem

## Open Tail

*(none)*

## Evidence

- transcript lines 182-489
- transcript lines 490-490
- transcript lines 584-589
- transcript lines 618-637

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-m14-0-android-app-loop-uniffi.json`](transcripts/2026-06-26-1-m14-0-android-app-loop-uniffi.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-m14-0-android-app-loop-uniffi.json`](transcripts/raw/2026-06-26-1-m14-0-android-app-loop-uniffi.json)
