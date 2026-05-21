# Opus direction review #46 — kind:10050 landed inert, ADR-0026 still deferred

Date: 2026-05-21. PRs covered: #143 merged, #151 merged, #154 + #157 pending.

## Headline

**PR #151 reproduced the inert-seam pathology within one cycle of #45 naming
it.** `apps/chirp/nmp-app-chirp/src/ffi.rs:577` registers
`publish_dm_relay_list_command`; grep across `ios/Chirp/` finds zero callers of
`nmp.dm.publish_relay_list`. Recent NIP-17 PRs (#125, #134, #151) all left the
consumer for next time. Discipline failure, not noise.

## Q1 — kind:10050 publish completeness

- Confirmed gap. `DmBridge.swift` has `sendDm` but no `publishDmRelayList`.
  `KernelModel.swift:273-274` (`addRelay`/`removeRelay`) never dispatches.
- Forcing function: **implicit + automatic**, not a Settings toggle.
  (a) Onboarding completion + every account switch. (b) Every `addRelay` /
  `removeRelay` against a `write`/`both` URL. One surface drives both kinds.
- A separate "DM inbox relays" UI is worse — another surface to drift.

## Q2 — push back on `unsupported: bool` on `DmInboxSnapshot`

`inbox.rs:174` already argues against widening it ("serde-stable wire type …
widening is a breaking schema change"). Correct home is the **identity layer**:
one `supports_nip44: bool` on the kernel snapshot, consumed by every NIP-44
feature. One field, N consumers. Coupling to the DM projection forces every
next NIP-44 feature to widen its own.

## Q3 — ADR-0026: still defer, same reasons

Nothing structural changed. `nip44_encrypt`/`nip44_decrypt` live only in
`remote_signer_tests.rs`; `actor/commands/dm.rs:87` still calls
`active_local_keys()`. `PendingDmSend` still needs two `ActorCommand` variants
+ async refactor of `unwrap_gift_wrap`. ADR-0026 is **two reviews old as
built-but-inert** — that fact is the lesson, not the schedule.

## Q4 — highest-ROI next PR: complete PR #151

- `ios/Chirp/Chirp/Bridge/DmBridge.swift` — add
  `publishDmRelayList(relays:)` mirroring `sendDm` (lines 67–93).
- `ios/Chirp/Chirp/Bridge/KernelModel.swift:273-274` — auto-dispatch after
  every `addRelay`/`removeRelay` against write-eligible URLs.
- Onboarding completion path — dispatch once after relay defaults populate.

Converts the action to live in the same cycle it landed and breaks the
inert-seam streak. A new feature here entrenches it.
