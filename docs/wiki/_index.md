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

## codebase-patterns (7 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [agent-workflow-policy](agent-workflow-policy.md) | Agent Workflow Policy | Completed PR descriptions must include a short TLDR summary, a detailed overview of the work performed, and any subjective decisions including tradeoffs or assu | capture | warm | 2026-06-14 | codebase-patterns |
| [agents-md-policy](agents-md-policy.md) | AGENTS.md Policy | The week-one act before any keystone is landing the supersession-deletion policy in AGENTS.md plus the empty mechanism_census test. | capture | warm | 2026-06-13 | codebase-patterns |
| [codebase-patterns](codebase-patterns.md) | Codebase Patterns | The file-size gate enforces a 500-line hard cap with an anti-cheat rule that blocks raising a file's baseline in a PR; zero baseline bumps were merged across th | capture | warm | 2026-06-13 | codebase-patterns |
| [excellence-program](excellence-program.md) | Excellence Program | The excellence program identifies six repo-wide patterns found by the reviewers: superseded generations never deleted, presence-is-not-coverage, invariants by c | capture | warm | 2026-06-13 | codebase-patterns |
| [git-worktree-policy](git-worktree-policy.md) | Git Worktree Policy | Agents must work in isolated git worktrees, never moving the base repo away from master. | capture | warm | 2026-06-13 | codebase-patterns |
| [keystone-overview](keystone-overview.md) | Keystone Overview | The three keystones are K1 (signer-session port covering sign\|nip44_encrypt\|nip44_decrypt with mailbox completions), K2 (instance-scoped registration replacing | capture | warm | 2026-06-13 | codebase-patterns |
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

## nmp-app-integration (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [cli-registry-manifest](cli-registry-manifest.md) | CLI Registry Manifest | The CLI registry manifest must mirror all component ids that appear in the web registry, including web-targeted components such as web/login-block, web/relay-li | capture | warm | 2026-06-14 | nmp-app-integration |
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

## zap-scope (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [zap-scope](zap-scope.md) | Zap Scope | Zap work is declared post-v1 by owner decision; issues #1008, #999, and #967 are deferred to post-v1 and their needs-decision labels should be dropped | capture | warm | 2026-06-13 | zap-scope |

