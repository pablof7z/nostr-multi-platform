# WIP — Active Work In Flight

> **Live tracker** for work currently on a branch (agent worktrees, in-progress PRs).
> Update this file when you start work, and remove the entry when the PR merges.
>
> Related files:
> - [`docs/BACKLOG.md`](docs/BACKLOG.md) — violations, pending user decisions, ordered v1 feature backlog
> - [`docs/plan.md`](docs/plan.md) — overarching plan (milestones, doctrine, where we are)

## Architecture migration ladder

The crate-boundary spec lives at
`docs/architecture/crate-boundaries.md` (2026-05-24); migration runs in the
12-step order from §5. As of end-of-day 2026-05-24 master:

| Step | State |
|---|---|
| 1 (substrate seams: IngestParser, ProtocolCommand, OutboxRouter, MailboxCache) | ✅ merged |
| 2 (`nmp-router` crate) | ✅ merged |
| 3 (kernel cut-over to `Arc<dyn OutboxRouter>` + absorb `nmp-nip65`) | ✅ merged |
| 4 (V-41 LNURL → `nmp-nip57`) | ✅ merged |
| 5 (V-39 DM send → `nmp-nip17`) | ✅ merged |
| 6 (V-40 kind:10050 + `DmRelayCache` → `nmp-nip17`) | ✅ merged |
| 7 (V-38 NWC → `nmp-nip47`) | 🟡 PR #460 open, deprioritized |
| 8 phase A (`nmp-network` extraction) | ✅ merged |
| 8 phases B/C/D/E (Pool API redesign, `BrowserRelayDriver` move, broker dedupe, NIP-42 split) | ❌ not started |
| 9 (`nmp-store` + `nmp-planner` extraction) | ✅ merged |
| 10 (`nmp-app-template`, V-48) | ❌ not started |
| 11 partial (chirp-* + `nmp-chirp-config` → `apps/chirp/`) | ✅ merged; `fixture-todo-core` deferred on codegen path hardcode; `nmp-ffi` extraction not started |
| 12 (return `nmp-marmot` from `apps/` to `crates/`) | ❌ not started |

Adjacent: **V-51 routing observability** — phases 1 (substrate observer + ring buffer), 4 (validation harness against pablof7z's real NIP-65), 5 (kernel-router observability cut-over) ✅ merged. Phases 2 (FFI/wasm snapshot surface) + 3 (Chirp inspector UI) not started.

## Active

- 2026-05-24 — refactor(nmp-core/nmp-router/nmp-app-chirp): "make substrate honest" — router becomes decision authority (not observe-only), delete `nmp-core::substrate::default_routing.rs`, eliminate `ProtocolCommandContext` 11-accessor balloon (capability traits), fix 14 `expect("RwLock poisoned")` panics. Branch `worktree-agent-ac01eccb4cdd13a99`.
- 2026-05-24 — docs(plan): reconcile WIP.md / BACKLOG.md / plan.md / crate-boundaries.md after today's 16 merges — branch `worktree-docs-reconcile-plan-files` (this branch)

## Recent history (verified merged or abandoned as of 2026-05-24)

- 2026-05-24 — refactor(nmp-core/nmp-store/nmp-planner): step 9 — extract `nmp-store` and `nmp-planner` — PR #463 merged (8a3cd62b)
- 2026-05-24 — feat(nmp-core/nmp-app-chirp): V-51 phase 5 — kernel calls injected `OutboxRouter`; chirp wires `GenericOutboxRouter` — PR #462 merged (1dbff579)
- 2026-05-24 — feat(nmp-core/chirp-repl/nmp-testing): V-51 phase 4 — routing-trace validation harness (real-pubkey integration test PASSES against pablof7z's NIP-65) — PR #461 merged (b9e0fc15)
- 2026-05-24 — feat(nmp-network): step 8 phase A — extract relay worker + protocol primitives to new `nmp-network` crate — PR #459 merged (1342912f)
- 2026-05-24 — refactor(nmp-nip17/nmp-core): V-39 + V-40 — full NIP-17 DM stack migration → `nmp-nip17` — PR #458 merged (852750b2)
- 2026-05-24 — feat(nmp-core/nmp-router): V-51 phase 1 — `RoutingTraceObserver` substrate seam + bounded ring-buffer projection — PR #457 merged (efe72537)
- 2026-05-24 — refactor(nmp-nip57): step 4 / V-41 — move LNURL fetcher onto `ProtocolCommand`; delete `ActorCommand::FetchLnurlInvoice` variant — PR #456 merged (c9fc728f)
- 2026-05-24 — docs(agents): scope cargo test to touched crates; supervisor runs --workspace at merge — PR #455 merged (8f74ab50)
- 2026-05-24 — refactor(nmp-core/nmp-router): step 3 — kernel cutover to `Arc<dyn OutboxRouter>` + absorb nmp-nip65 — PR #454 merged (c565f7c4)
- 2026-05-24 — refactor(nip01/nip29/core): V-12 — extract group_chat / timeline_projection / identity_state tests — PR #453 merged (f4e6609c)
- 2026-05-24 — docs(backlog): V-51 — routing-decision observability + Chirp peek-under-hood UI — PR #452 merged (64f3a297)
- 2026-05-24 — refactor(workspace): step 11 — move app-specific crates out of crates/ — PR #451 merged (56e8a6bf)
- 2026-05-24 — feat(nmp-router): step 2 — new crate with `InMemoryMailboxCache` + `Kind10002Parser` + `GenericOutboxRouter` — PR #450 merged (f441939f)
- 2026-05-24 — refactor(nmp-core): step 1.c + 1.d — `OutboxRouter` + substrate `MailboxCache` traits — PR #449 merged (767c1152)
- 2026-05-24 — refactor(nmp-core): step 1.b — `ProtocolCommand` + `ActorCommand::Protocol(...)` seam — PR #448 merged (dd231e4d)
- 2026-05-24 — refactor(nmp-core): step 1.a — `IngestParser` + `EventIngestDispatcher` substrate seam — PR #447 merged (b2867008)

- 2026-05-24 — fix(nmp-nip01): V-34 — avatar initials from display name, not hex pubkey — PR #445 merged
- 2026-05-24 — refactor(nmp-core): V-33 — canonical display helpers in nmp-core::display; delete 5 copies — PR #444 merged
- 2026-05-24 — fix(nmp-core): unify avatar_color to djb2 — all surfaces consistent — committed 70ede645 to master
- 2026-05-24 — fix(nmp-nip01/ios): V-32 — add authorPictureUrl/contentPreview to TimelineEventCard; delete Swift computation — PR #443 merged
- 2026-05-24 — fix(nmp-core/ios): V-31 — mention_profiles covers all visible views; delete Swift profile dict construction — PR #442 merged
- 2026-05-24 — fix(nmp-nip29/ios): V-29 — GroupChatSnapshot.group_initials; delete Swift initials computed prop — PR #441 merged
- 2026-05-24 — fix(ios): V-30 — remove Swift pubkey truncation from ModularBlockView.displayName — PR #440 merged
- 2026-05-24 — fix(nmp-core/nmp-nip01/ios): V-28 — shortPubkey/shortID/relativeTime display strings to Rust — PR #439 merged
- 2026-05-24 — fix(nmp-nip01/ios): V-27 — ChirpEventCard display fields to Rust — PR #437 merged
- 2026-05-24 — fix(nmp-core/ios): V-26 — AccountSummary avatarInitials/avatarColorHex to Rust — PR #438 merged
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
