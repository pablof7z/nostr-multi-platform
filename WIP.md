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
| 3 (kernel cut-over to `Arc<dyn OutboxRouter>` + absorb `nmp-nip65`) | ✅ merged; spec §271 follow-up (`Nip65OutboxResolver` → `nmp-router`) — PR #484 open |
| 4 (V-41 LNURL → `nmp-nip57`) | ✅ merged |
| 5 (V-39 DM send → `nmp-nip17`) | ✅ merged |
| 6 (V-40 kind:10050 + `DmRelayCache` → `nmp-nip17`) | ✅ merged |
| 7 (V-38 NWC → `nmp-nip47`) | 🟡 PR #460 open, deprioritized |
| 8 phase A (`nmp-network` extraction) | ✅ merged |
| 8 phase B (push-model `Pool` API redesign) | ✅ merged |
| 8 phase C (`BrowserRelayDriver` move into `nmp-network`) | ✅ merged |
| 8 phase D (broker dedupe on Pool) | 🟡 PR #477 in flight |
| 8 phase E (NIP-42 wire/FSM split — `RelayFrame::Auth`) | ✅ merged |
| 8 phase F (kernel-actor cut-over to `Pool`) | ⏳ in flight (this branch) |
| 9 (`nmp-store` + `nmp-planner` extraction) | ✅ merged |
| 10 (`nmp-app-template`, V-48) | ✅ merged (#467) |
| 11 partial (chirp-* + `nmp-chirp-config` → `apps/chirp/`) | ✅ merged |
| 11 final (`nmp-ffi` extraction; codegen path hardcode lifted + `fixture-todo-core` → `apps/fixture/`) | ✅ merged (#472 + follow-up) |
| 12 (return `nmp-marmot` from `apps/` to `crates/`) | ❌ not started |

Adjacent: **V-51 routing observability** — phases 1 (substrate observer + ring buffer), 4 (validation harness against pablof7z's real NIP-65), 5 (kernel-router observability cut-over) ✅ merged. Phase 2 (FFI/wasm snapshot surface) ⏳ in flight (branch `feat/v51-routing-trace-ffi-wasm-snapshot`). Phase 3 (Chirp inspector UI) not started.

**Substrate-honest debts** — D ✅ merged (RwLock panics, #465). A ⏳ in PR #468 (router becomes decision authority — **needs kind:10002 self-seal fix before merge**, see "Active" below). B ✅ done — `default_routing.rs` (484 LOC duplicate of `nmp_router::GenericOutboxRouter` + `nmp_router::InMemoryMailboxCache`) deleted; kernel defaults to `EmptyOutboxRouter` + (test-only) `TestInMemoryMailboxCache`; production composition's existing `set_routing_substrate` factory unchanged. C (`ProtocolCommandContext` capability-trait bundling) ✅ merged (PR #471); follow-up collapse of the surviving 8-arg `new()` onto a named-field `ProtocolCommandContextParts` struct (drops `#[allow(clippy::too_many_arguments)]`; `protocol.rs` back under 500 LOC) in flight (see "Active" below).

## Active

- 2026-05-25 — refactor(nmp-core): Debt-C follow-up — collapse `ProtocolCommandContext::new`'s 8-positional-arg constructor onto a single named-field `ProtocolCommandContextParts` struct literal; drops `#[allow(clippy::too_many_arguments)]`. Doc-trim pass brings `crates/nmp-core/src/substrate/protocol.rs` from 519 → 497 LOC (back under the 500-LOC ceiling). All 6 call sites (`actor/dispatch.rs`, `actor/commands/remote_signer_tests.rs`, `substrate/protocol/tests.rs`, `nmp-nip17/src/dm_send/tests.rs`, `nmp-nip57/src/lnurl/tests.rs`) rewritten as struct-literal constructions; the test-only `with_send_only` constructor now also goes through `Self::new(ProtocolCommandContextParts { ... })`, preserving D11 (one production door). 894 nmp-core + 70 nmp-nip17 + 113 nmp-nip57 tests pass. — branch `refactor-protocol-ctx-new`.

- 2026-05-25 — refactor(nmp-router): spec §271 follow-up — move `Nip65OutboxResolver` out of `crates/nmp-core/src/publish/nip65/` into `crates/nmp-router/src/nip65_resolver.rs` (closes the structural debt the Opus reviewer flagged on step 3) — PR #484. The publish-side `OutboxResolver` trait stays in `nmp_core::publish::traits` (substrate seam); the in-crate kernel default is now `NoopOutboxResolver` (fail-closed). Production composition (`nmp-app-template::register_defaults`) installs the router-side resolver via a new `NmpApp::set_publish_resolver_factory` slot the actor reads at kernel construction (mirrors `set_routing_substrate` exactly, incl. `Reset` re-apply). New kernel accessors `event_store_handle()` / `indexer_relays_handle()` / `local_write_relays_handle()` / `active_account_handle()` thread the actor-owned slots into the factory. In-tree nmp-core tests auto-install a stripped-down `TestKind10002OutboxResolver` (publish/test_resolver.rs, 183 LOC) under `cfg(any(test, feature="test-support"))` — pulling `nmp-router` as a dev-dep would form a feature-incompatible cycle. — branch `move-nip65-resolver-to-router`.

- 2026-05-25 — refactor(nmp-codegen): un-hardcode `crates/` path; relocate `fixture-todo-core` to `apps/fixture/fixture-todo-core/`. Adds `crates/nmp-codegen/src/workspace.rs` (a `WorkspaceLayout` that discovers the workspace `Cargo.toml`, parses `[workspace] members`, and reads each member's `[package].name`) and rewires `generate::cargo_toml` to compute per-module `path = "…"` strings from that layout instead of the legacy `../../../crates/<name>` template. Temp-dir unit tests fall back to the old template (they only string-check the output). `tests/fixture_zero_diff.rs` is the regen-clean witness against the live tree. Doctrine-lint d0 exemption assertion updated to the new path. Closes the §11-final follow-up that revert PR #451 left open. — branch `agent/codegen-fixture-todo-core-relocate`.

- 2026-05-25 — refactor(nmp-core): eliminate `Nip17LocalKeysSlot` plumbing (V-39 §substrate-purity). Renamed the NIP-17-named slot type to substrate-generic `ActiveLocalKeysSlot`; dropped the NIP-17 noun from `ActorContext`, `run_actor_with_observers`, and all dispatch-arm writer call sites. `nmp-ffi` exposes the same `Arc<Mutex<Option<nostr::Keys>>>` through a renamed `NmpApp::active_local_keys()` accessor (was `nip17_local_keys()`); consumers in `nmp-app-template` (`DmInboxProjection` registration, NIP-57 zap-receipt runtime) and `apps/chirp/nmp-app-chirp/tests/dm_inbox_round_trip.rs` updated to match. The substrate now names no NIP at the active-local-keys seam (D0). — branch `refactor/eliminate-nip17-local-keys-slot`.

- 2026-05-24 — refactor(nmp-core): Debt A — router becomes live decision authority — PR #468. **Needs follow-up before merge**: `kernel/requests/profile.rs:431` routes kind:10002 discovery through the cached write set, which can self-seal stale metadata (if the cached relay list is old, asking only the old write relays misses the author's newer kind:10002 elsewhere). Discovery kinds (0/3/10000–19999) must hit the Indexer lane (router §3.1 lane 6).
- 2026-05-24 — refactor(nmp-core): Debt B — delete `default_routing.rs` (484 LOC algorithm duplicate); kernel defaults switched to `EmptyOutboxRouter` + `(test) TestInMemoryMailboxCache` — branch `worktree-agent-a635030638058cda7`.
- 2026-05-24 — feat(nmp-network): step 8 phase B — push-model `Pool` API + generational `RelayHandle` + `PoolEvent` channel — subagent in flight.
- 2026-05-24 — feat(nmp-network): step 8 phase C — move `BrowserRelayDriver` from `nmp-wasm/src/relay_driver.rs` into `nmp-network/src/browser_driver.rs` (gated `#[cfg(target_arch = "wasm32")]`). Driver's kernel touchpoints abstracted behind `BrowserKernelHandlers` (`Rc<dyn Fn>` callback bag) so the layering invariant (`nmp-network` does not depend on `nmp-core`) holds. `nmp-wasm::relay_pool::build_handlers` is the single construction site. — branch `feat/nmp-network-step-8-phase-c-browser-driver-move`.
- 2026-05-24 — feat(nmp-ffi): step 11 final — extract `nmp-core::ffi` to a sibling crate — subagent in flight.
- 2026-05-24 — docs(plan): post-merge reconciliation pass — branch `worktree-docs-postmerge-reconcile` (this branch).
- 2026-05-25 — feat(nmp-core/nmp-ffi/nmp-wasm): V-51 phase 2 — routing-trace FFI + wasm snapshot surface — PR #476. New FFI symbol `nmp_app_recent_routing_decisions` + wasm `NmpWasmRuntime::recent_routing_decisions()`; consumer-side JSON renderer in `nmp_core::kernel::routing_trace_dto` keeps substrate types free of `serde::Serialize`. `NmpCore.h` updated; CI drift gate green.
- 2026-05-25 — feat(nmp-core/nmp-network): step 8 phase F — kernel-actor cut-over to `nmp_network::pool::Pool`. The actor's 47 `RelayEvent`/`RelayCommand`/`spawn_relay_worker` callsites across `actor/{mod,dispatch,relay_mgmt}.rs` and the three actor test files are migrated to `Pool::ensure_open_with_role` / `Pool::send(handle, WireFrame::Text)` / `Pool::close(handle)`; the inbound event channel item is now `PoolEvent::{Opened, Frame, Closed, Failed, Health}`. The generational handle's stale-rejection invariant is preserved (the pool's translator drops stale-generation events; the actor's `resolve_handle` rechecks `handle.generation()` against the slot's current `RelayControl.handle.generation()`). The legacy `nmp_network::relay_worker` module is demoted to `pub(crate)` with `spawn_relay_worker` deleted (zero out-of-crate consumers remain; the pool wraps `spawn_relay_worker_with_keepalive` internally). All ~904 nmp-core lib tests + nmp-network 43 + nmp-signer-broker 39 + nmp-app-chirp + doctrine_lint smoke 42 + routing_trace_real_nostr live pass. — branch `feat/step-8-phase-f-actor-pool-cutover`.

- 2026-05-25 — chore(V-12): test-extraction batch — extract inline `#[cfg(test)] mod tests` blocks from `crates/nmp-router/src/router.rs` (703 → 242 LOC), `crates/nmp-core/src/substrate/routing.rs` (531 → 346 LOC), and `crates/nmp-core/src/substrate/protocol.rs` (745 → 519 LOC) into sibling `*/tests.rs` files via `#[path = ".../tests.rs"]`. All 904 nmp-core lib tests + 47 nmp-router tests + 42 doctrine_lint smoke pass; tests moved unchanged. router.rs and routing.rs now under the 500-LOC ceiling; protocol.rs remains 19 LOC over (production-side split is out-of-scope per V-12 staging). — PR #480.

- 2026-05-25 — feat(nmp-router): implement lanes 2/3/4/5 — close the `// TODO §3.1 lane N` markers in `crates/nmp-router/src/router.rs`. Lane 2 (Hint) lifts e/p/a/q tag position 2 on publish and `interest.hints[..].source = HintSource::EventTag` on subscribe; lane 3 (Provenance) lifts `interest.hints[..].source = HintSource::Provenance` (subscribe-only); lane 4 (UserConfigured) attributes `session_keys.active_write` on self-publish and `session_keys.active_read` when the active account is in the interest's author scope (or the interest is authorless); lane 5 (ClassRouted) refines the explicit-targets attribution to the right `EventClass` (Wiki for 818/30818/30819, Draft for 1234/31234, `Other("explicit")` otherwise) so the V-51 routing-trace inspector reports the class label rather than the placeholder. The mirror `nmp_core::kernel::test_router::TestOutboxRouter` is updated lane-for-lane so kernel tests cover the same coverage as production. 16 new tests in `crates/nmp-router/src/router/tests_lanes.rs`; 904 nmp-core lib + 63 nmp-router tests pass. — branch `agent/router-lanes-2345`.

- 2026-05-25 — feat(nmp-network): step 8 phase E — NIP-42 AUTH wire/FSM split. Adds `RelayFrame::Auth(challenge)` variant to `nmp_network::pool::RelayFrame` and pre-classifies inbound `["AUTH", <challenge>]` text frames at the wire layer via the dependency-free `nmp_nip42_types::parse_auth_frame`. The kind:22242 reply builder stays in `nmp-nip42::build_auth_event`; the per-relay pause/replay FSM stays in `nmp_core::subs::AuthGate`; `nmp-network` does not name either. A doctrine guard test (`auth_gate_and_22242_are_not_named_in_this_crate`) greps the crate's own source so future drift trips at test time. — branch `worktree-agent-ad2af474ef86cf998`.

## Recent history (verified merged or abandoned as of 2026-05-24)

- 2026-05-25 — refactor(nmp-codegen): un-hardcode `crates/` path; relocate `fixture-todo-core` — PR #485 open (480f86b0). Adds `crates/nmp-codegen/src/workspace.rs` (a `WorkspaceLayout` that discovers the workspace `Cargo.toml`, parses `[workspace] members`, and reads each member's `[package].name`); `generate::cargo_toml` consults the layout to compute per-dep `path = "…"` strings from the workspace's real layout instead of the legacy `../../../crates/<name>` template. Moves `crates/fixture-todo-core/` to `apps/fixture/fixture-todo-core/` (its spec home per `docs/architecture/crate-boundaries.md` per-crate-table) and regenerates `apps/fixture/nmp-app-fixture/` (one-line Cargo.toml dep-path delta). `tests/fixture_zero_diff.rs` regen-clean witness passes; `nmp-cli` init scaffold test (`tests/init.rs`) still passes — discovery works for both monorepo and scaffolded workspaces. Closes the §11-final follow-up that revert PR #451 left open.
- 2026-05-25 — feat(nmp-router): implement lanes 2/3/4/5 — PR #483 open (0e77cf31). Closes the `// TODO §3.1 lane N` markers in `GenericOutboxRouter`; all seven `RoutingSource` lanes now fire (Hint from e/p/a/q tag position 2 + `HintSource::EventTag` interest hints; Provenance from `HintSource::Provenance` interest hints; UserConfigured from `session_keys.active_read|write` when active account is in scope; ClassRouted refines explicit-targets attribution to the right `EventClass` per `evt.kind`). Mirror in `nmp_core::kernel::test_router::TestOutboxRouter` updated lane-for-lane. 16 new tests in `crates/nmp-router/src/router/tests_lanes.rs`; 904 nmp-core + 63 nmp-router + 42 doctrine_lint pass.
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
