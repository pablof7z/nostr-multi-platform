---
type: episode-card
date: 2026-06-10
session: 8db7983d-2852-4213-9b8c-43650a958e7a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/8db7983d-2852-4213-9b8c-43650a958e7a.jsonl
salience: architecture
status: active
subjects:
  - nmp-ffi
  - update-callback
  - memory-safety
  - session-lifecycle
supersedes: []
related_claims: []
source_lines:
  - 764-772
  - 1111-1132
captured_at: 2026-06-11T23:11:53Z
---

# Episode: Update-callback quiescence contract closes UAF on Android and iOS

## Prior State

`nmp_app_set_update_callback` returned immediately after swapping the callback slot. The listener thread could still be executing the foreign callback, racing with ARC deallocation of the callback context (iOS `KernelBridge.deinit`) or `Session::drop` (Android).

## Trigger

Audit identified the UAF: `Session::drop` calls `free_native()` which drops the `Arc<CallbackState>`, but the listener thread may still be inside `on_update` using that same pointer. On iOS, `KernelBridge.deinit` calls `nmp_app_set_update_callback(raw, nil, nil)` before ARC-releasing the sink.

## Decision

Introduced `UpdateCallbackGate` with `Mutex<inner { registration, in_flight: u32 }> + Condvar drained`. Listener increments `in_flight` before invoking foreign code, decrements after. Setter swaps registration then `Condvar::wait_while` until `in_flight == 0`. Option b (condvar drain) was chosen over option a (invoke-under-lock) to avoid deadlock if host callback re-enters the setter.

## Consequences

- Once `nmp_app_set_update_callback` returns, the listener will never touch the old callback again — hard guarantee
- Android `Session::drop` and iOS `KernelBridge.deinit` are now safe without ordering tricks
- Future callbacks that re-enter the setter won't deadlock (unlike invoke-under-lock)

## Open Tail

*(none)*

## Evidence

- transcript lines 764-772
- transcript lines 1111-1132

