---
type: episode-card
date: 2026-05-18
session: d27a4f61-511b-4086-845d-335493f9b464
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/d27a4f61-511b-4086-845d-335493f9b464.jsonl
salience: root-cause
status: active
subjects:
  - actor-command-abi
  - nmp-signer-broker
  - nmp-app-chirp
  - cross-ffi-enum
supersedes: []
related_claims: []
source_lines:
  - 6748-6791
  - 7159-7160
  - 7935-7944
  - 8006-8010
captured_at: 2026-06-18T04:15:45Z
---

# Episode: ActorCommand ABI crash from stale cross-FFI static lib

## Prior State

libnmp_signer_broker.a was built May 18 against an older nmp-core; five new ActorCommand variants were added since then, shifting discriminant numbers. The stale .a was still linked into the iOS binary alongside a current libnmp_app_chirp.a (built May 19)

## Trigger

App crashed immediately on launch with EXC_BAD_ACCESS (KERN_INVALID_ADDRESS 0x800000000000000c). Root cause: broker sends ActorCommand with old discriminant numbers; actor decodes with new discriminants → misinterprets a broker progress message as SignInNsec with garbage String data → str::trim() dereferences invalid pointer

## Decision

All static libs that cross the FFI boundary must be rebuilt from identical nmp-core source. Any ActorCommand variant addition requires rebuilding nmp-signer-broker and nmp-app-chirp in lockstep. Resolved via cargo clean + full rebuild of both libs from same source tree

## Consequences

- Crash eliminated: app survived the startup window that previously killed it within seconds
- Incomplete relay.rs cfg(test) refactoring (gating BOOTSTRAP_DISCOVERY_RELAYS, RelayRole::url() out of production) had to be reverted to restore compilation — that refactoring is blocked until all production call sites are updated
- role_for_relay_url now falls through to relay_edit_rows instead of only matching bootstrap seeds (partial PD-030 reconciliation)
- has_role() promoted to pub(crate) and re-exported through actor/mod.rs for outbox.rs access

## Open Tail

- No automated guard against stale .a files being linked — Xcode project searches both debug and release paths and can pick up an outdated archive
- Xcode debug/release path ambiguity produced duplicate-symbol warnings; needs a single canonical lib search strategy
- Debug overlay on OnboardingView.swift and kbLog.fault diagnostic still need removal after login is confirmed working

## Evidence

- transcript lines 6748-6791
- transcript lines 7159-7160
- transcript lines 7935-7944
- transcript lines 8006-8010

