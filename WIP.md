# WIP — Active Work In Flight

> **Live tracker** for work currently on a branch (agent worktrees, in-progress PRs).
> Update this file when you start work, and remove the entry when the PR merges.
>
> Related files:
> - [`docs/BACKLOG.md`](docs/BACKLOG.md) — violations, pending user decisions, ordered v1 feature backlog
> - [`docs/plan.md`](docs/plan.md) — overarching plan (milestones, doctrine, where we are)

## Active

- PR #438 — fix(nmp-core/ios): V-26 — AccountSummary avatarInitials/avatarColorHex to Rust — in progress

## Recent history (verified merged or abandoned as of 2026-05-24)

- 2026-05-24 — fix(nmp-nip29/ios): V-25 — GroupChatView pubkey display to Rust — PR #436 merged
- 2026-05-24 — fix(core/nip29/ios): V-24 — AccountsView + JoinGroupView thin-shell — PR #435 merged
- 2026-05-24 — fix(wallet/ios): V-23 — move WalletView balance/npub formatting to Rust — PR #434 merged
- 2026-05-24 — chore(marmot): delete dead MemberListView cluster — superseded by MarmotGroupRow.members (V-17) — PR #433 merged
- 2026-05-24 — fix(signer-broker): V-13 + V-14 — mio readiness + auto-reconnect with backoff — PR #431 merged
- 2026-05-24 — fix(nip29/ios): move GroupChatMessage.relativeTime to Rust — thin-shell V-22 — PR #432 merged
- 2026-05-24 — refactor(nmp-core): delete dead Kernel::req + ONESHOT_SUB_PREFIX retirement gate (V-04 Stage 4) — PR #430 merged
- 2026-05-24 — fix(marmot/ios): wire member list into group chat toolbar (V-17) — PR #429 merged
- 2026-05-24 — fix(nip17/ios): move dmRelativeTime formatting to Rust — thin-shell V-20 — PR #428 merged
- 2026-05-24 — fix(ios): delete dead SearchView.swift + fix keyring mock race (V-16) — PR #427 merged
- 2026-05-24 — ci(V-15): add real-relay-nightly workflow — commit 41feec14
- 2026-05-24 — fix(publish): fire last_error_toast when FailedAfterRetries settles (V-18) — PR #426 merged
- 2026-05-24 — fix(ios): gate DiagnosticsView behind #if DEBUG (V-19) — PR #425 merged
- 2026-05-24 — refactor(nmp-core): V-04 Stage 2 — migrate 4 bootstrap `self.req()` calls to `InterestRegistry::ensure_sub`; add `Kernel::drain_lifecycle_outbound()` for wasm path — PR #422 merged
- 2026-05-24 — fix(nmp-core): remove kind:9735 zap receipt bootstrap REQ; move to nmp-nip57 ZapReceiptsRuntimeController — PR #421 merged
- 2026-05-24 — fix(chirp): refresh modular timeline every tick so quoted-event cards appear — PR #420 merged
- 2026-05-24 — fix(nmp-core): D0 rename MlsOpHandler → HostOpHandler — PR #419 merged
- 2026-05-24 — fix(nmp-nip02): move FollowListProjection out of Chirp (thin-shell) — PR #418 merged
- 2026-05-24 — fix(nmp-nip29): delete foreign kind constants — PR #417 merged
- 2026-05-24 — fix(nmp-nip59): define KIND_GIFT_WRAP — PR #416 merged
- 2026-05-24 — refactor(nmp-content/nmp-core/nmp-testing): V-12 second batch — PRs #407, #408, #410, #411, #412, #413, #414, #415 merged
- 2026-05-24 — refactor(nmp-core/nmp-codegen): V-12 second batch (swift, ingest, case_a, event_observer, compiler, nip19) — PRs #402-#406, #409 merged
- 2026-05-24 — refactor(nmp-core): V-12 extract relay tests to subfile — PR #397 merged
- 2026-05-24 — feat(nmp-nip02): lift Follow/Unfollow/React ActionModules from nmp-app-chirp — PR #390 merged
- 2026-05-24 — fix(nmp-nip29): remove kind:1111 / NIP-22 wrong crate boundary — PR #393 merged
- 2026-05-24 — refactor(nmp-core): V-12 extract relay_mgmt/dm/publish-state tests — PRs #394, #395, #396 merged
- 2026-05-24 — refactor(nmp-core): V-12 extract raw_event_observer/outbox/zap tests — PRs #398, #399, #401 merged
- 2026-05-24 — refactor(nmp-nostr-lmdb): V-12 extract lib tests — PR #400 merged
- 2026-05-24 — refactor(nmp-core): V-12 extract action_stages/selection/bounded/nip65/publish-action tests — PR #388 merged
- 2026-05-24 — test(nmp-core): snapshot perf CI regression gate — PR #391 merged
- 2026-05-24 — feat(planner): PD-033-C Stage 2 precursor (Case C bootstrap-content #p fallback) — PR #389 merged
- 2026-05-24 — feat(nmp-codegen): F-05 Stage 3 partial (generate Swift TimelineItem) — PR #387 merged
- 2026-05-23 — fix(nmp-content): require bech32 "1" separator in bolt11 regex — committed to master (f68dbf52)
