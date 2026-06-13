# Wiki Index

> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.

Last updated: 2026-06-13

## cache-serve (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [cache-serve](cache-serve.md) | Cache-Serve | Cache-serve gates v1 as a v1-blocker; the universal mechanism is a single event-acquisition pipeline where store-serving is the first half and the network REQ i | capture | warm | 2026-06-13 | cache-serve |
| [interest-withdrawal](interest-withdrawal.md) | Interest Withdrawal | Interest IDs are deterministic (group_message_interest_id over group_id_hex + relay_url); the kernel de-dupes via registry push replacing the slot, making re-re | capture | warm | 2026-06-13 | cache-serve |

## capability-socket (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [capability-socket](capability-socket.md) | Capability Socket | The capability trampoline routes all non-external_signer namespaces synchronously to a Kotlin handler registered via nativeSetCapabilityHandler; the existing ex | capture | warm | 2026-06-13 | capability-socket |
| [signer-session-port](signer-session-port.md) | Signer-Session Port | The signer-session capability port (ADR-0050) generalizes the prior single-verb SignEventForAccount port into a signer-session capability covering sign, nip44_e | capture | warm | 2026-06-13 | capability-socket |

## chirp-migration (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-migration](chirp-migration.md) | Chirp Migration | The TUI Chirp shell uses NMP registry components for name (NostrProfileName), content (NostrContentView), avatar (NostrAvatar), and NIP-05 (NostrNip05Badge), ma | capture | warm | 2026-06-13 | chirp-migration |

## ci-gates (4 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [build-infrastructure](build-infrastructure.md) | Build Infrastructure | Disk space has been a recurring constraint (ENOSPC stalled agents three times) | capture | warm | 2026-06-13 | ci-gates |
| [ci-gates](ci-gates.md) | CI Gates | CI must include `cargo test -p nmp-app-template` and `cargo build --workspace --examples` in the test plan to prevent the example-compile gap class that let a ` | capture | warm | 2026-06-13 | ci-gates |
| [model-substitution](model-substitution.md) | Model Substitution | The fable-5 model is unavailable and all keystone teams now run on Opus/Sonnet instead. | capture | warm | 2026-06-13 | ci-gates |
| [wasm-publish](wasm-publish.md) | WASM Publish | Issue #1202 (wasm silent publish failure) is resolved by replacing the silent NoTargets with an honest CapabilityFailure (`publish_not_supported_in_web_preview_ | capture | warm | 2026-06-13 | ci-gates |

## codebase-patterns (6 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [agents-md-policy](agents-md-policy.md) | AGENTS.md Policy | The week-one act before any keystone is landing the supersession-deletion policy in AGENTS.md plus the empty mechanism_census test. | capture | warm | 2026-06-13 | codebase-patterns |
| [codebase-patterns](codebase-patterns.md) | Codebase Patterns | The file-size gate enforces a 500-line hard cap with an anti-cheat rule that blocks raising a file's baseline in a PR; zero baseline bumps were merged across th | capture | warm | 2026-06-13 | codebase-patterns |
| [excellence-program](excellence-program.md) | Excellence Program | The excellence program identifies six repo-wide patterns found by the reviewers: superseded generations never deleted, presence-is-not-coverage, invariants by c | capture | warm | 2026-06-13 | codebase-patterns |
| [git-worktree-policy](git-worktree-policy.md) | Git Worktree Policy | Agents must work in isolated git worktrees, never moving the base repo away from master. | capture | warm | 2026-06-13 | codebase-patterns |
| [keystone-overview](keystone-overview.md) | Keystone Overview | The three keystones are K1 (signer-session port covering sign\|nip44_encrypt\|nip44_decrypt with mailbox completions), K2 (instance-scoped registration replacing | capture | warm | 2026-06-13 | codebase-patterns |
| [shell-protocol-violations](shell-protocol-violations.md) | Shell Protocol Violations | Issue #1283 is resolved with the EmbedHost resolver moving to nmp-ffi (which sits above both nmp-core and nmp-content in the DAG), shipping a typed EmbedKindPro | capture | warm | 2026-06-13 | codebase-patterns |

## concurrency (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [concurrency-ordering](concurrency-ordering.md) | Concurrency Ordering | All Relaxed orderings on the cancel flag were replaced with Release/Acquire pairs for correctness on ARM. | capture | warm | 2026-06-13 | concurrency |
| [dispatcher-shutdown](dispatcher-shutdown.md) | Dispatcher Shutdown | The dispatcher thread is joined in `cancel()` with a race guard to prevent thread leaks | capture | warm | 2026-06-13 | concurrency |
| [network-pool-timeout](network-pool-timeout.md) | Network Pool Timeout | Network pool connection must use `TcpStream::connect_timeout` (10 s) and `client_tls_with_config` to prevent shutdown blocking for ~75 seconds on black-hole hos | capture | warm | 2026-06-13 | concurrency |

## cron-create (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [cron-create](cron-create.md) | Cron Create | Recurring CronCreate tasks auto-expire after 7 days. | capture | warm | 2026-06-13 | cron-create |

## event-acquisition (6 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [coverage-ledger](coverage-ledger.md) | Coverage Ledger | The coverage ledger (K3) is gated behind a full K2 landing on master, per the user's sequential ordering | capture | warm | 2026-06-13 | event-acquisition |
| [event-acquisition](event-acquisition.md) | Event Acquisition | There is a single event-acquisition mechanism: serving from the local store is its first half, the planner's wire REQ is its refinement half, running through th | capture | warm | 2026-06-13 | event-acquisition |
| [feed-pagination](feed-pagination.md) | Feed Pagination | Feed pagination caps at MAX_FEED_WINDOW_LIMIT (500) | capture | warm | 2026-06-13 | event-acquisition |
| [kernel-timestamp-clamp](kernel-timestamp-clamp.md) | Kernel Timestamp Clamp | Kernel fan-out must clamp `created_at` on `KernelEvent` to `now_secs` for the observer-visible timestamp while preserving the wire timestamp in `StoredEvent` fo | capture | warm | 2026-06-13 | event-acquisition |
| [tag-ingest](tag-ingest.md) | Tag Ingest | Malformed elements in tag loops are skipped rather than treated as fatal errors. | capture | warm | 2026-06-13 | event-acquisition |
| [watermark-removal](watermark-removal.md) | Watermark Removal | The live since-floor for REQ subscriptions is derived from store content (newest matching event per author/coord/tag) via watermark_fn, not from persisted Water | capture | warm | 2026-06-13 | event-acquisition |

## kernel-snapshot (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [kernel-snapshot-adr](kernel-snapshot-adr.md) | Kernel Snapshot ADR | The full-kernel-snapshot emission model re-encodes every projection every dirty tick (O(state) per tick); the ADR-0037 typed sidecar made each re-encode cheaper | capture | warm | 2026-06-13 | kernel-snapshot |

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

## nmp-app-integration (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [instance-scoped-registration](instance-scoped-registration.md) | Instance-Scoped Registration | Instance-scoped module registration (register_action by value with &self methods) replaces stateless-by-construction ActionModule trait, and per-app slots repla | capture | warm | 2026-06-13 | nmp-app-integration |
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

## relay-routing (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [outbox-resolver](outbox-resolver.md) | Outbox Resolver | The OutboxResolver must apply the blocked-relay filter on publish, not just subscribe, to prevent publishing to user-blocked relays | capture | warm | 2026-06-13 | relay-routing |

## shell-defects (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [desktop-shell-defects](desktop-shell-defects.md) | Desktop Shell Defects | The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never a | capture | warm | 2026-06-13 | shell-defects |

## store-projection-replay (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [store-projection-replay](store-projection-replay.md) | Store-Projection Replay | ADR-0045 storeâprojection replay is accepted (staged); implementation is tracked separately | capture | warm | 2026-06-13 | store-projection-replay |

## zap-scope (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [zap-scope](zap-scope.md) | Zap Scope | Zap work is declared post-v1 by owner decision; issues #1008, #999, and #967 are deferred to post-v1 and their needs-decision labels should be dropped | capture | warm | 2026-06-13 | zap-scope |

## Research Records (12 records)

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

## Episode Cards (58 cards)

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
| [2026-06-13-4-post-v1-dead-crates-parked-as](episodes/2026-06-13-4-post-v1-dead-crates-parked-as.md) | 2026-06-13 | Post-v1 dead crates parked as historical | reversal | active |
| [2026-06-13-4-supersede-adr-0039-allow-host-declared](episodes/2026-06-13-4-supersede-adr-0039-allow-host-declared.md) | 2026-06-13 | Supersede ADR-0039: Allow host-declared projection interest (rejecting the blanket prohibition) | reversal | active |
| [2026-06-13-5-host-declared-projection-consumption-supersedes-adr](episodes/2026-06-13-5-host-declared-projection-consumption-supersedes-adr.md) | 2026-06-13 | Host-declared projection consumption supersedes ADR-0039's blanket prohibition | reversal | superseded |
| [2026-06-13-5-pubkey-only-identity-accessor-enables-bunker](episodes/2026-06-13-5-pubkey-only-identity-accessor-enables-bunker.md) | 2026-06-13 | Pubkey-only identity accessor enables bunker account runtimes | product | active |
| [2026-06-13-5-wasm-publish-path-surfaces-honest-error](episodes/2026-06-13-5-wasm-publish-path-surfaces-honest-error.md) | 2026-06-13 | Wasm publish path surfaces honest error instead of silent drop | product | active |

