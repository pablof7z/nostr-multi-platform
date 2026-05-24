# WIP — Active Work In Flight

> **Live tracker** for work currently on a branch (agent worktrees, in-progress PRs).
> Update this file when you start work, and remove the entry when the PR merges.
>
> Related files:
> - [`docs/BACKLOG.md`](docs/BACKLOG.md) — violations, pending user decisions, ordered v1 feature backlog
> - [`docs/plan.md`](docs/plan.md) — overarching plan (milestones, doctrine, where we are)

## Architecture migration ladder

The crate-boundary spec is committed
(`docs/architecture/crate-boundaries.md`, 2026-05-24); migration runs in the
12-step order from §5. Steps 1 (substrate seams), 2 (`nmp-router`), 3
(kernel cut-over), and 4 (LNURL onto `ProtocolCommand` — V-41) have all
landed on master. V-39+V-40 (NIP-17 DM stack) is in PR #458.

## Active

- 2026-05-24 — refactor(nmp-core/nmp-store/nmp-planner): step 9 — extract `nmp-store` (EventStore + LMDB/Mem backends + DomainMigration value types) and `nmp-planner` (LogicalInterest + subscription compiler + stable_hash). `nmp-core` re-exports `store::*`, `planner::*`, `stable_hash::*`, and `substrate::{DomainMigration, MigrationTx}` so existing import sites compile unchanged. Router signature decision: `LogicalInterest` is re-exported through `nmp_core::planner::LogicalInterest` (which forwards to `nmp_planner`), so `OutboxRouter::route_subscription(&LogicalInterest, ...)` is byte-identical post-extraction — zero call site churn at the router boundary. Planner-internal `MailboxCache` and substrate `MailboxCache` continue to cohabit (V-40 follow-up). Branch `feat/step9-extract-store-and-planner`.
- 2026-05-24 — refactor(nmp-core/nmp-nip17): V-39+V-40 — full NIP-17 DM stack migration → nmp-nip17 — PR #458 (rebased onto master)
- 2026-05-24 — feat(nmp-core/chirp-repl/nmp-testing): V-51 phase 4 — routing-trace validation harness. Widens `Kernel::routing_trace()` to `pub`, threads an `Arc<RoutingTraceProjection>` slot through `NmpApp`, adds `chirp-repl routing-trace` subcommand + a `#[ignore]`'d real-relay integration test (`routing_trace_real_nostr`) that fetches pablof7z's live kind:10002 and asserts `Nip65/Read` lane attribution with no `AppRelay/Fallback` leak. Companion shell smoke `scripts/validate-routing.sh`. Branch `feat/v51-phase4-validation-harness`.
- 2026-05-24 — feat(nmp-core/nmp-app-chirp): V-51 phase 5 — kernel-router wire-up. Widens `Kernel::set_routing` from `pub(crate)` to `pub`, adds an `observe_subscription_through_router` helper that fires the injected `OutboxRouter` for the per-author REQ-construction sites (`author_requests`, `firehose_requests`, `profile_claim_request`, `pending_profile_claim_requests`) AND on kind:10002 ingest so the trace projection populates with live decisions. Adds a `RoutingSubstrateFactory` slot on `NmpApp` (`set_routing_substrate`) the actor reads after kernel construction to install `nmp_router::GenericOutboxRouter` + `nmp_router::InMemoryMailboxCache` (the production swap is now live in `nmp_app_chirp_register`). The `validate-routing.sh` smoke now PASSes against pablof7z's real kind:10002 with `Nip65/Read` lane attribution. Branch `feat/v51-kernel-router-wireup`.

## Recent history (verified merged or abandoned as of 2026-05-24)

- 2026-05-24 — refactor(nmp-nip57): step 4 / V-41 — move LNURL fetcher onto `ProtocolCommand`; delete `ActorCommand::FetchLnurlInvoice` variant — PR #456 merged
- 2026-05-24 — refactor(nmp-core/nmp-router): step 3 — kernel cutover to `Arc<dyn OutboxRouter>` + absorb nmp-nip65 — PR #454 merged
- 2026-05-24 — feat(nmp-router): step 2 — new crate with `InMemoryMailboxCache` + `Kind10002Parser` + `GenericOutboxRouter` — PR #450 merged
- 2026-05-24 — refactor(nmp-core): step 1.c + 1.d — `OutboxRouter` + substrate `MailboxCache` traits — PR #449 merged
- 2026-05-24 — refactor(nmp-core): step 1.b — `ProtocolCommand` + `ActorCommand::Protocol(...)` seam — PR #448 merged
- 2026-05-24 — refactor(nmp-core): step 1.a — `IngestParser` + `EventIngestDispatcher` substrate seam — PR #447 merged
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
