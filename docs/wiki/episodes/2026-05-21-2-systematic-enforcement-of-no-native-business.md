---
type: episode-card
date: 2026-05-21
session: 45fcf96e-5b37-414f-a080-820b74a4e179
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/45fcf96e-5b37-414f-a080-820b74a4e179.jsonl
salience: architecture
status: active
subjects:
  - chirp-swift-violations
  - rust-projections
  - aim-md-§4-§6
  - snapshot-seam
supersedes: []
related_claims: []
source_lines:
  - 475-570
  - 611-664
  - 666-768
  - 771-1161
captured_at: 2026-06-18T04:53:12Z
---

# Episode: Systematic enforcement of no-native-business-logic doctrine across Chirp

## Prior State

Chirp contained 34+ violations of aim.md §4.4 (no Swift if/switch deciding app behavior), §4.5 (no derived state in native), and §6 anti-patterns (duplicated formatting, protocol knowledge in Swift). The canonical bad example (Marmot's ~1400 LOC projection logic) had recurred as 709 LOC in Swift. String comparisons for handshake stages, per-row Swift formatting, polling via DispatchQueue.asyncAfter, and 3-dictionary rebuilds per render were standard practice.

## Trigger

User directive at line 611: 'ANY hack or complex logic in Chirp is EXACTLY the opposite of what we need — we need to make the kernel and nmp crates to make things easier for all apps, not make chirp more complex!!!' This followed the discovery that Marmot's 709 LOC was flagged in AGENTS.md as the canonical mistake to never repeat — yet it shipped again.

## Decision

10 parallel PRs systematically moved business logic from Swift to Rust projections: signer classification (AccountSummary), DM inbox ordering, relay diagnostics, outbox/notifications, Marmot display fields, NIP-29 GroupChat display fields, OnboardingView+NIP46 state machine, ThreadScreen polling/pluralization, ProfileView dispatch + mention_profiles, SettingsHub+SearchView dead code + settings_hub projection. Every PR added Rust-side pre-computed fields consumed by Swift; no new C-ABI symbols were created.

## Consequences

- New Rust projections added: relay_diagnostics, outbox_summary, settings_hub, mention_profiles, nip46_onboarding, plus display fields on MarmotGroupRow, Nip29GroupChatMessage, ProfileAction, ThreadViewPayload, AccountSummary
- LOC reality: most files only had 30–60 LOC of substrate-shaped logic among 200–400 LOC of legitimate SwiftUI layout — only SettingsHub (−31%) and NotificationsView (−65.5%) exceeded the 30% reduction target
- Clock injection pattern converging: 3 of 3 agents independently solved it as 'pass now_secs into projection construction at snapshot time'
- Localization trade-off acknowledged: Rust pre-formatting produces ASCII labels ('5m ago') instead of locale-aware RelativeDateTimeFormatter output — explicit follow-up needed
- Cross-cutting initials-in-Swift pattern identified across 5+ files (GroupChatView, DmListView, ModularBlockView, NoteEntityViews, PublicGroupRow) — project-wide sweep candidate
- mention_profiles projection now live; HomeFeedView and ThreadScreen should consume it as follow-ups
- The `dispatch_action` → `ProfileAction.dispatch: Option<ProfileDispatchSpec>` pattern emerged as the canonical way to replace switch-on-action-kind without new C-ABI symbols
- 9 PRs queued against crates/nmp-core/src/kernel/types.rs — merge order will matter (Task #7 still open)

## Open Tail

- Clock injection canonical pattern (2 agents solved per-projection, 1 punted)
- Cross-cutting initials-in-Swift sweep (5+ files)
- HomeFeed + Thread should adopt mention_profiles projection
- DiagnosticsView non-relay sections still need kernel/perf/metrics/runtime-log seams
- Localization regression from ASCII relative-time labels
- chirp://nip46 callback-scheme constant ownership should move to Rust
- ProfileEditSheet extraction to own file
- ADR-0026 Phase 3 (DM raw-keys migration)

## Evidence

- transcript lines 475-570
- transcript lines 611-664
- transcript lines 666-768
- transcript lines 771-1161

