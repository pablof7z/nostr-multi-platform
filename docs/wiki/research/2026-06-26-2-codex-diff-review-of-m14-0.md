---
type: research-record
date: 2026-06-26
session: ae3e7b5b-75e8-4018-8d1a-ce05f7d4654a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ae3e7b5b-75e8-4018-8d1a-ce05f7d4654a.jsonl
source_lines: 1025-1027
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Codex diff review of M14-0 implementation against architectural criteria; verdict 4 blockers and 2 should-fix issues identified"
captured_at: 2026-06-26T11:42:42Z
---

Codex diff review of M14-0 implementation against architectural criteria; verdict 4 blockers and 2 should-fix issues identified

---

Findings

- BLOCKER [apps/chirp/android/app/src/main/java/org/nmp/android/KernelBridge.kt:153](/private/tmp/worktrees/nmp-2129-android-uniffi-apploop/apps/chirp/android/app/src/main/java/org/nmp/android/KernelBridge.kt:153) and [apps/chirp/crates/nmp-chirp-android-ffi/src/uniffi_app_loop.rs:192](/private/tmp/worktrees/nmp-2129-android-uniffi-apploop/apps/chirp/crates/nmp-chirp-android-ffi/src/uniffi_app_loop.rs:192) - The migrated app-loop still exposes and uses JSON dispatch (`dispatchIntentJson` / `dispatchActionJson`) beside the byte doorway. The review contract says UniFFI owns lifecycle/callback shape while `NMPD DispatchEnvelope` bytes remain the dispatch payload and Kotlin forwards bytes only. This also violates the no-compat-shims rule: the code explicitly labels these APIs “TRANSITIONAL” at lines 190 and 206. Fix by deleting the JSON UniFFI dispatch methods and routing Android through `AppHandle
