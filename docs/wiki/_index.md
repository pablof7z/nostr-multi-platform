# Wiki Index

> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.

Last updated: 2026-06-14

## cache-serve (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [cache-serve](cache-serve.md) | Cache-Serve | Cache-serve gates v1 as a v1-blocker; the universal mechanism is a single event-acquisition pipeline where store-serving is the first half and the network REQ i | capture | warm | 2026-06-13 | cache-serve |
| [interest-withdrawal](interest-withdrawal.md) | Interest Withdrawal | Interest IDs are deterministic (group_message_interest_id over group_id_hex + relay_url); the kernel de-dupes via registry push replacing the slot, making re-re | capture | warm | 2026-06-13 | cache-serve |

## capability-socket (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [capability-socket](capability-socket.md) | Capability Socket | The capability trampoline routes all non-external_signer namespaces synchronously to a Kotlin handler registered via nativeSetCapabilityHandler; the existing ex | capture | warm | 2026-06-13 | capability-socket |
| [signer-session-port](signer-session-port.md) | Signer-Session Port | The signer-session capability port (ADR-0050) generalizes the prior single-verb SignEventForAccount into a backend-transparent signer-session capability coverin | capture | warm | 2026-06-13 | capability-socket |

## chirp-migration (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-migration](chirp-migration.md) | Chirp Migration | The TUI Chirp shell uses NMP registry components for name (NostrProfileName), content (NostrContentView), avatar (NostrAvatar), and NIP-05 (NostrNip05Badge), ma | capture | warm | 2026-06-13 | chirp-migration |

## ci-gates (4 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [build-infrastructure](build-infrastructure.md) | Build Infrastructure | Disk space has been a recurring constraint (ENOSPC stalled agents three times) | capture | warm | 2026-06-13 | ci-gates |
| [ci-gates](ci-gates.md) | CI Gates | CI must include `cargo test -p nmp-app-template` and `cargo build --workspace --examples` in the test plan to prevent the example-compile gap class that let a ` | capture | warm | 2026-06-13 | ci-gates |
| [model-substitution](model-substitution.md) | Model Substitution | The fable-5 model became inaccessible during the session, killing two early K1 leads; all subsequent teams ran on Opus/Sonnet. | capture | warm | 2026-06-13 | ci-gates |
| [wasm-publish](wasm-publish.md) | WASM Publish | Issue #1202 (wasm silent publish failure) is resolved by replacing the silent NoTargets with an honest CapabilityFailure (`publish_not_supported_in_web_preview_ | capture | warm | 2026-06-13 | ci-gates |

## codebase-patterns (8 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [action-lifecycle](action-lifecycle.md) | Action Lifecycle | Action feedback collapses to a single mechanism: action_lifecycle with TTL-anchored retention, where ack serves as an early-dismiss only | capture | warm | 2026-06-14 | codebase-patterns |
| [agent-workflow-policy](agent-workflow-policy.md) | Agent Workflow Policy | Completed PR descriptions must include a short TLDR summary, a detailed overview of the work performed, and any subjective decisions including tradeoffs or assu | capture | warm | 2026-06-14 | codebase-patterns |
| [agents-md-policy](agents-md-policy.md) | AGENTS.md Policy | The week-one act before any keystone is landing the supersession-deletion policy in AGENTS.md plus the empty mechanism_census test | capture | warm | 2026-06-13 | codebase-patterns |
| [codebase-patterns](codebase-patterns.md) | Codebase Patterns | Hand-authored source and documentation files must be kept under 300 lines of code where practical, with 500 lines as a hard ceiling | capture | warm | 2026-06-13 | codebase-patterns |
| [excellence-program](excellence-program.md) | Excellence Program | The excellence program identifies six repo-wide patterns and defines EXCELLENT per pattern: exactly one production mechanism per capability (P1), since floors t | capture | warm | 2026-06-13 | codebase-patterns |
| [git-worktree-policy](git-worktree-policy.md) | Git Worktree Policy | Agents must work in isolated git worktrees, never moving the base repo away from master. | capture | warm | 2026-06-13 | codebase-patterns |
| [keystone-overview](keystone-overview.md) | Keystone Overview | The three keystones that discharge most of the six patterns are: K1 (signer-session port covering sign/encrypt/decrypt with mailbox-completion delivery), K2 (in | capture | warm | 2026-06-13 | codebase-patterns |
| [shell-protocol-violations](shell-protocol-violations.md) | Shell Protocol Violations | Issue #1283 is resolved with the EmbedHost resolver moving to nmp-ffi (which sits above both nmp-core and nmp-content in the DAG), shipping a typed EmbedKindPro | capture | warm | 2026-06-13 | codebase-patterns |

## concurrency (4 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [concurrency](concurrency.md) | Concurrency | Polling is forbidden at every layer of the stack: no sleep+check loops, no Timer.scheduledTimer querying state, no try_recv+sleep spin loops, no Task while !can | capture | warm | 2026-06-14 | concurrency |
| [concurrency-ordering](concurrency-ordering.md) | Concurrency Ordering | All Relaxed orderings on the cancel flag were replaced with Release/Acquire pairs for correctness on ARM. | capture | warm | 2026-06-13 | concurrency |
| [dispatcher-shutdown](dispatcher-shutdown.md) | Dispatcher Shutdown | The dispatcher thread is joined in `cancel()` with a race guard to prevent thread leaks | capture | warm | 2026-06-13 | concurrency |
| [network-pool-timeout](network-pool-timeout.md) | Network Pool Timeout | Network pool connection must use `TcpStream::connect_timeout` (10 s) and `client_tls_with_config` to prevent shutdown blocking for ~75 seconds on black-hole hos | capture | warm | 2026-06-13 | concurrency |

## cron-create (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [cron-create](cron-create.md) | Cron Create | Recurring CronCreate tasks auto-expire after 7 days. | capture | warm | 2026-06-13 | cron-create |

## data-persistence (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [wasm-persistence](wasm-persistence.md) | WASM Persistence | The WASM runtime must use OPFS SyncAccessHandle-backed SQLite as the primary persistence backend, not IndexedDB, because EventStore is a synchronous trait and I | capture | warm | 2026-06-14 | data-persistence |

## event-acquisition (7 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [coverage-ledger](coverage-ledger.md) | Coverage Ledger | The coverage ledger (K3) discharges P2 wholesale and is the precondition #1090's eviction re-enable was always missing; it is the surgery and goes last, behind | capture | warm | 2026-06-13 | event-acquisition |
| [event-acquisition](event-acquisition.md) | Event Acquisition | There is a single event-acquisition mechanism: serving from the local store is its first half, the planner's wire REQ is its refinement half, running through th | capture | warm | 2026-06-13 | event-acquisition |
| [feed-pagination](feed-pagination.md) | Feed Pagination | Feed pagination caps at MAX_FEED_WINDOW_LIMIT (500) | capture | warm | 2026-06-13 | event-acquisition |
| [kernel-timestamp-clamp](kernel-timestamp-clamp.md) | Kernel Timestamp Clamp | Kernel fan-out must clamp `created_at` on `KernelEvent` to `now_secs` for the observer-visible timestamp while preserving the wire timestamp in `StoredEvent` fo | capture | warm | 2026-06-13 | event-acquisition |
| [neg-77-set-reconciliation](neg-77-set-reconciliation.md) | NEG-77 Set Reconciliation | Relay scores must be fed from mainline ingest (record Hit) rather than only from claims, activating the W4 warm filter and giving record_failure teeth for relay | capture | warm | 2026-06-14 | event-acquisition |
| [tag-ingest](tag-ingest.md) | Tag Ingest | Malformed elements in tag loops are skipped rather than treated as fatal errors. | capture | warm | 2026-06-13 | event-acquisition |
| [watermark-removal](watermark-removal.md) | Watermark Removal | The live since-floor for REQ subscriptions is derived from store content (newest matching event per author/coord/tag) via watermark_fn, not from persisted Water | capture | warm | 2026-06-13 | event-acquisition |

## kernel-snapshot (4 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [incremental-emission](incremental-emission.md) | Incremental Emission | ADR-0055 (Rung 1) uses source-version-stamp derived revs (option c) rather than per-projection mutation-site bumps (option a) or emit-time content hashing as th | capture | warm | 2026-06-14 | kernel-snapshot |
| [kernel-snapshot](kernel-snapshot.md) | Kernel Snapshot | The kernel omits Unchanged projection rows entirely from the wire, keeps an explicit payload-less Cleared row, and keeps full Changed rows. | capture | warm | 2026-06-14 | kernel-snapshot |
| [kernel-snapshot-adr](kernel-snapshot-adr.md) | Kernel Snapshot ADR | FullState/full snapshot is the correctness path; granular ViewBatch or delta variants are added only when profiling proves the snapshot path is the bottleneck a | capture | warm | 2026-06-13 | kernel-snapshot |
| [projection-cache-interposer](projection-cache-interposer.md) | Projection Cache Interposer | The iOS ProjectionCache interposer keeps a keyâ(rev, bytes) cache, merges each frame (Changed overwrites, Cleared drops, omitted retained), and hands the exis | capture | warm | 2026-06-14 | kernel-snapshot |

## mls (5 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [android-mls-keyring](android-mls-keyring.md) | Android MLS Keyring | Android used an in-memory mock keyring in production, causing group secrets to be lost on every app restart. | capture | warm | 2026-06-13 | mls |
| [android-mls-ui](android-mls-ui.md) | Android MLS UI | Android Marmot parity ops (leave, invite, remove, clear_pending) are thin dispatch shells with zero Kotlin protocol logic, available in the UI via typed seriali | capture | warm | 2026-06-13 | mls |
| [marmot-pending-ops](marmot-pending-ops.md) | Marmot Pending Ops | When create_group or invite encounters key_package_unavailable, the MarmotMlsOpHandler parks a pending op (typed action + correlation_id + missing pubkey set) a | capture | warm | 2026-06-13 | mls |
| [marmot-resubscribe](marmot-resubscribe.md) | Marmot Resubscribe | On register_with_keys (restart), MarmotProjection resubscribes per-group kind:445 message interests by enumerating persisted groups, reading their stored relays | capture | warm | 2026-06-13 | mls |
| [mls-architecture](mls-architecture.md) | MLS Architecture | Chirp MLS logic is owned by Rust; iOS and Android shells contain zero protocol/crypto/ratchet logic (only ADR-0032 display formatting) | capture | warm | 2026-06-13 | mls |

## mobile-build-config (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [android-frame-decoder](android-frame-decoder.md) | Android Frame Decoder | Android consumers must skip v0.3.0 and pin v0.4.0 directly due to a completely dark Android frame decoder in v0.3.0 | capture | warm | 2026-06-13 | mobile-build-config |
| [mobile-build-config](mobile-build-config.md) | Mobile Build Config | iOS device builds require IPHONEOS_DEPLOYMENT_TARGET=17.0 to avoid a ___chkstk_darwin linker error (unavailable at iOS 10.0 baseline) caused by the Xcode 26 SDK | capture | warm | 2026-06-13 | mobile-build-config |

## nip-55-signer (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [nip-55-signer](nip-55-signer.md) | NIP-55 External Signer | NIP-55 (Amber external signer) ADR-0048 places the signer behind the uniform V-78 SignEventForAccount port with a 90-second per-op interactive deadline, pubkey- | capture | warm | 2026-06-13 | nip-55-signer |

## nmp-app-integration (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [cli-registry-manifest](cli-registry-manifest.md) | CLI Registry Manifest | The CLI registry manifest must mirror all component ids that appear in the web registry, including web-targeted components such as web/login-block, web/relay-li | capture | warm | 2026-06-14 | nmp-app-integration |
| [instance-scoped-registration](instance-scoped-registration.md) | Instance-Scoped Registration | Instance-scoped module registration replaces type-based register_action::<M>() with instance-scoped register_action(&mut self, module: M), where extension modul | capture | warm | 2026-06-13 | nmp-app-integration |
| [nmp-app-integration](nmp-app-integration.md) | NMP App Integration | The hl app fully embeds the NMP kernel via path deps (including nmp-ffi with the external-signer feature) and uses UniFFI (not JNI) for the FFI boundary, so no | capture | warm | 2026-06-13 | nmp-app-integration |

## nmp-ffi-surface (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [ios-ffi-safety](ios-ffi-safety.md) | iOS FFI Safety | iOS KernelBridge.listen() uses passUnretained(sink) creating a fragile ARC teardown contract; passRetained(sink) with takeRetainedValue in the callback is safer | capture | warm | 2026-06-13 | nmp-ffi-surface |
| [nmp-ffi-surface](nmp-ffi-surface.md) | NMP FFI Surface | The legacy author/thread C-ABI open surfaces (nmp_app_open_author, nmp_app_close_author, nmp_app_open_thread, nmp_app_close_thread) are removed; consumers must | capture | warm | 2026-06-13 | nmp-ffi-surface |

## projection-registry (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [projection-doctrine](projection-doctrine.md) | Projection Doctrine | Host-declared projection subscriptions are rejected by ADR-0039 on the principle that the kernel must never know which view is open; this blanket ban conflates | capture | warm | 2026-06-13 | projection-registry |
| [projection-registry](projection-registry.md) | Projection Registry | The projection registry contains 34 total keys (28 with Swift typed decode stubs) after the removal of KAVW and KTVW. | capture | warm | 2026-06-13 | projection-registry |

## protocol-ingest-safety (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [protocol-ingest-safety](protocol-ingest-safety.md) | Protocol Ingest Safety | NWC `url_decode` casts arbitrary bytes to `char` via `bytes[i] as char`, producing ill-formed Unicode for percent-encoded multi-byte UTF-8 sequences and potenti | capture | warm | 2026-06-13 | protocol-ingest-safety |

## ram-eviction (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [ram-eviction](ram-eviction.md) | RAM Eviction | Open-view RAM eviction pins events matching any of four sets per open thread view: the focused event id, the derived root id, referenced_event_ids of the focuse | capture | warm | 2026-06-13 | ram-eviction |

## relay-routing (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [outbox-resolver](outbox-resolver.md) | Outbox Resolver | The OutboxResolver must apply the blocked-relay filter on publish, not just subscribe, to prevent publishing to user-blocked relays | capture | warm | 2026-06-13 | relay-routing |
| [wasm-relay-pool](wasm-relay-pool.md) | WASM Relay Pool | The WASM relay pool opens one WebSocket per distinct relay URL (native parity), reports inbound frames under a single first-role-wins role, and spawns drivers on demand for kernel-targeted URLs so the kernel owns socket lifecycle on web. | capture | warm | 2026-06-14 | relay-routing |

## shell-defects (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [desktop-shell-defects](desktop-shell-defects.md) | Desktop Shell Defects | The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never a | capture | warm | 2026-06-13 | shell-defects |

## store-projection-replay (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [store-projection-replay](store-projection-replay.md) | Store-Projection Replay | ADR-0045 storeâprojection replay is accepted (staged); implementation is tracked separately | capture | warm | 2026-06-13 | store-projection-replay |

## zap-scope (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [zap-protocol](zap-protocol.md) | Zap Protocol | The fetched bolt11 amount must be validated against the requested amount before auto-pay using the in-crate amount_msats parser | capture | warm | 2026-06-14 | zap-scope |
| [zap-scope](zap-scope.md) | Zap Scope | Zap work is declared post-v1 by owner decision; issues #1008, #999, and #967 are deferred to post-v1 and their needs-decision labels should be dropped | capture | warm | 2026-06-13 | zap-scope |

## Research Records (68 records)

| Record | Date | Finding | Agent |
|--------|------|---------|-------|
| [2026-06-13-1-adversarial-review-of-20-wave-1](research/2026-06-13-1-adversarial-review-of-20-wave-1.md) | 2026-06-13 | Adversarial review of 20 Wave-1 PRs against doctrine criteria (file-size gates, real tests, thin shell, no baseline bumps), producing approved (9) and rejected (1, #1298 for hard-cap violation) verdicts with per-PR CI check tallies and detailed parity/correctness/test analysis | wf_64b436b2-acb (Wave 1 workflow) |
| [2026-06-13-1-audit-of-android-compose-claim-host](research/2026-06-13-1-audit-of-android-compose-claim-host.md) | 2026-06-13 | Audit of Android Compose claim-host components across 5 dimensions (claim lifecycle, D0 thin-shell, dedup, image loading, LazyColumn recycling) with HIGH/MEDIUM/LOW/CLEAN verdicts; found HIGH claim-churn loop bug | Audit new #1294 Android claim components |
| [2026-06-13-1-diagnosis-of-post-restart-marmot-group](research/2026-06-13-1-diagnosis-of-post-restart-marmot-group.md) | 2026-06-13 | Diagnosis of post-restart Marmot group message delivery failure; confirmed hypothesis that register_with_keys never re-subscribes per-group kind:445 feeds for already-joined groups on restart, with PR-by-PR fix plan and regression test design | sub-agent (diagnosis investigation) |
| [2026-06-13-1-mls-marmot-support-verification-for-ios](research/2026-06-13-1-mls-marmot-support-verification-for-ios.md) | 2026-06-13 | MLS/Marmot support verification for iOS and Android: verdict SHOULD-WORK-UNVERIFIED for both platforms, with Rust-owns-logic check and V-109 gap quantification | Verify MLS support iOS+Android (aa96677fc89e26ff3) |
| [2026-06-13-1-mls-support-verification-for-chirp-ios](research/2026-06-13-1-mls-support-verification-for-chirp-ios.md) | 2026-06-13 | MLS support verification for Chirp iOS and Android, verdict SHOULD-WORK-UNVERIFIED for both platforms (report truncated in transcript) | aa96677fc89e26ff3 |
| [2026-06-13-1-opus-agent-evaluation-of-all-11](research/2026-06-13-1-opus-agent-evaluation-of-all-11.md) | 2026-06-13 | Opus agent evaluation of all 11 needs-decision issues against documented product direction, classifying each as A (determined-by-direction) or B (needs-owner) with verified code facts and unblocked actions | aa54266b1636c10ef |
| [2026-06-13-1-systematic-triage-of-88-open-github](research/2026-06-13-1-systematic-triage-of-88-open-github.md) | 2026-06-13 | Systematic triage of 88 open GitHub issues against pre-registered doctrine criteria, classifying each as must-fix (39), close-stale (23), owner-decision (8), or legit-deferred (18), with 9 confirmed business-logic-in-UI violations identified | main |
| [2026-06-13-2-opus-code-review-of-pr-1332](research/2026-06-13-2-opus-code-review-of-pr-1332.md) | 2026-06-13 | Opus code review of PR #1332 (relay-diagnostics raw timestamps): verdict REQUEST-CHANGES with 3 blockers (wrong flatc version, invalid JSON fixture, file-size hard-cap expansion) | Opus review PR #1332 |
| [2026-06-13-2-re-verification-of-11-wave-1b](research/2026-06-13-2-re-verification-of-11-wave-1b.md) | 2026-06-13 | Re-verification of 11 Wave-1b debt-fix PRs after file-size splits, release-manifest fix, and iOS-lane fix, producing green (8 PRs) and notGreen (3 PRs: #1308 follow-fanout regression, #1319 flaky wasm, #1316 Sendable compile error) verdicts with per-PR CI evidence | wf_94b80062-705 (Wave 1b workflow) |
| [2026-06-13-2-root-cause-diagnosis-of-post-restart](research/2026-06-13-2-root-cause-diagnosis-of-post-restart.md) | 2026-06-13 | Root-cause diagnosis of post-restart live message reception failure in nmp-marmot, HYPOTHESIS CONFIRMED: register_with_keys never re-subscribes per-group kind:445 feeds | a0bfdad69034b526e |
| [2026-06-13-3-post-hoc-architectural-review-of-merged](research/2026-06-13-3-post-hoc-architectural-review-of-merged.md) | 2026-06-13 | Post-hoc architectural review of merged ADR-0053 (host-declared projections): verdict HAS-DEBT-FIX-FORWARD — empty=everything footgun, missing drift-protection gate, unenforced init-only invariant | Post-hoc review merged Decision-2 (a1ffdfeca42f52771) |
| [2026-06-13-3-re-verification-of-5-wave-1c](research/2026-06-13-3-re-verification-of-5-wave-1c.md) | 2026-06-13 | Re-verification of 5 Wave-1c PRs (3 rebases + 2 real-bug fixes), producing green (1, #1308) and notGreen (4 PRs with inherited master reds, negentropy regression, codegen drift, ChirpTests garbled-bytes failure) verdicts with per-PR CI evidence | wf_4ffc29ed-975 (Wave 1c workflow) |
| [2026-06-14-1-adversarial-code-review-of-pr-1417](research/2026-06-14-1-adversarial-code-review-of-pr-1417.md) | 2026-06-14 | Adversarial code review of PR #1417 (feed change-signal) with pre-registered rulings on determinism, flag divergence, epoch-reset coverage, and test/debt status; verdict REQUEST-CHANGES due to Reset-freeze bug and file-size CI failure | aa4f32b4bf798bb50 |
| [2026-06-14-1-adversarial-review-of-ios-projectioncache-interposer](research/2026-06-14-1-adversarial-review-of-ios-projectioncache-interposer.md) | 2026-06-14 | Adversarial review of iOS ProjectionCache interposer (PR #1409); verdict REQUEST-CHANGES because iOS build fails (missing FlatBuffers import) and double-decode perf debt | Opus review R3-S3 PR1409 subagent |
| [2026-06-14-1-adversarial-review-of-pr-1388-r3](research/2026-06-14-1-adversarial-review-of-pr-1388-r3.md) | 2026-06-14 | Adversarial review of PR #1388 (R3-S1 producer omit-Unchanged): deviation is over-emit-only safe, but two file-size hard-cap violations block merge — REQUEST-CHANGES | Opus review R3-S1 PR1388 |
| [2026-06-14-1-adversarial-review-of-pr-1389-flatbufferbuilder](research/2026-06-14-1-adversarial-review-of-pr-1389-flatbufferbuilder.md) | 2026-06-14 | Adversarial review of PR #1389 (FlatBufferBuilder reuse) — APPROVE-WITH-NITS; use-after-reset and re-entrancy SAFE, one vacuous test identified | Opus review agent (subagent) |
| [2026-06-14-1-adversarial-review-of-pr-1393-cleared](research/2026-06-14-1-adversarial-review-of-pr-1393-cleared.md) | 2026-06-14 | Adversarial review of PR #1393 Cleared-signal completeness fix; verdict REQUEST-CHANGES due to perpetual-Changed byte leak in note_copy_emit and masked missing rev bump | opus-review-r3-s1b-pr1393 |
| [2026-06-14-1-adversarial-review-of-pr-1393-empirically](research/2026-06-14-1-adversarial-review-of-pr-1393-empirically.md) | 2026-06-14 | Adversarial review of PR #1393 empirically verified regression-test non-vacuity by running tests on master (5/6 fail), found perpetual-Changed byte leak in note_copy_emit; verdict REQUEST-CHANGES | opus |
| [2026-06-14-1-empirical-measurement-of-idle-feed-re](research/2026-06-14-1-empirical-measurement-of-idle-feed-re.md) | 2026-06-14 | Empirical measurement of idle-feed re-serialization waste: 58.8 KB/tick byte-identical across 40 idle ticks, 129µs release vs 2266µs debug encode (17.6× factor), verdict: feed is ~6× rest of frame and re-serializes unconditionally — recommendation is staged Option A (rev-gate feed) then possibly B (row-deltas) gated behind release measurement | a85df4db283078c1e |
| [2026-06-14-1-focused-re-review-of-r6-s1](research/2026-06-14-1-focused-re-review-of-r6-s1.md) | 2026-06-14 | Focused re-review of R6-S1 freeze fix: empirically proved Reset freeze is closed (c10 fails pre-fix, passes post-fix) and no new missed-emit edge in frame-identity ordering; verdict APPROVE | Focused re-review R6-S1 freeze fix (ab57b3bb7b2fe54fb) |
| [2026-06-14-1-measured-idle-feed-re-serialization-waste](research/2026-06-14-1-measured-idle-feed-re-serialization-waste.md) | 2026-06-14 | Measured idle feed re-serialization waste: 58.8 KB/tick byte-identical across 40 idle ticks, feed ~6× rest of frame, release 129µs vs debug 2266µs encode; verdict decisive waste, recommendation staged Option A (rev-gating) then maybe B (row-deltas) | a85df4db283078c1e (Design Tier-1 feed gating rung) |
| [2026-06-14-1-mls-marmot-support-verification-for-ios](research/2026-06-14-1-mls-marmot-support-verification-for-ios.md) | 2026-06-14 | MLS/Marmot support verification for iOS and Android — verdict SHOULD-WORK-UNVERIFIED for both platforms with code-grounded FFI/JNI/UI evidence | Verify MLS support iOS+Android (sonnet agent) |
| [2026-06-14-1-opus-adversarial-review-of-r3-s5](research/2026-06-14-1-opus-adversarial-review-of-r3-s5.md) | 2026-06-14 | Opus adversarial review of R3-S5 capstone PR#1413 — reproduces all numbers across 5 runs, adjudicates metric swap legitimacy, serialize-time tolerance, byte-identity oracle, and file-size gates; verdict: REQUEST-CHANGES (metric swap principled, but file-size hard-cap violations and docstring overclaims block merge) | a4d22df1effd64b83 |
| [2026-06-14-1-opus-review-of-adr-0055-r3](research/2026-06-14-1-opus-review-of-adr-0055-r3.md) | 2026-06-14 | Opus review of ADR-0055 R3-S1b Cleared-signal completeness: found perpetual-Changed re-emission defect in note_copy_emit (presence.rs), verified other findings CORRECT/PASS, verdict REQUEST-CHANGES with 3 required fixes | opus-review-agent |
| [2026-06-14-1-r6-s1-freeze-fix-re-review](research/2026-06-14-1-r6-s1-freeze-fix-re-review.md) | 2026-06-14 | R6-S1 freeze fix re-review: two kill criteria (Reset freeze closed, no new freeze/missed-emit edge) empirically validated by reverting code and running tests — verdict APPROVE | Focused re-review R6-S1 freeze fix |
| [2026-06-14-1-r6-s4-feed-idle-capstone-measurement](research/2026-06-14-1-r6-s4-feed-idle-capstone-measurement.md) | 2026-06-14 | R6-S4 feed-idle capstone measurement: 4 hard gates (idle feed bytes omitted, frame bytes ON<OFF, byte-identity oracle, false-resend rate) all PASS; 97.6% idle frame-byte reduction (45,440→1,104 B) | a36e4107cc809b311 |
| [2026-06-14-1-s6-capstone-empirical-measurement-of-incremental](research/2026-06-14-1-s6-capstone-empirical-measurement-of-incremental.md) | 2026-06-14 | S6 capstone empirical measurement of incremental emission: 4/4 gates PASS (row_suppression 0.6875≥0.50, p50 bytes 7928≤9639, serialize_us 61µs≤75µs, byte-identity oracle 0.0≤0.0) | a6c7622ab3cc18ff2 |
| [2026-06-14-1-s6-capstone-harness-measuring-incremental-emission](research/2026-06-14-1-s6-capstone-harness-measuring-incremental-emission.md) | 2026-06-14 | S6 capstone harness measuring incremental-emission savings: 4 pre-registered gates all PASS (row_suppression_ratio 0.6875≥0.50, frame bytes 7928≤9639B, serialize 61µs≤75µs, byte identity 0.0≤0.0) | a6c7622ab3cc18ff2 |
| [2026-06-14-1-verification-of-mls-marmot-support-in](research/2026-06-14-1-verification-of-mls-marmot-support-in.md) | 2026-06-14 | Verification of MLS/Marmot support in Chirp iOS and Android against Rust-owns-logic architecture criteria, verdict SHOULD-WORK-UNVERIFIED for both platforms | Verify MLS support iOS+Android |
| [2026-06-14-1-verification-of-mls-marmot-support-on](research/2026-06-14-1-verification-of-mls-marmot-support-on.md) | 2026-06-14 | Verification of MLS/Marmot support on iOS and Android; verdict SHOULD-WORK-UNVERIFIED for both platforms with detailed FFI/projection/UI evidence | Verify MLS support iOS+Android |
| [2026-06-14-2-adversarial-opus-review-of-s5-capstone](research/2026-06-14-2-adversarial-opus-review-of-s5-capstone.md) | 2026-06-14 | Adversarial Opus review of S5 capstone: REQUEST-CHANGES — metric swap judged legitimate (row_suppression vs waste_ratio), 5-run independent reproduction, docstring overclaim and two file-size violations blocking | a4d22df1effd64b83 |
| [2026-06-14-2-adversarial-review-of-android-projectioncache-interposer](research/2026-06-14-2-adversarial-review-of-android-projectioncache-interposer.md) | 2026-06-14 | Adversarial review of Android ProjectionCache interposer (PR #1410); verdict APPROVE-WITH-NITS, D3-4 decode-before-commit parity confirmed end-to-end | Opus review R3-S4 PR1410 subagent |
| [2026-06-14-2-adversarial-review-of-pr-1389-kernel](research/2026-06-14-2-adversarial-review-of-pr-1389-kernel.md) | 2026-06-14 | Adversarial review of PR 1389 kernel FlatBufferBuilder reuse; verdict APPROVE-WITH-NITS, one vacuous test found, use-after-reset and re-entrancy ruled safe | Opus review R3-S2 PR1389 |
| [2026-06-14-2-adversarial-review-of-pr-1389-r3](research/2026-06-14-2-adversarial-review-of-pr-1389-r3.md) | 2026-06-14 | Adversarial review of PR #1389 (R3-S2 encoder buffer reuse): use-after-reset safe, re-entrancy safe, aux path correctly left alone, one test partially vacuous — APPROVE-WITH-NITS | Opus review R3-S2 PR1389 |
| [2026-06-14-2-adversarial-review-of-pr-1393-cleared](research/2026-06-14-2-adversarial-review-of-pr-1393-cleared.md) | 2026-06-14 | Adversarial review of PR #1393 (Cleared-signal completeness) — REQUEST-CHANGES; regression test proven non-vacuous (5/6 fail on master), perpetual-Changed byte leak found in note_copy_emit | Opus review agent (subagent) |
| [2026-06-14-2-adversarial-review-of-pr-1393-verdict](research/2026-06-14-2-adversarial-review-of-pr-1393-verdict.md) | 2026-06-14 | Adversarial review of PR #1393 — verdict REQUEST-CHANGES due to perpetual Changed re-emission byte leak in note_copy_emit; regression test proven non-vacuous on master | Opus review R3-S1b PR1393 |
| [2026-06-14-2-adversarial-review-of-pr-1409-empirically](research/2026-06-14-2-adversarial-review-of-pr-1409-empirically.md) | 2026-06-14 | Adversarial review of PR #1409 empirically built iOS (xcodebuild failed — missing FlatBuffers import), verified merge algorithm D3-4 correctness and apply-gating; verdict REQUEST-CHANGES | opus |
| [2026-06-14-2-adversarial-review-of-pr-1409-ios](research/2026-06-14-2-adversarial-review-of-pr-1409-ios.md) | 2026-06-14 | Adversarial review of PR #1409 iOS ProjectionCache interposer; verdict REQUEST-CHANGES due to missing FlatBuffers import causing iOS build failure | opus-review-r3-s3-pr1409 |
| [2026-06-14-2-adversarial-review-of-pr-1417-r6](research/2026-06-14-2-adversarial-review-of-pr-1417-r6.md) | 2026-06-14 | Adversarial review of PR #1417 R6-S1 feed change-signal against 3 pre-registered rulings, verdict REQUEST-CHANGES — found Reset/session freeze bug and CI file-size blocker | Opus review R6-S1 PR1417 |
| [2026-06-14-2-adversarial-review-of-s5-capstone-pr](research/2026-06-14-2-adversarial-review-of-s5-capstone-pr.md) | 2026-06-14 | Adversarial review of S5 capstone PR#1413 verifying metric-swap honesty and reproducibility: REQUEST-CHANGES (docstring overclaims 81%→<5%, removed gate still listed, byte-identity is end-state not per-tick, two file-size hard-cap violations) | a4d22df1effd64b83 |
| [2026-06-14-2-focused-re-review-of-pr-1417](research/2026-06-14-2-focused-re-review-of-pr-1417.md) | 2026-06-14 | Focused re-review of PR #1417 freeze fix verifying two pre-registered kill criteria (Reset freeze closure, new missed-emit edges); verdict APPROVE after empirical falsification of c10 test pre-fix | ab57e89885690a16c |
| [2026-06-14-2-focused-review-of-r6-s2-pr1418](research/2026-06-14-2-focused-review-of-r6-s2-pr1418.md) | 2026-06-14 | Focused review of R6-S2 PR1418: verified feed refactor is behavior-preserving, both keys have per-key freeze guards, and frame-identity ordering is correct; verdict APPROVE-WITH-NITS (lib.rs file-size gate fails) | Focused review R6-S2 PR1418 (a4b39778c6d1cac71) |
| [2026-06-14-2-opus-review-of-r3-s3-ios](research/2026-06-14-2-opus-review-of-r3-s3-ios.md) | 2026-06-14 | Opus review of R3-S3 iOS ProjectionCache interposer: iOS build FAILED (missing FlatBuffers import in KernelBridge.swift), algorithm correct, host-apply gating sound, verdict REQUEST-CHANGES | Opus review R3-S3 PR1409 |
| [2026-06-14-2-r6-s2-pr1418-review-four-criteria](research/2026-06-14-2-r6-s2-pr1418-review-four-criteria.md) | 2026-06-14 | R6-S2 PR1418 review: four criteria (feed-refactor behavior-preserving, per-key freeze guard, publish ordering, capability-OFF byte-identical) evaluated via diff and test runs — verdict APPROVE-WITH-NITS | Focused review R6-S2 PR1418 |
| [2026-06-14-2-r6-s4-feed-idle-capstone-measurement](research/2026-06-14-2-r6-s4-feed-idle-capstone-measurement.md) | 2026-06-14 | R6-S4 feed-idle capstone measurement: two-phase idle benchmark proving 97.6% frame-byte reduction with 4 pre-registered PASS/FAIL gates all passing | sonnet (implement R6-S4 agent) |
| [2026-06-14-2-release-a-b-idle-jank-measurement](research/2026-06-14-2-release-a-b-idle-jank-measurement.md) | 2026-06-14 | Release A/B idle-jank measurement: hypothesis that feed-omission stops idle timeline body re-eval is REFUTED — .equatable() is the load-bearing shield; body-evals/sec = 0 in both ON and OFF arms | ac4b61441e369b683 |
| [2026-06-14-2-v1-feature-completeness-verification-of-promised](research/2026-06-14-2-v1-feature-completeness-verification-of-promised.md) | 2026-06-14 | v1 feature-completeness verification of promised user capabilities against actual iOS/Android/desktop shell code; 23 gaps found ranked by impact, including Android production identity not persisted on cold start (HIGH/broken) and missing reply-compose + follow-list projection gaps | w4w7g7a2r |
| [2026-06-14-3-adversarial-review-of-pr-1393-r3](research/2026-06-14-3-adversarial-review-of-pr-1393-r3.md) | 2026-06-14 | Adversarial review of PR #1393 (R3-S1b Cleared-signal completeness): regression test proven non-vacuous (5/6 fail on master), but note_copy_emit introduces perpetual-Changed byte leak — REQUEST-CHANGES | Opus review R3-S1b PR1393 |
| [2026-06-14-3-adversarial-review-of-pr-1409-ios](research/2026-06-14-3-adversarial-review-of-pr-1409-ios.md) | 2026-06-14 | Adversarial review of PR #1409 (iOS ProjectionCache interposer) — REQUEST-CHANGES; iOS build fails (missing FlatBuffers import), merge algorithm and apply-gating verified correct | Opus review agent (subagent) |
| [2026-06-14-3-adversarial-review-of-pr-1410-android](research/2026-06-14-3-adversarial-review-of-pr-1410-android.md) | 2026-06-14 | Adversarial review of PR #1410 Android ProjectionCache interposer; verdict APPROVE-WITH-NITS, D3-4 decode-before-commit honored, isNotEmpty divergence illusory | opus-review-r3-s4-pr1410 |
| [2026-06-14-3-adversarial-review-of-pr-1410-empirically](research/2026-06-14-3-adversarial-review-of-pr-1410-empirically.md) | 2026-06-14 | Adversarial review of PR #1410 empirically ran codegen checks and gradle tests (208 pass, 0 fail), traced corrupt-payload paths on both platforms, ruled D3-4 parity holds; verdict APPROVE-WITH-NITS | opus |
| [2026-06-14-3-design-of-cleared-signal-completeness-fix](research/2026-06-14-3-design-of-cleared-signal-completeness-fix.md) | 2026-06-14 | Design of Cleared-signal completeness fix for #1390; manifest enumerates full key universe so fix is consumer-side (omit_unchanged), codex overturned finding-7 proposed fix | Design R3-S1b Cleared-signal fix |
| [2026-06-14-3-focused-re-review-of-r6-s1](research/2026-06-14-3-focused-re-review-of-r6-s1.md) | 2026-06-14 | Focused re-review of R6-S1 freeze fix against 2 kill criteria (Reset freeze closed, no new freeze/missed-emit edge), verdict APPROVE | Focused re-review R6-S1 freeze fix |
| [2026-06-14-3-focused-review-of-pr-1418-r6](research/2026-06-14-3-focused-review-of-pr-1418-r6.md) | 2026-06-14 | Focused review of PR #1418 (R6-S2 whole-value key gating) verifying feed-refactor transparency, per-key freeze guard, and frame-identity ordering; verdict APPROVE-WITH-NITS (file-size blocker) | a4b39778c6d1cac71 |
| [2026-06-14-3-opus-review-of-r3-s4-android](research/2026-06-14-3-opus-review-of-r3-s4-android.md) | 2026-06-14 | Opus review of R3-S4 Android ProjectionCache interposer: decodeSucceeds divergence illusory (both platforms commit non-empty corrupt payloads), D3-4 honored end-to-end, all gates green, verdict APPROVE-WITH-NITS | Opus review R3-S4 PR1410 |
| [2026-06-14-3-r6-s4-capstone-4-hard-pass](research/2026-06-14-3-r6-s4-capstone-4-hard-pass.md) | 2026-06-14 | R6-S4 capstone: 4 hard PASS/FAIL gates measuring idle feed bytes (45,440→1,104 B, 97.6% reduction), byte-identity oracle, and false-resend rate across two-phase measurement — all PASS | Implement R6-S4 capstone measurement |
| [2026-06-14-3-r6-s4-feed-idle-capstone-measurement](research/2026-06-14-3-r6-s4-feed-idle-capstone-measurement.md) | 2026-06-14 | R6-S4 feed-idle capstone measurement: two-phase benchmark of idle feed bytes ON vs OFF with 4 hard PASS/FAIL gates; 97.6% reduction (45,440→1,104 B), all 4 gates PASS | Implement R6-S4 capstone measurement (a36e4107cc809b311) |
| [2026-06-14-3-s6-capstone-harness-results-all-4](research/2026-06-14-3-s6-capstone-harness-results-all-4.md) | 2026-06-14 | S6 capstone harness results: all 4 gates PASS (row_suppression 68.8%, frame bytes −18%, no encode regression, byte-identity oracle 0.0 mismatches) | Implement R3-S5 S6 capstone subagent |
| [2026-06-14-3-tier-1-feed-idle-waste-measurement](research/2026-06-14-3-tier-1-feed-idle-waste-measurement.md) | 2026-06-14 | Tier-1 feed idle-waste measurement: feed re-serializes byte-identical 58.8 KB every 4Hz idle tick (~6× rest of frame), release 129µs vs debug 2266µs (17.6× factor), verdict YES feed is unambiguous waste | a85df4db283078c1e |
| [2026-06-14-4-adversarial-review-of-pr-1393-cleared](research/2026-06-14-4-adversarial-review-of-pr-1393-cleared.md) | 2026-06-14 | Adversarial review of PR 1393 Cleared-signal fix; verdict REQUEST-CHANGES, regression test proven genuinely non-vacuous, perpetual-Changed byte leak found in note_copy_emit non-empty arm | Opus review R3-S1b PR1393 |
| [2026-06-14-4-adversarial-review-of-pr-1410-android](research/2026-06-14-4-adversarial-review-of-pr-1410-android.md) | 2026-06-14 | Adversarial review of PR #1410 Android ProjectionCache interposer — verdict APPROVE-WITH-NITS; D3-4 decode-before-commit parity confirmed, decodeSucceeds divergence illusory | Opus review R3-S4 PR1410 |
| [2026-06-14-4-adversarial-review-of-s6-capstone-pr](research/2026-06-14-4-adversarial-review-of-s6-capstone-pr.md) | 2026-06-14 | Adversarial review of S6 capstone (PR #1413); verdict REQUEST-CHANGES — metric swap is principled but docstring overclaims (~81%→<5% is false, real win is ~18% frame-byte + 68.8% row suppression), file-size violations, oracle only end-state not per-tick | Opus review R3-S5 capstone PR1413 subagent |
| [2026-06-14-4-r3-s5-s6-capstone-empirical-harness](research/2026-06-14-4-r3-s5-s6-capstone-empirical-harness.md) | 2026-06-14 | R3-S5 S6 capstone empirical harness: 4 gates PASS (row_suppression 68.8% ≥ 0.50 threshold, frame p50 7928B vs 9640B baseline, serialize p50 61µs ≤ 75µs tolerance, byte-identity oracle 0.0) | Implement R3-S5 S6 capstone |
| [2026-06-14-4-r6-s4-capstone-review-reproduced-97](research/2026-06-14-4-r6-s4-capstone-review-reproduced-97.md) | 2026-06-14 | R6-S4 capstone review: reproduced 97.6% result, identified false-resend gate tests trivial case (stranger pubkey rejected by predicate, not followed out-of-window event) — verdict REQUEST-CHANGES | Review R6-S4 capstone (local git) |
| [2026-06-14-4-review-of-r6-s2-whole-value](research/2026-06-14-4-review-of-r6-s2-whole-value.md) | 2026-06-14 | Review of R6-S2 whole-value key gating generalization against 4 criteria (refactor transparency, per-key freeze guard, frame-identity ordering, scope discipline), verdict APPROVE-WITH-NITS | Focused review R6-S2 PR1418 |
| [2026-06-14-4-review-of-r6-s4-capstone-reproduced](research/2026-06-14-4-review-of-r6-s4-capstone-reproduced.md) | 2026-06-14 | Review of R6-S4 capstone: reproduced 97.6% headline, found false-resend gate tests trivial case (stranger pubkey rejected by predicate) instead of real over-invalidation risk (followed out-of-window event); verdict REQUEST-CHANGES | Review R6-S4 capstone (a6b8549e3fbef8781) |
| [2026-06-14-5-adversarial-review-of-pr-1409-ios](research/2026-06-14-5-adversarial-review-of-pr-1409-ios.md) | 2026-06-14 | Adversarial review of PR 1409 iOS ProjectionCache interposer; verdict REQUEST-CHANGES, iOS build fails (missing FlatBuffers import in KernelBridge), merge algorithm verified correct | Opus review R3-S3 PR1409 |
| [2026-06-14-5-empirical-capstone-measurement-of-idle-feed](research/2026-06-14-5-empirical-capstone-measurement-of-idle-feed.md) | 2026-06-14 | Empirical capstone measurement of idle feed byte reduction with 4 pre-registered hard gates, 97.6% reduction (45,440→1,104 B), all gates PASS | Implement R6-S4 capstone measurement |

## Episode Cards (124 cards)

| Card | Date | Title | Salience | Status |
|------|------|-------|----------|--------|
| [2026-06-13-1-android-compose-profile-claim-churn-loop](episodes/2026-06-13-1-android-compose-profile-claim-churn-loop.md) | 2026-06-13 | Android Compose profile-claim churn loop — same bug class as chirp-web | root-cause | active |
| [2026-06-13-1-chirp-reference-app-requires-full-parity](episodes/2026-06-13-1-chirp-reference-app-requires-full-parity.md) | 2026-06-13 | Chirp reference app requires full parity across all 3 platforms | product | active |
| [2026-06-13-1-don-t-restore-pbxproj-after-xcodegen](episodes/2026-06-13-1-don-t-restore-pbxproj-after-xcodegen.md) | 2026-06-13 | Don't restore pbxproj after xcodegen on device builds | architecture | superseded |
| [2026-06-13-1-ios-device-builds-must-keep-xcodegen](episodes/2026-06-13-1-ios-device-builds-must-keep-xcodegen.md) | 2026-06-13 | iOS device builds must keep xcodegen-generated pbxproj, not restore from git | root-cause | active |
| [2026-06-13-1-k1-signer-session-capability-port-replaces](episodes/2026-06-13-1-k1-signer-session-capability-port-replaces.md) | 2026-06-13 | K1: Signer-session capability port replaces SignerForSeal | architecture | superseded |
| [2026-06-13-1-mls-cross-platform-validated-end-to](episodes/2026-06-13-1-mls-cross-platform-validated-end-to.md) | 2026-06-13 | MLS cross-platform validated end-to-end on real devices | product | active |
| [2026-06-13-1-mls-end-to-end-validated-on](episodes/2026-06-13-1-mls-end-to-end-validated-on.md) | 2026-06-13 | MLS end-to-end validated on both platforms — Rust-owns-all architecture confirmed | product | superseded |
| [2026-06-13-1-needs-decision-backlog-resolved-by-documented](episodes/2026-06-13-1-needs-decision-backlog-resolved-by-documented.md) | 2026-06-13 | Needs-decision backlog resolved by documented direction (10/11) | architecture | active |
| [2026-06-13-1-post-restart-mls-group-messages-never](episodes/2026-06-13-1-post-restart-mls-group-messages-never.md) | 2026-06-13 | Post-restart MLS group messages never arrive — resubscribe all groups on register | root-cause | active |
| [2026-06-13-1-projection-emission-default-inverted-incremental-by](episodes/2026-06-13-1-projection-emission-default-inverted-incremental-by.md) | 2026-06-13 | Projection emission default inverted: incremental-by-default, snapshot-as-resync | reversal | active |
| [2026-06-13-1-remove-flatbuffers-verifier-on-trusted-in](episodes/2026-06-13-1-remove-flatbuffers-verifier-on-trusted-in.md) | 2026-06-13 | Remove FlatBuffers Verifier on trusted in-process decode path | architecture | active |
| [2026-06-13-1-signer-session-capability-port-replaces-ambient](episodes/2026-06-13-1-signer-session-capability-port-replaces-ambient.md) | 2026-06-13 | Signer-session capability port replaces ambient signer authority (K1/ADR-0050) | architecture | active |
| [2026-06-13-1-signer-session-capability-port-replaces-signerforseal](episodes/2026-06-13-1-signer-session-capability-port-replaces-signerforseal.md) | 2026-06-13 | Signer-session capability port replaces SignerForSeal thread cluster (ADR-0050 / K1) | architecture | superseded |
| [2026-06-13-1-since-none-watermark-exemption-made-lifecycle](episodes/2026-06-13-1-since-none-watermark-exemption-made-lifecycle.md) | 2026-06-13 | since=None watermark exemption made lifecycle-aware | product | active |
| [2026-06-13-1-since-none-watermark-exemption-must-be](episodes/2026-06-13-1-since-none-watermark-exemption-must-be.md) | 2026-06-13 | since=None watermark exemption must be lifecycle-aware (backfill exempt, Tailing narrowed) | product | superseded |
| [2026-06-13-1-since-none-watermark-exemption-refined-to](episodes/2026-06-13-1-since-none-watermark-exemption-refined-to.md) | 2026-06-13 | since=None watermark exemption refined to lifecycle-aware | root-cause | superseded |
| [2026-06-13-1-since-none-watermark-rewrite-exemption-refined](episodes/2026-06-13-1-since-none-watermark-rewrite-exemption-refined.md) | 2026-06-13 | since=None watermark rewrite exemption refined to lifecycle-aware | product | superseded |
| [2026-06-13-1-store-eviction-ceiling-re-enabled-with](episodes/2026-06-13-1-store-eviction-ceiling-re-enabled-with.md) | 2026-06-13 | Store eviction ceiling re-enabled with floor-coherent pinning (#1090) | product | superseded |
| [2026-06-13-1-surface-marmot-registration-requirement-on-disabled](episodes/2026-06-13-1-surface-marmot-registration-requirement-on-disabled.md) | 2026-06-13 | Surface Marmot registration requirement on disabled Create Group button | product | active |
| [2026-06-13-1-systematic-agent-produced-file-size-debt](episodes/2026-06-13-1-systematic-agent-produced-file-size-debt.md) | 2026-06-13 | Systematic agent-produced file-size debt — enforce split, ban baseline bumps | root-cause | active |
| [2026-06-13-1-thin-shell-d0-doctrine-9-confirmed](episodes/2026-06-13-1-thin-shell-d0-doctrine-9-confirmed.md) | 2026-06-13 | Thin-shell D0 doctrine: 9 confirmed UI-logic violations cataloged for Wave 2 fix | architecture | active |
| [2026-06-13-2-1281-exempt-since-none-interests-from](episodes/2026-06-13-2-1281-exempt-since-none-interests-from.md) | 2026-06-13 | #1281: exempt since=None interests from T129 watermark rewrite | product | superseded |
| [2026-06-13-2-4hz-full-kernel-snapshot-cycle-is](episodes/2026-06-13-2-4hz-full-kernel-snapshot-cycle-is.md) | 2026-06-13 | 4Hz full-kernel-snapshot cycle is the performance bottleneck on physical device | root-cause | superseded |
| [2026-06-13-2-d0-thin-shell-violation-resolved-embed](episodes/2026-06-13-2-d0-thin-shell-violation-resolved-embed.md) | 2026-06-13 | D0 thin-shell violation resolved — embed projection moves to nmp-ffi (#1283) | architecture | superseded |
| [2026-06-13-2-embedhost-resolution-migrates-from-swift-to](episodes/2026-06-13-2-embedhost-resolution-migrates-from-swift-to.md) | 2026-06-13 | EmbedHost resolution migrates from Swift to Rust (D0 thin-shell) | architecture | active |
| [2026-06-13-2-excellence-program-deletion-first-doctrine-and](episodes/2026-06-13-2-excellence-program-deletion-first-doctrine-and.md) | 2026-06-13 | Excellence program: deletion-first doctrine and scope exclusions | architecture | superseded |
| [2026-06-13-2-file-size-gate-enforced-as-debt](episodes/2026-06-13-2-file-size-gate-enforced-as-debt.md) | 2026-06-13 | File-size gate enforced as debt barrier: decompose into siblings, never bump baselines | architecture | superseded |
| [2026-06-13-2-file-size-hard-cap-enforced-as](episodes/2026-06-13-2-file-size-hard-cap-enforced-as.md) | 2026-06-13 | File-size hard cap enforced as zero-tolerance: always split, never baseline-bump | architecture | superseded |
| [2026-06-13-2-flatbuffers-verifier-removed-on-trusted-in](episodes/2026-06-13-2-flatbuffers-verifier-removed-on-trusted-in.md) | 2026-06-13 | FlatBuffers Verifier removed on trusted in-process decode path | architecture | superseded |
| [2026-06-13-2-host-declared-projections-shipped-with-three](episodes/2026-06-13-2-host-declared-projections-shipped-with-three.md) | 2026-06-13 | Host-declared projections shipped with three debts: silent footgun, unbuilt drift gate, unenforced init-only invariant | root-cause | active |
| [2026-06-13-2-instance-scoped-registration-replaces-ambient-authority](episodes/2026-06-13-2-instance-scoped-registration-replaces-ambient-authority.md) | 2026-06-13 | Instance-scoped registration replaces ambient-authority globals (K2/ADR-0052) | architecture | active |
| [2026-06-13-2-prohibit-pre-formatted-relative-time-strings](episodes/2026-06-13-2-prohibit-pre-formatted-relative-time-strings.md) | 2026-06-13 | Prohibit pre-formatted relative-time strings in projection builders | product | active |
| [2026-06-13-2-relay-diagnostics-must-emit-raw-timestamps](episodes/2026-06-13-2-relay-diagnostics-must-emit-raw-timestamps.md) | 2026-06-13 | Relay diagnostics must emit raw timestamps, not formatted relative-time strings | reversal | superseded |
| [2026-06-13-2-snapshot-path-performance-defects-relay-diagnostics](episodes/2026-06-13-2-snapshot-path-performance-defects-relay-diagnostics.md) | 2026-06-13 | Snapshot-path performance defects: relay-diagnostics time-string churn and FlatBuffers Verifier on trusted data | root-cause | superseded |
| [2026-06-13-2-standing-doctrine-never-hedge-on-breaking](episodes/2026-06-13-2-standing-doctrine-never-hedge-on-breaking.md) | 2026-06-13 | Standing doctrine: never hedge on breaking changes — upgrade consumers by hand | architecture | active |
| [2026-06-13-2-store-eviction-floor-coherent-pins-ceiling](episodes/2026-06-13-2-store-eviction-floor-coherent-pins-ceiling.md) | 2026-06-13 | Store eviction: floor-coherent pins + ceiling re-enabled + watermark machinery deleted | architecture | superseded |
| [2026-06-13-2-supersession-deletion-doctrine-prevents-mechanism-fossil](episodes/2026-06-13-2-supersession-deletion-doctrine-prevents-mechanism-fossil.md) | 2026-06-13 | Supersession-deletion doctrine prevents mechanism fossil accumulation | architecture | active |
| [2026-06-13-3-adr-0053-invert-projection-emission-from](episodes/2026-06-13-3-adr-0053-invert-projection-emission-from.md) | 2026-06-13 | ADR-0053: Invert projection emission from full-snapshot-default to incremental-default | reversal | superseded |
| [2026-06-13-3-chirp-scope-full-feature-parity-across](episodes/2026-06-13-3-chirp-scope-full-feature-parity-across.md) | 2026-06-13 | Chirp scope: full feature parity across all 3 platforms | product | active |
| [2026-06-13-3-doctrine-never-hedge-on-breaking-changes](episodes/2026-06-13-3-doctrine-never-hedge-on-breaking-changes.md) | 2026-06-13 | Doctrine: never hedge on breaking changes — land and upgrade consumers | architecture | superseded |
| [2026-06-13-3-embedhost-resolution-moves-from-per-platform](episodes/2026-06-13-3-embedhost-resolution-moves-from-per-platform.md) | 2026-06-13 | EmbedHost resolution moves from per-platform Swift to Rust FFI sidecar | architecture | superseded |
| [2026-06-13-3-flatbuffers-verifier-dropped-on-trusted-in](episodes/2026-06-13-3-flatbuffers-verifier-dropped-on-trusted-in.md) | 2026-06-13 | FlatBuffers Verifier dropped on trusted in-process snapshot decode path | architecture | superseded |
| [2026-06-13-3-floor-coherent-eviction-replaces-dead-watermark](episodes/2026-06-13-3-floor-coherent-eviction-replaces-dead-watermark.md) | 2026-06-13 | Floor-coherent eviction replaces dead watermark machinery | architecture | active |
| [2026-06-13-3-nmp-blossom-and-nmp-nip60-parked](episodes/2026-06-13-3-nmp-blossom-and-nmp-nip60-parked.md) | 2026-06-13 | nmp-blossom and nmp-nip60 parked as post-v1 dead islands | reversal | superseded |
| [2026-06-13-3-read-your-writes-for-follows-via](episodes/2026-06-13-3-read-your-writes-for-follows-via.md) | 2026-06-13 | Read-your-writes for follows via single acquisition path | product | active |
| [2026-06-13-3-relay-diagnostics-pre-formatted-relative-time](episodes/2026-06-13-3-relay-diagnostics-pre-formatted-relative-time.md) | 2026-06-13 | Relay-diagnostics pre-formatted relative-time strings removed (aim.md §62 violation) | product | superseded |
| [2026-06-13-3-relay-diagnostics-timestamp-fix-trades-per](episodes/2026-06-13-3-relay-diagnostics-timestamp-fix-trades-per.md) | 2026-06-13 | Relay-diagnostics timestamp fix trades per-second churn for per-millisecond churn — requires deterministic wall-clock anchor | root-cause | active |
| [2026-06-13-3-timelineitem-naive-move-would-create-crate](episodes/2026-06-13-3-timelineitem-naive-move-would-create-crate.md) | 2026-06-13 | TimelineItem naive move would create crate cycle — envelope-cut is the right shape (#920) | root-cause | superseded |
| [2026-06-13-4-10-of-11-needs-decision-issues](episodes/2026-06-13-4-10-of-11-needs-decision-issues.md) | 2026-06-13 | 10 of 11 needs-decision issues already determined by documented direction | workflow | superseded |
| [2026-06-13-4-920-naive-timelineitem-move-creates-crate](episodes/2026-06-13-4-920-naive-timelineitem-move-creates-crate.md) | 2026-06-13 | #920: naive TimelineItem move creates crate cycle — envelope-cut is the correct fix | root-cause | active |
| [2026-06-13-4-auth-relay-publishes-park-via-availability](episodes/2026-06-13-4-auth-relay-publishes-park-via-availability.md) | 2026-06-13 | AUTH relay publishes park via availability gate instead of failing | product | active |
| [2026-06-13-4-embed-resolution-migrated-from-swift-embedhost](episodes/2026-06-13-4-embed-resolution-migrated-from-swift-embedhost.md) | 2026-06-13 | Embed resolution migrated from Swift EmbedHost to Rust nmp-ffi sidecar | architecture | superseded |
| [2026-06-13-4-full-snapshot-per-tick-model-superseded](episodes/2026-06-13-4-full-snapshot-per-tick-model-superseded.md) | 2026-06-13 | Full-snapshot-per-tick model superseded by incremental projection emission (ADR-0053) | reversal | superseded |
| [2026-06-13-4-post-v1-dead-crates-parked-as](episodes/2026-06-13-4-post-v1-dead-crates-parked-as.md) | 2026-06-13 | Post-v1 dead crates parked as historical | reversal | superseded |
| [2026-06-13-4-supersede-adr-0039-allow-host-declared](episodes/2026-06-13-4-supersede-adr-0039-allow-host-declared.md) | 2026-06-13 | Supersede ADR-0039: Allow host-declared projection interest (rejecting the blanket prohibition) | reversal | active |
| [2026-06-13-5-host-declared-projection-consumption-supersedes-adr](episodes/2026-06-13-5-host-declared-projection-consumption-supersedes-adr.md) | 2026-06-13 | Host-declared projection consumption supersedes ADR-0039's blanket prohibition | reversal | superseded |
| [2026-06-13-5-pubkey-only-identity-accessor-enables-bunker](episodes/2026-06-13-5-pubkey-only-identity-accessor-enables-bunker.md) | 2026-06-13 | Pubkey-only identity accessor enables bunker account runtimes | product | active |
| [2026-06-13-5-wasm-publish-path-surfaces-honest-error](episodes/2026-06-13-5-wasm-publish-path-surfaces-honest-error.md) | 2026-06-13 | Wasm publish path surfaces honest error instead of silent drop | product | active |
| [2026-06-14-1-adr-0055-omit-unchanged-cleared-signal](episodes/2026-06-14-1-adr-0055-omit-unchanged-cleared-signal.md) | 2026-06-14 | ADR-0055 omit-Unchanged Cleared-signal defect — flagged as blocker, not self-patched | architecture | superseded |
| [2026-06-14-1-adr-0055-r3-capstone-metric-swap](episodes/2026-06-14-1-adr-0055-r3-capstone-metric-swap.md) | 2026-06-14 | ADR-0055 R3 Capstone: Metric swap from waste_ratio to row_suppression_ratio | architecture | superseded |
| [2026-06-14-1-adr-0055-r3-s1b-cleared-signal](episodes/2026-06-14-1-adr-0055-r3-s1b-cleared-signal.md) | 2026-06-14 | ADR-0055 R3-S1b: Cleared-signal completeness for conditional projections | architecture | superseded |
| [2026-06-14-1-ambient-authority-eliminated-five-globals-and](episodes/2026-06-14-1-ambient-authority-eliminated-five-globals-and.md) | 2026-06-14 | Ambient authority eliminated: five globals and kernel_mut replaced by narrow capability traits with D21 regression gate | architecture | superseded |
| [2026-06-14-1-ambient-authority-instance-scoped-capability-model](episodes/2026-06-14-1-ambient-authority-instance-scoped-capability-model.md) | 2026-06-14 | Ambient authority → instance-scoped capability model (K2) | architecture | active |
| [2026-06-14-1-blossom-reclassified-from-parked-dead-island](episodes/2026-06-14-1-blossom-reclassified-from-parked-dead-island.md) | 2026-06-14 | Blossom reclassified from parked dead-island to v1 workspace member | reversal | superseded |
| [2026-06-14-1-blossom-reclassified-from-parked-post-v1](episodes/2026-06-14-1-blossom-reclassified-from-parked-post-v1.md) | 2026-06-14 | Blossom reclassified from parked/post-v1 to active v1 workspace member | reversal | superseded |
| [2026-06-14-1-blossom-un-parked-from-dead-island](episodes/2026-06-14-1-blossom-un-parked-from-dead-island.md) | 2026-06-14 | Blossom un-parked from dead-island to v1 workspace member | reversal | active |
| [2026-06-14-1-cleared-signal-byte-leak-and-source](episodes/2026-06-14-1-cleared-signal-byte-leak-and-source.md) | 2026-06-14 | Cleared-signal byte leak and source-version bump (R3-S1b) | root-cause | active |
| [2026-06-14-1-cleared-signal-completeness-for-incremental-emission](episodes/2026-06-14-1-cleared-signal-completeness-for-incremental-emission.md) | 2026-06-14 | Cleared-signal completeness for incremental emission (#1390) | root-cause | superseded |
| [2026-06-14-1-coverage-ledger-replaces-presence-heuristic-as](episodes/2026-06-14-1-coverage-ledger-replaces-presence-heuristic-as.md) | 2026-06-14 | Coverage ledger replaces presence heuristic as sole floor source | product | active |
| [2026-06-14-1-feed-byte-equality-gating-shipped-idle](episodes/2026-06-14-1-feed-byte-equality-gating-shipped-idle.md) | 2026-06-14 | Feed byte-equality gating shipped; idle-jank cause refuted; row-deltas closed | root-cause | active |
| [2026-06-14-1-feed-emission-freezes-on-reset-rebaseline](episodes/2026-06-14-1-feed-emission-freezes-on-reset-rebaseline.md) | 2026-06-14 | Feed emission freezes on Reset — rebaseline keyed on host's actual reset signal | root-cause | superseded |
| [2026-06-14-1-feed-projection-becomes-trap-proof-omitting](episodes/2026-06-14-1-feed-projection-becomes-trap-proof-omitting.md) | 2026-06-14 | Feed projection becomes trap-proof omitting projection via byte-fingerprint + FrameIdentity | architecture | superseded |
| [2026-06-14-1-k2-ambient-authority-eliminated-narrow-capability](episodes/2026-06-14-1-k2-ambient-authority-eliminated-narrow-capability.md) | 2026-06-14 | K2: Ambient authority eliminated — narrow capability traits replace globals + kernel_mut | architecture | superseded |
| [2026-06-14-1-nmp-blossom-reclassified-from-parked-post](episodes/2026-06-14-1-nmp-blossom-reclassified-from-parked-post.md) | 2026-06-14 | nmp-blossom reclassified from parked/post-v1 to active v1 workspace member | reversal | active |
| [2026-06-14-1-note-copy-emit-perpetual-changed-byte](episodes/2026-06-14-1-note-copy-emit-perpetual-changed-byte.md) | 2026-06-14 | note_copy_emit perpetual-Changed byte leak fix (R3-S1b) | root-cause | superseded |
| [2026-06-14-1-note-copy-emit-perpetual-changed-re](episodes/2026-06-14-1-note-copy-emit-perpetual-changed-re.md) | 2026-06-14 | note_copy_emit perpetual-Changed re-emission bug fixed | root-cause | superseded |
| [2026-06-14-1-parked-crates-must-be-their-own](episodes/2026-06-14-1-parked-crates-must-be-their-own.md) | 2026-06-14 | Parked crates must be their own workspace roots — Cargo auto-discovery binds excluded crates to the parent | root-cause | superseded |
| [2026-06-14-1-presence-heuristic-replaced-by-per-filter](episodes/2026-06-14-1-presence-heuristic-replaced-by-per-filter.md) | 2026-06-14 | Presence heuristic replaced by per-(filter, relay) coverage ledger as sole floor source | product | superseded |
| [2026-06-14-1-presence-is-not-coverage-coverage-ledger](episodes/2026-06-14-1-presence-is-not-coverage-coverage-ledger.md) | 2026-06-14 | Presence-is-not-coverage: coverage ledger replaces presence-floor (K3) | architecture | superseded |
| [2026-06-14-1-presence-to-coverage-ledger-migration-k3](episodes/2026-06-14-1-presence-to-coverage-ledger-migration-k3.md) | 2026-06-14 | Presence-to-coverage ledger migration (K3) | architecture | superseded |
| [2026-06-14-1-projectioncache-interposer-single-decode-both-platforms](episodes/2026-06-14-1-projectioncache-interposer-single-decode-both-platforms.md) | 2026-06-14 | ProjectionCache interposer: single-decode, both platforms, same D3-4 floor | architecture | active |
| [2026-06-14-1-rung-3-byte-savings-reframed-18](episodes/2026-06-14-1-rung-3-byte-savings-reframed-18.md) | 2026-06-14 | Rung 3 byte-savings reframed: 18% not 81% | root-cause | active |
| [2026-06-14-1-rung-3-delivers-18-byte-reduction](episodes/2026-06-14-1-rung-3-delivers-18-byte-reduction.md) | 2026-06-14 | Rung 3 delivers ~18% byte reduction, not ~81% — Tier-1 feed dominates frame budget | reversal | superseded |
| [2026-06-14-1-rung-3-metric-swap-waste-ratio](episodes/2026-06-14-1-rung-3-metric-swap-waste-ratio.md) | 2026-06-14 | Rung 3 metric swap — waste_ratio replaced by row_suppression_ratio as acceptance gate | architecture | active |
| [2026-06-14-1-trap-proof-incremental-emission-for-all](episodes/2026-06-14-1-trap-proof-incremental-emission-for-all.md) | 2026-06-14 | Trap-proof incremental emission for all Tier-1 projections | architecture | active |
| [2026-06-14-1-trap-proof-projection-emission-with-frameidentity](episodes/2026-06-14-1-trap-proof-projection-emission-with-frameidentity.md) | 2026-06-14 | Trap-proof projection emission with FrameIdentity reset signal | architecture | superseded |
| [2026-06-14-1-trap-proof-projection-omit-with-reset](episodes/2026-06-14-1-trap-proof-projection-omit-with-reset.md) | 2026-06-14 | Trap-proof projection omit with Reset-safe FrameIdentity rebaseline | root-cause | superseded |
| [2026-06-14-2-adr-0055-r3-s3-ios-projectioncache](episodes/2026-06-14-2-adr-0055-r3-s3-ios-projectioncache.md) | 2026-06-14 | ADR-0055 R3-S3: iOS ProjectionCache interposer — first host to enable incremental_apply | architecture | superseded |
| [2026-06-14-2-android-production-identity-persistence-keyring-restore](episodes/2026-06-14-2-android-production-identity-persistence-keyring-restore.md) | 2026-06-14 | Android production identity persistence — keyring + restore must be unconditional, not DEBUG-gated | product | active |
| [2026-06-14-2-byte-identity-oracle-hardened-to-fail](episodes/2026-06-14-2-byte-identity-oracle-hardened-to-fail.md) | 2026-06-14 | Byte-identity oracle hardened to fail-closed on unexpected absent keys | root-cause | active |
| [2026-06-14-2-cargo-workspace-auto-discovery-breaks-parked](episodes/2026-06-14-2-cargo-workspace-auto-discovery-breaks-parked.md) | 2026-06-14 | Cargo workspace auto-discovery breaks parked crates with inherited fields | root-cause | superseded |
| [2026-06-14-2-d3-4-decode-before-commit-parity](episodes/2026-06-14-2-d3-4-decode-before-commit-parity.md) | 2026-06-14 | D3-4 decode-before-commit parity: both platforms reject empty only | root-cause | active |
| [2026-06-14-2-excluded-crates-must-be-self-contained](episodes/2026-06-14-2-excluded-crates-must-be-self-contained.md) | 2026-06-14 | Excluded crates must be self-contained — workspace inheritance breaks standalone resolution | root-cause | superseded |
| [2026-06-14-2-external-nmp-consumers-already-on-post](episodes/2026-06-14-2-external-nmp-consumers-already-on-post.md) | 2026-06-14 | External NMP consumers already on post-keystone API — no migration needed | root-cause | active |
| [2026-06-14-2-feed-emission-gating-via-m1-byte](episodes/2026-06-14-2-feed-emission-gating-via-m1-byte.md) | 2026-06-14 | Feed emission gating via M1 byte-fingerprint with FrameIdentity reset safety | architecture | superseded |
| [2026-06-14-2-feed-engine-does-not-follow-gate](episodes/2026-06-14-2-feed-engine-does-not-follow-gate.md) | 2026-06-14 | Feed engine does NOT follow-gate roots — only replies | root-cause | active |
| [2026-06-14-2-feed-idle-omission-validated-at-97](episodes/2026-06-14-2-feed-idle-omission-validated-at-97.md) | 2026-06-14 | Feed idle omission validated at 97.6%; OP-centric root ingestion not follow-gated | root-cause | superseded |
| [2026-06-14-2-feed-idle-waste-measurement-58-8](episodes/2026-06-14-2-feed-idle-waste-measurement-58-8.md) | 2026-06-14 | Feed idle-waste measurement — ~58.8 KB byte-identical payload re-serialized every idle tick | root-cause | superseded |
| [2026-06-14-2-flatbufferbuilder-per-tick-reuse-replaces-allocation](episodes/2026-06-14-2-flatbufferbuilder-per-tick-reuse-replaces-allocation.md) | 2026-06-14 | FlatBufferBuilder per-tick reuse replaces allocation (ADR-0055 R3-S2) | architecture | active |
| [2026-06-14-2-ios-double-decode-per-tick-waste](episodes/2026-06-14-2-ios-double-decode-per-tick-waste.md) | 2026-06-14 | iOS double-decode per-tick waste eliminated — session/epoch threaded from single decode (R3-S3) | architecture | superseded |
| [2026-06-14-2-ios-projectioncache-interposer-single-decode-architecture](episodes/2026-06-14-2-ios-projectioncache-interposer-single-decode-architecture.md) | 2026-06-14 | iOS ProjectionCache interposer — single-decode architecture, no per-frame buffer re-parse | architecture | active |
| [2026-06-14-2-ios-projectioncache-single-decode-architecture](episodes/2026-06-14-2-ios-projectioncache-single-decode-architecture.md) | 2026-06-14 | iOS ProjectionCache single-decode architecture | architecture | superseded |
| [2026-06-14-2-neg-open-reconciliation-un-floored-floor](episodes/2026-06-14-2-neg-open-reconciliation-un-floored-floor.md) | 2026-06-14 | NEG-OPEN reconciliation un-floored + floor soundness patches | product | superseded |
| [2026-06-14-2-neg-open-un-floor-full-window](episodes/2026-06-14-2-neg-open-un-floor-full-window.md) | 2026-06-14 | NEG-OPEN un-floor: full-window reconciliation for NIP-77 | product | active |
| [2026-06-14-2-option-b-feed-row-deltas-closed](episodes/2026-06-14-2-option-b-feed-row-deltas-closed.md) | 2026-06-14 | Option B (feed row-deltas) closed as not warranted | reversal | superseded |
| [2026-06-14-2-parked-crate-cargo-workspace-auto-discovery](episodes/2026-06-14-2-parked-crate-cargo-workspace-auto-discovery.md) | 2026-06-14 | Parked-crate Cargo workspace auto-discovery breaks inherited fields | root-cause | superseded |
| [2026-06-14-2-parked-crate-standalone-buildability-requires-empty](episodes/2026-06-14-2-parked-crate-standalone-buildability-requires-empty.md) | 2026-06-14 | Parked crate standalone-buildability requires empty [workspace] table (not just de-inherited fields) | root-cause | active |
| [2026-06-14-2-parked-crates-must-be-self-contained](episodes/2026-06-14-2-parked-crates-must-be-self-contained.md) | 2026-06-14 | Parked crates must be self-contained — workspace inheritance breaks excluded consumers | root-cause | superseded |
| [2026-06-14-2-projectioncache-codegen-generated-interposer-enables-incremental](episodes/2026-06-14-2-projectioncache-codegen-generated-interposer-enables-incremental.md) | 2026-06-14 | ProjectionCache codegen-generated interposer enables incremental emission on both mobile hosts | architecture | superseded |
| [2026-06-14-2-projectioncache-interposer-enables-incremental-apply-on](episodes/2026-06-14-2-projectioncache-interposer-enables-incremental-apply-on.md) | 2026-06-14 | ProjectionCache interposer enables incremental_apply on mobile | architecture | superseded |
| [2026-06-14-2-rung-3-delivers-18-frame-byte](episodes/2026-06-14-2-rung-3-delivers-18-frame-byte.md) | 2026-06-14 | Rung 3 delivers ~18% frame-byte reduction — Tier-1 feed gating is the remaining prize | root-cause | superseded |
| [2026-06-14-2-since-floor-migrating-from-presence-heuristic](episodes/2026-06-14-2-since-floor-migrating-from-presence-heuristic.md) | 2026-06-14 | Since-floor migrating from presence heuristic to per-(filter, relay) coverage ledger; presence hardened and single-sourced in transit | architecture | superseded |
| [2026-06-14-3-aim-md-amended-projections-incremental-by](episodes/2026-06-14-3-aim-md-amended-projections-incremental-by.md) | 2026-06-14 | aim.md amended: projections incremental-by-default for rev-aware hosts | architecture | active |
| [2026-06-14-3-android-d3-4-decodesucceeds-parity-doctrine](episodes/2026-06-14-3-android-d3-4-decodesucceeds-parity-doctrine.md) | 2026-06-14 | Android D3-4 decodeSucceeds parity doctrine — isNotEmpty() ruled acceptable (R3-S4) | architecture | superseded |
| [2026-06-14-3-android-decodesucceeds-parity-ruling-isnotempty-is](episodes/2026-06-14-3-android-decodesucceeds-parity-ruling-isnotempty-is.md) | 2026-06-14 | Android decodeSucceeds parity ruling — isNotEmpty() is equivalent D3-4 to iOS per-key decoder preflight | root-cause | superseded |
| [2026-06-14-3-android-mls-status-corrected-from-unwired](episodes/2026-06-14-3-android-mls-status-corrected-from-unwired.md) | 2026-06-14 | Android MLS status corrected from unwired to wired-with-gaps | root-cause | active |
| [2026-06-14-3-capstone-success-metric-redefined-from-waste](episodes/2026-06-14-3-capstone-success-metric-redefined-from-waste.md) | 2026-06-14 | Capstone success metric redefined from waste_ratio to row_suppression_ratio | product | superseded |
| [2026-06-14-3-feed-change-signal-mechanism-m1-fingerprint](episodes/2026-06-14-3-feed-change-signal-mechanism-m1-fingerprint.md) | 2026-06-14 | Feed change-signal mechanism — M1 fingerprint-of-encoded-bytes, trap-proof by construction | architecture | superseded |
| [2026-06-14-3-floor-predicate-single-sourced-drift-bug](episodes/2026-06-14-3-floor-predicate-single-sourced-drift-bug.md) | 2026-06-14 | Floor predicate single-sourced — drift bug found and eliminated | architecture | superseded |
| [2026-06-14-3-option-b-feed-row-deltas-not](episodes/2026-06-14-3-option-b-feed-row-deltas-not.md) | 2026-06-14 | Option B (feed row-deltas) not warranted — Rung-6-B ADR stays closed | reversal | superseded |
| [2026-06-14-3-podcast-player-adopts-0-7-2](episodes/2026-06-14-3-podcast-player-adopts-0-7-2.md) | 2026-06-14 | Podcast-player adopts 0.7.2 git-dep model over vendored-blossom strategy | direction | active |
| [2026-06-14-3-presence-floor-coverage-ledger-migration-k3](episodes/2026-06-14-3-presence-floor-coverage-ledger-migration-k3.md) | 2026-06-14 | Presence-floor → coverage-ledger migration (K3) | architecture | superseded |
| [2026-06-14-4-coverage-ledger-replaces-presence-as-since](episodes/2026-06-14-4-coverage-ledger-replaces-presence-as-since.md) | 2026-06-14 | Coverage ledger replaces presence as since-floor source-of-truth | reversal | superseded |
| [2026-06-14-4-feed-gating-m1-encoded-payload-fingerprint](episodes/2026-06-14-4-feed-gating-m1-encoded-payload-fingerprint.md) | 2026-06-14 | Feed gating: M1 encoded-payload fingerprint, not O(1) dirty counter (Rung 6) | architecture | superseded |

