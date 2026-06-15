---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - adr-0055-r6
  - nmp-feed
  - typed-projection-emission
  - chirp-home-feed
supersedes:
  - 2026-06-14-2-option-b-feed-row-deltas-closed
related_claims: []
source_lines:
  - 10469-10487
  - 10560-10593
  - 10640-10706
  - 10719-10754
  - 10794-10823
  - 10867-10906
  - 10915-10941
captured_at: 2026-06-14T20:03:18Z
---

# Episode: Feed byte-equality gating shipped; idle-jank cause refuted; row-deltas closed

## Prior State

Home feed (nmp.feed.home) and two whole-value Tier-1 keys (claimed_event_embeds, nip46_onboarding) were always-Changed — every tick emitted ~41KB feed payload regardless of data change, believed to cause idle timeline jank on a 4Hz snapshot pump

## Trigger

Byte-equality gating implemented and measured (97.6% idle reduction); Release A/B measurement then showed timeline body re-evals were 0 in both ON and OFF arms — .equatable() at HomeFeedView.swift:147 was already the load-bearing shield, not feed omission

## Decision

Feed byte-equality gating is correct and shipped (97.6% idle frame-byte reduction, 45,440B→1,104B; nightly CI-gated); however, feed omission is NOT the idle-jank cause — it is a valuable-but-redundant second layer; Option B (feed row-deltas) NOT warranted (idle already handled by .equatable(), mutating frames must re-render anyway); Rung-6-B ADR stays closed unless release-device data shows mutating-feed frames missing the frame budget

## Consequences

- 97.6% idle frame-byte reduction is real and empirically proven (feed 41,112B→0 on idle ticks, nightly ffi-stress gate guards regression)
- Feed omission prevents @Published fan-out and FFI bytes on idle (battery/bandwidth win)
- OP-centric feed engine does NOT follow-gate roots (only replies are follow-gated); RootFeedSnapshot carries total_blocks so any new root legitimately re-emits
- TypedHomeFeedDecoder still decodes the retained feed each idle tick (residual cost, not yet gated due to doctrine D5 bounded-native-state tension)
- Actual idle-jank cause remains unconfirmed — candidates are scroll-path render cost or Debug build artifact (17.6× slower encode)
- Kernel is NOT a blind 4Hz pump at idle — it ticks only on change (~11s apart at deep idle)

## Open Tail

- On-device Release frame/scroll trace needed to identify actual jank source
- devicectl broken on host Mac (CoreDevice library-validation failure) blocks on-device verification until repaired
- Optional follow-up: gate TypedHomeFeedDecoder.decode on changedKeys (requires resolving D5 doctrine tension)
- Release-device data showing mutating-feed frames missing frame budget would reopen Option B

## Evidence

- transcript lines 10469-10487
- transcript lines 10560-10593
- transcript lines 10640-10706
- transcript lines 10719-10754
- transcript lines 10794-10823
- transcript lines 10867-10906
- transcript lines 10915-10941

