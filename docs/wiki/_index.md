# Wiki Index

> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.

Last updated: 2026-06-12

## actor-loop (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [actor-loop](actor-loop.md) | Actor Loop | The actor loop uses a dual-channel design with COMMAND_DRAIN_BUDGET for commands and recv_timeout for relay events, preventing command starvation during relay e | capture | warm | 2026-06-11 | actor-loop |

## bunker-connection (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [bunker-connection](bunker-connection.md) | Bunker Connection State | Bunker connection state has a full typed FlatBuffers pipeline (schema, Rust codec, Swift/Android decoders, UI indicators on both platforms) wired through the Ti | capture | warm | 2026-06-11 | bunker-connection |

## code-architecture (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [error-enum-granularity](error-enum-granularity.md) | Error Enum Granularity | Each module should use a single error enum (KeyError, EncryptionError, EventError) rather than per-operation error types, deferring granularity until it earns i | capture | warm | 2026-06-12 | code-architecture |
| [kernel-architecture](kernel-architecture.md) | Kernel Architecture and Reducer Loop | NMP's kernel implements an Elm-style reducer loop as the pure event-processing core, with five architectural tiers: Kernel struct, actor loop, substrate layer, | capture | warm | 2026-06-12 | code-architecture |
| [type-state-pipelines](type-state-pipelines.md) | Type-State Pipelines and Compile-Time Enforcement | State machine transitions should use compile-time-enforced type pipelines rather than runtime checks where applicable | capture | warm | 2026-06-12 | code-architecture |

## code-cleanup (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [dead-code-removal](dead-code-removal.md) | Dead Code and App Removals | The chirp-repl app is deleted entirely — it is not used. | capture | warm | 2026-06-03 | code-cleanup |

## code-generation (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [code-generation](code-generation.md) | Code Generation and FFI | A Rust flatc codegen-drift CI gate (ci/check-rust-flatc-drift.sh) exists and fails on synthetic drift in both directions. | capture | warm | 2026-06-11 | code-generation |

## data-modeling (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [kind-trait-design](kind-trait-design.md) | Kind Trait Design | Kinds should use a trait, not a centralized enum, so each domain module can add its own kinds independently without creating a bottleneck. | capture | warm | 2026-06-12 | data-modeling |
| [search-query-model](search-query-model.md) | SearchQuery Typed Model | SearchQuery should be a typed model with terms and ordered key:value extensions, not an opaque string, since no reference implementation parses extensions. | capture | warm | 2026-06-12 | data-modeling |
| [tag-type-design](tag-type-design.md) | Tag Type Design | Tag types should remain a thin wrapper over Vec<String> rather than a heavy enum, because tag meaning is kind-dependent and an enum bakes in a false abstraction | capture | warm | 2026-06-12 | data-modeling |

## dependency-management (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [dependency-management](dependency-management.md) | Dependency Management and Versioning | nmp-feedback was bumped to nmp-v0.3.0 directly (commit a6794d6) to resolve the diamond NmpApp type conflict | capture | warm | 2026-06-11 | dependency-management |

## dm-relay-ingest (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [dm-crypto-optimization](dm-crypto-optimization.md) | DM/Giftwrap Crypto Optimization | NMP should check whether DM and giftwrap unwrap paths re-derive ECDH+HKDF per message or reuse a ConversationKey | capture | warm | 2026-06-12 | dm-relay-ingest |
| [dm-relay-ingest](dm-relay-ingest.md) | DM Relay Ingest and Compile Triggers | The kind:10050 DM-relay-list ingest now triggers CompileTrigger::DmRelayListChanged (the production seam was previously unwired), fixing a bug where fresh accou | capture | warm | 2026-06-11 | dm-relay-ingest |

## flat-feed (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [flat-feed](flat-feed.md) | FlatFeed and Author/Thread Views | `FlatFeed` in `nmp-nip01` provides a flat chronological note list for author and thread views — distinct from `RootIndexedFeed` which is thread-roots-only with | capture | warm | 2026-06-01 | flat-feed |

## garbage-collection (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [garbage-collection](garbage-collection.md) | Garbage Collection | gc_step is wired onto the actor idle tick behind a 60-second wall-clock gate using the kernel's injected Clock seam for replay determinism | capture | warm | 2026-06-11 | garbage-collection |

## interest-compiler (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [interest-compiler](interest-compiler.md) | Interest Compiler and Feed Subscription | The kernel is kind-agnostic; app-level kind decisions (e.g., `{1, 6}` for social feeds) belong in the Swift/app layer, not the FFI substrate. | capture | warm | 2026-06-01 | interest-compiler |

## kernel-boundary (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [kernel-filter-semantics](kernel-filter-semantics.md) | Kernel Filter Semantics and None vs Empty | When parsing app-supplied filter JSON, `None` (no constraint) must not be collapsed into `Some(empty)` (matches nothing), and vice versa.  For `open_interest(fi | capture | warm | 2026-06-12 | kernel-boundary |
| [kernel-substrate-purity](kernel-substrate-purity.md) | Kernel Substrate Purity (D0) | The kernel/FFI layer must remain a pure substrate with no NIP-specific or kind-specific knowledge, no UI debouncing, and no dead code | capture | warm | 2026-06-03 | kernel-boundary |
| [pow-batching](pow-batching.md) | Proof-of-Work Batching API | Proof-of-work mining must use a batch-based API (start, count) rather than a blocking loop, so the caller controls parallelization and the library stays platfor | capture | warm | 2026-06-12 | kernel-boundary |

## mobile-ci (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [ci-disk-space](ci-disk-space.md) | CI Disk Space and Infrastructure | The CI runner disk-exhaustion bug (cargo test failing with "No space left on device") was fixed by PR #1030 adding a free-disk-space step to test.yml that remov | capture | warm | 2026-06-09 | mobile-ci |
| [mobile-ci](mobile-ci.md) | Mobile CI and Testing | Android JUnit tests run on every PR on ubuntu via native-tests.yml, path-filtered to android/**, nmp-android-ffi, nmp-codegen, nmp-core | capture | warm | 2026-06-11 | mobile-ci |
| [xcode-build-quirks](xcode-build-quirks.md) | Xcode Build and Code Generation Quirks | The BuildInfo.generated.swift xcodegen quirk requires a two-step build: build once to generate BuildInfo, re-run xcodegen, then build again | capture | warm | 2026-06-09 | mobile-ci |

## nostr-protocol (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [encryption-layering](encryption-layering.md) | Encryption Layering and Primitives | Encryption primitives (NIP-04/44) should live in the core library as stateless free functions, with signer-based delegation deferred to a later layer. | capture | warm | 2026-06-12 | nostr-protocol |
| [protected-events](protected-events.md) | Protected Events and Tag Enforcement | Protected events provide only a boolean predicate (`is_protected`) and a `Tag::protected()` constructor | capture | warm | 2026-06-12 | nostr-protocol |

## project-governance (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [podcast-player-integration](podcast-player-integration.md) | Podcast-Player Integration and Consumer Wiring | The podcast-player is a real external NMP consumer that pins nmp-app-template, nmp-core, nmp-ffi, and nmp-signer-broker v0.2.9 by git rev, composing framework s | capture | warm | 2026-06-12 | project-governance |
| [project-governance](project-governance.md) | NMP Scope and Roadmap Decisions | All work must flow through isolated worktrees; committing directly on the main checkout is forbidden. | capture | warm | 2026-06-01 | project-governance |
| [scope-and-roadmap](scope-and-roadmap.md) | NMP Scope and Roadmap Decisions | NMP's v1 scope excludes web (IndexedDB, Worker port), per the owner's Decision A | capture | warm | 2026-06-11 | project-governance |

## publish-action (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [publish-action-path](publish-action-path.md) | Publish Action Path | The PublishNote action variant is deleted from the kernel; the generic PublishRaw (taking kind, tags, content, target) is the only unsigned publish path needed | capture | warm | 2026-06-03 | publish-action |

## relay-connection (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [nip42-auth-reconnect](nip42-auth-reconnect.md) | NIP-42 Auth Reconnect and Subscription Replay | On Authenticated transition, the kernel calls handle_reconnect for the relay instead of just flushing the AuthGate buffer, so all active plan subscriptions are | capture | warm | 2026-06-03 | relay-connection |

## relay-routing (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [relay-routing](relay-routing.md) | NIP Crate Relay Routing Ownership | NIP crates own relay routing for protocol-specific event kinds; apps pass only protocol identity (e.g., GroupId, recipient_pubkey), never relay URLs | capture | warm | 2026-06-03 | relay-routing |

## replaceable-freshness (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [replaceable-freshness](replaceable-freshness.md) | Replaceable Event Freshness and TTL | Replaceable events use a TTL-based freshness system: each replaceable identity tracks `check_again_after`, and claims against stale entries automatically enqueu | capture | warm | 2026-06-01 | replaceable-freshness |

## security (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [secret-handling](secret-handling.md) | Secret Material Handling and Debug Safety | NMP must audit that nmp-core never Debug- or Display-formats secret material into logs, routing traces, or snapshots | capture | warm | 2026-06-12 | security |

## signer-management (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [signer-management](signer-management.md) | Signer Management and Multi-Account Signing | `SignInNsec`, `SignInBunker`, and `AddRemoteSigner` are replaced by a single primitive: `AddSigner { source: SignerSource, make_active: bool }` | capture | warm | 2026-06-03 | signer-management |

## snapshot-emission (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [snapshot-emission](snapshot-emission.md) | Snapshot Emission | The 4Hz snapshot emit only fires when state has changed (changed_since_emit) and 250ms has elapsed; user-dispatched commands emit immediately via maybe_emit_aft | capture | warm | 2026-06-11 | snapshot-emission |

## store-projection-replay (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [store-projection-replay](store-projection-replay.md) | Store-to-Projection Replay | ADR-0045 specifies store→projection replay at interest-open time using existing StoreQuery indexes, replaying via existing post-store projection functions with | capture | warm | 2026-06-11 | store-projection-replay |

## test-infrastructure (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [testable-time](testable-time.md) | Testable Time and Clock Seams | Testable-time APIs should expose the explicit-time variant as the primitive (e.g., `is_expired_at(now)`) with the wall-clock version as a convenience wrapper. | capture | warm | 2026-06-12 | test-infrastructure |

## typed-projections (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [typed-projections](typed-projections.md) | Typed Projections and Overlay Pattern | The typed-projections migration uses the overlay-over-fallback pattern on both iOS and Android; when payload:Value is deleted, the generic fallback branches bec | capture | warm | 2026-06-11 | typed-projections |

## watermark-rewrite (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [watermark-rewrite](watermark-rewrite.md) | Watermark Rewrite and Multi-Author Shapes | The watermark rewrite for multi-author shapes now uses per-author AuthorKind(limit=1) queries against the B-tree index instead of the author-blind KindTime glob | capture | warm | 2026-06-11 | watermark-rewrite |

## wire-frame-format (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [wire-frame-format](wire-frame-format.md) | Wire Frame Format and Schema Versioning | The payload:Value field is deleted from the wire frame, reducing frame size from ~14,504B to ~3,384B (a 76.7% reduction) | capture | warm | 2026-06-11 | wire-frame-format |

## zap-flow (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [nutzap-redemption](nutzap-redemption.md) | Nutzap Redemption and Idempotency | Redemption idempotency is a NIP-61 protocol-level concern and must be handled in the nmp-nip60 crate, not patched per-app | capture | warm | 2026-06-07 | zap-flow |
| [zap-flow](zap-flow.md) | Zap Send Flow and Feedback | The zap send flow (Rust side) is complete: ZapAction → FetchLnurlInvoiceCommand → signs kind:9734 → LNURL HTTP → WalletPayInvoiceCommand → NWC kind:23195 → kern | capture | warm | 2026-06-01 | zap-flow |

## Research Records (1 record)

| Record | Date | Finding | Agent |
|--------|------|---------|-------|
| [2026-06-11-1-d0-violation-audit-of-nmp-core](research/2026-06-11-1-d0-violation-audit-of-nmp-core.md) | 2026-06-11 | D0 violation audit of nmp-core kernel — found 12 instances of NIP/kind-specific knowledge in the pure substrate layer, tiered by migration priority | a0b1dcda99d2cf66e |

## Episode Cards (76 cards)

| Card | Date | Title | Salience |
|------|------|-------|----------|
| [2026-06-01-1-wasm-scope-removed-from-v1-backlog](episodes/2026-06-01-1-wasm-scope-removed-from-v1-backlog.md) | 2026-06-01 | WASM scope removed from v1 backlog | reversal |
| [2026-06-01-1-zap-success-feedback-discarded-correlation-id](episodes/2026-06-01-1-zap-success-feedback-discarded-correlation-id.md) | 2026-06-01 | Zap success feedback: discarded correlation ID was the only gap | reversal |
| [2026-06-03-1-c-abi-surface-reframed-as-frozen](episodes/2026-06-03-1-c-abi-surface-reframed-as-frozen.md) | 2026-06-03 | C-ABI surface reframed as frozen framework API, not migration debt | architecture |
| [2026-06-03-1-converge-and-delete-replaces-struct-relocation](episodes/2026-06-03-1-converge-and-delete-replaces-struct-relocation.md) | 2026-06-03 | Converge-and-delete replaces struct relocation for TimelineItem | architecture |
| [2026-06-03-1-podcast-player-bypasses-nmp-publish-engine](episodes/2026-06-03-1-podcast-player-bypasses-nmp-publish-engine.md) | 2026-06-03 | Podcast-player bypasses NMP publish engine with raw relay capability | architecture |
| [2026-06-03-1-publishnote-bespoke-ffi-entry-replaced-by](episodes/2026-06-03-1-publishnote-bespoke-ffi-entry-replaced-by.md) | 2026-06-03 | PublishNote bespoke FFI entry replaced by PublishRaw namespace | architecture |
| [2026-06-03-1-three-sign-in-commands-collapsed-into](episodes/2026-06-03-1-three-sign-in-commands-collapsed-into.md) | 2026-06-03 | Three sign-in commands collapsed into AddSigner | reversal |
| [2026-06-03-1-v-78-bunker-accounts-can-now](episodes/2026-06-03-1-v-78-bunker-accounts-can-now.md) | 2026-06-03 | V-78: bunker accounts can now zap via nonblocking sign seam | product |
| [2026-06-03-2-ephemeral-nip-01-kinds-20000-29999](episodes/2026-06-03-2-ephemeral-nip-01-kinds-20000-29999.md) | 2026-06-03 | Ephemeral NIP-01 kinds 20000–29999 misclassified as replaceable | root-cause |
| [2026-06-03-2-minimal-nip-10-reply-markers-rejected](episodes/2026-06-03-2-minimal-nip-10-reply-markers-rejected.md) | 2026-06-03 | Minimal NIP-10 reply markers rejected as unauthorized technical debt | product |
| [2026-06-03-2-nip-57-zap-awareness-expelled-from](episodes/2026-06-03-2-nip-57-zap-awareness-expelled-from.md) | 2026-06-03 | NIP-57 zap awareness expelled from timeline projection | product |
| [2026-06-03-2-v-112-step-d-deferred-author](episodes/2026-06-03-2-v-112-step-d-deferred-author.md) | 2026-06-03 | V-112 Step D deferred: author_view/thread_view carry unreplaced display fields | reversal |
| [2026-06-03-2-verify-signer-probe-and-add-unverified](episodes/2026-06-03-2-verify-signer-probe-and-add-unverified.md) | 2026-06-03 | verify_signer probe and add_unverified removed | root-cause |
| [2026-06-03-3-nmp-nip01-confirmed-as-destination-no](episodes/2026-06-03-3-nmp-nip01-confirmed-as-destination-no.md) | 2026-06-03 | nmp-nip01 confirmed as destination — no new crate needed | architecture |
| [2026-06-03-3-non-active-signer-publish-path-added](episodes/2026-06-03-3-non-active-signer-publish-path-added.md) | 2026-06-03 | Non-active-signer publish path added | product |
| [2026-06-03-3-noterecord-renamed-to-eventrecord-nip-10](episodes/2026-06-03-3-noterecord-renamed-to-eventrecord-nip-10.md) | 2026-06-03 | NoteRecord renamed to EventRecord — NIP-10 is kind-agnostic | architecture |
| [2026-06-03-4-feed-cluster-deletion-scoped-down-frozen](episodes/2026-06-03-4-feed-cluster-deletion-scoped-down-frozen.md) | 2026-06-03 | Feed cluster deletion scoped down — frozen FFI blocks full struct removal | root-cause |
| [2026-06-03-4-timelineitem-in-nmp-core-identified-as](episodes/2026-06-03-4-timelineitem-in-nmp-core-identified-as.md) | 2026-06-03 | TimelineItem in nmp-core identified as D0 violation — social concepts must not live in substrate | architecture |
| [2026-06-03-5-home-feed-projection-switched-from-legacy](episodes/2026-06-03-5-home-feed-projection-switched-from-legacy.md) | 2026-06-03 | Home feed projection switched from legacy keys to nmp.feed.home OP-feed | product |
| [2026-06-03-5-shell-side-nip-10-construction-prohibited](episodes/2026-06-03-5-shell-side-nip-10-construction-prohibited.md) | 2026-06-03 | Shell-side NIP-10 construction prohibited by D5/D8 doctrine | architecture |
| [2026-06-03-6-timelineeventcard-stripped-of-display-presentation-fields](episodes/2026-06-03-6-timelineeventcard-stripped-of-display-presentation-fields.md) | 2026-06-03 | TimelineEventCard stripped of display/presentation fields | architecture |
| [2026-06-07-1-nutzap-redemption-not-idempotent-crate-writes](episodes/2026-06-07-1-nutzap-redemption-not-idempotent-crate-writes.md) | 2026-06-07 | nutzap redemption not idempotent — crate writes redeemed markers but never reads them back | root-cause |
| [2026-06-08-1-flatfeed-api-consolidation-dual-path-feed](episodes/2026-06-08-1-flatfeed-api-consolidation-dual-path-feed.md) | 2026-06-08 | FlatFeed API consolidation: dual-path feed design superseded by single-path on master | architecture |
| [2026-06-09-1-nmp-conformance-scanner-in-repo-catalog](episodes/2026-06-09-1-nmp-conformance-scanner-in-repo-catalog.md) | 2026-06-09 | NMP conformance scanner: in-repo catalog + drift gate, not portable standalone | architecture |
| [2026-06-09-2-producer-completeness-gate-host-tests-can](episodes/2026-06-09-2-producer-completeness-gate-host-tests-can.md) | 2026-06-09 | Producer-completeness gate: host tests can't prove typed side is complete | root-cause |
| [2026-06-09-3-chirp-consumer-typed-only-remove-all](episodes/2026-06-09-3-chirp-consumer-typed-only-remove-all.md) | 2026-06-09 | Chirp consumer typed-only: remove all JSON fallbacks and whole-payload decode | product |
| [2026-06-09-4-payload-value-schema-deletion-gated-on](episodes/2026-06-09-4-payload-value-schema-deletion-gated-on.md) | 2026-06-09 | payload:Value schema deletion gated on six remaining consumers | architecture |
| [2026-06-10-1-v58-flake-was-a-real-production](episodes/2026-06-10-1-v58-flake-was-a-real-production.md) | 2026-06-10 | v58 "flake" was a real production bug — edge-triggered poll-event loss | root-cause |
| [2026-06-10-2-update-callback-quiescence-contract-closes-uaf](episodes/2026-06-10-2-update-callback-quiescence-contract-closes-uaf.md) | 2026-06-10 | Update-callback quiescence contract closes UAF on Android and iOS | architecture |
| [2026-06-10-3-dm-send-single-terminal-invariant-one](episodes/2026-06-10-3-dm-send-single-terminal-invariant-one.md) | 2026-06-10 | DM-send single-terminal invariant — one action, one verdict | product |
| [2026-06-10-4-addr-tombstones-never-gc-purged-unbounded](episodes/2026-06-10-4-addr-tombstones-never-gc-purged-unbounded.md) | 2026-06-10 | addr_tombstones never GC-purged — unbounded store growth | root-cause |
| [2026-06-10-5-router-fail-closed-on-explicitly-empty](episodes/2026-06-10-5-router-fail-closed-on-explicitly-empty.md) | 2026-06-10 | Router fail-closed on explicitly empty NIP-65 write set | product |
| [2026-06-10-6-flaky-test-doctrine-retirement-future-failures](episodes/2026-06-10-6-flaky-test-doctrine-retirement-future-failures.md) | 2026-06-10 | Flaky-test doctrine retirement — future failures are real regressions | architecture |
| [2026-06-10-7-relay-worker-event-honesty-three-real](episodes/2026-06-10-7-relay-worker-event-honesty-three-real.md) | 2026-06-10 | Relay worker event honesty — three real bugs, one disproven | root-cause |
| [2026-06-11-1-zap-wallet-moved-from-v1-to](episodes/2026-06-11-1-zap-wallet-moved-from-v1-to.md) | 2026-06-11 | Zap/wallet moved from v1 to post-v1 | reversal |
| [2026-06-11-1-zap-wallet-scope-narrowed-from-v1](episodes/2026-06-11-1-zap-wallet-scope-narrowed-from-v1.md) | 2026-06-11 | Zap/wallet scope narrowed from v1 to post-v1 | reversal |
| [2026-06-11-2-author-blind-watermark-caused-new-follow](episodes/2026-06-11-2-author-blind-watermark-caused-new-follow.md) | 2026-06-11 | Author-blind watermark caused new-follow backfill failure | root-cause |
| [2026-06-11-2-watermark-author-blindness-root-cause-and](episodes/2026-06-11-2-watermark-author-blindness-root-cause-and.md) | 2026-06-11 | Watermark author-blindness root cause and fix | root-cause |
| [2026-06-11-3-android-dark-frame-root-cause-tier](episodes/2026-06-11-3-android-dark-frame-root-cause-tier.md) | 2026-06-11 | Android dark-frame root cause — Tier-3 spine rebuild | root-cause |
| [2026-06-11-3-android-tier-3-spine-eliminates-payload](episodes/2026-06-11-3-android-tier-3-spine-eliminates-payload.md) | 2026-06-11 | Android Tier-3 spine eliminates payload dependency | root-cause |
| [2026-06-11-4-gc-budget-honesty-resumable-cursor-o](episodes/2026-06-11-4-gc-budget-honesty-resumable-cursor-o.md) | 2026-06-11 | GC budget honesty: resumable cursor, O(1) count, LRU ceiling disabled | architecture |
| [2026-06-11-4-gc-honesty-budgeted-resumable-scans-lru](episodes/2026-06-11-4-gc-honesty-budgeted-resumable-scans-lru.md) | 2026-06-11 | GC honesty — budgeted resumable scans, LRU ceiling disabled | architecture |
| [2026-06-11-5-kernel-ram-eviction-with-open-view](episodes/2026-06-11-5-kernel-ram-eviction-with-open-view.md) | 2026-06-11 | Kernel RAM eviction with open-view pin sets | architecture |
| [2026-06-11-5-open-view-pin-sets-close-eviction](episodes/2026-06-11-5-open-view-pin-sets-close-eviction.md) | 2026-06-11 | Open-view pin sets close eviction-blank-row regression | root-cause |
| [2026-06-11-6-adr-0045-store-projection-replay-architecture](episodes/2026-06-11-6-adr-0045-store-projection-replay-architecture.md) | 2026-06-11 | ADR-0045: store→projection replay architecture | architecture |
| [2026-06-11-6-store-replay-must-bypass-insert-duplicate](episodes/2026-06-11-6-store-replay-must-bypass-insert-duplicate.md) | 2026-06-11 | Store replay must bypass insert — Duplicate arm is a deliberate no-op | root-cause |
| [2026-06-11-7-bunker-connection-state-built-but-unwired](episodes/2026-06-11-7-bunker-connection-state-built-but-unwired.md) | 2026-06-11 | Bunker connection state — built-but-unwired V-14 fixed | product |
| [2026-06-11-8-legacy-1-6-surface-deletion-silent](episodes/2026-06-11-8-legacy-1-6-surface-deletion-silent.md) | 2026-06-11 | Legacy {1,6} surface deletion — silent merge-break caught | architecture |
| [2026-06-12-1-adr-0045-single-cache-serve-mechanism](episodes/2026-06-12-1-adr-0045-single-cache-serve-mechanism.md) | 2026-06-12 | ADR-0045: Single cache-serve mechanism replaces staged domain-specific approach | architecture |
| [2026-06-12-1-m2-filter-api-must-preserve-none](episodes/2026-06-12-1-m2-filter-api-must-preserve-none.md) | 2026-06-12 | M2 filter API must preserve None-vs-empty semantics and prevent tag-key prefix typos | product |
| [2026-06-12-1-m2-filter-contract-hazards-none-vs](episodes/2026-06-12-1-m2-filter-contract-hazards-none-vs.md) | 2026-06-12 | M2 filter-contract hazards: None-vs-empty and tag-key typos | product |
| [2026-06-12-1-open-view-ram-eviction-would-silently](episodes/2026-06-12-1-open-view-ram-eviction-would-silently.md) | 2026-06-12 | Open-view RAM eviction would silently blank live threads | root-cause |
| [2026-06-12-1-retire-open-timeline-via-dedicated-contact](episodes/2026-06-12-1-retire-open-timeline-via-dedicated-contact.md) | 2026-06-12 | Retire open_timeline via dedicated contact-feed verb, not open_interest scope overload | architecture |
| [2026-06-12-1-store-projection-replay-must-bypass-store](episodes/2026-06-12-1-store-projection-replay-must-bypass-store.md) | 2026-06-12 | Store→Projection Replay Must Bypass store.insert (ADR-0045) | root-cause |
| [2026-06-12-2-gc-perf-gate-was-17-too](episodes/2026-06-12-2-gc-perf-gate-was-17-too.md) | 2026-06-12 | GC perf gate was 17× too loose; cursor livelock discovered | root-cause |
| [2026-06-12-2-mandate-real-expiration-index-for-gc](episodes/2026-06-12-2-mandate-real-expiration-index-for-gc.md) | 2026-06-12 | Mandate real expiration index for gc livelock, not tactical hack | root-cause |
| [2026-06-12-2-open-view-ram-pin-derivation-architecture](episodes/2026-06-12-2-open-view-ram-pin-derivation-architecture.md) | 2026-06-12 | Open-view RAM pin derivation — architecture doctrine for eviction safety | architecture |
| [2026-06-12-2-ram-eviction-pin-sets-missed-open](episodes/2026-06-12-2-ram-eviction-pin-sets-missed-open.md) | 2026-06-12 | RAM Eviction Pin Sets Missed Open View Working Sets | root-cause |
| [2026-06-12-2-rust-nostr-dependency-footguns-secretkey-debug](episodes/2026-06-12-2-rust-nostr-dependency-footguns-secretkey-debug.md) | 2026-06-12 | rust-nostr dependency footguns: SecretKey Debug, TagStandard overhead, ConversationKey reuse | root-cause |
| [2026-06-12-2-typed-flatbuffers-projections-validated-against-protocol](episodes/2026-06-12-2-typed-flatbuffers-projections-validated-against-protocol.md) | 2026-06-12 | Typed FlatBuffers projections validated against protocol-level anti-typing doctrine | architecture |
| [2026-06-12-3-ffi-surface-debt-corrected-from-48](episodes/2026-06-12-3-ffi-surface-debt-corrected-from-48.md) | 2026-06-12 | FFI surface debt corrected from '48 bespoke symbols' to three specific items | root-cause |
| [2026-06-12-3-gc-perf-gate-thresholds-were-17](episodes/2026-06-12-3-gc-perf-gate-thresholds-were-17.md) | 2026-06-12 | GC Perf Gate Thresholds Were ~17× Too Loose | root-cause |
| [2026-06-12-3-legacy-1-6-c-abi-surfaces](episodes/2026-06-12-3-legacy-1-6-c-abi-surfaces.md) | 2026-06-12 | Legacy {1,6} C-ABI surfaces + author/thread state machine retired | product |
| [2026-06-12-3-legacy-author-thread-c-abi-surfaces](episodes/2026-06-12-3-legacy-author-thread-c-abi-surfaces.md) | 2026-06-12 | Legacy author/thread C-ABI surfaces replaced by generic interest seam | reversal |
| [2026-06-12-3-raw-event-tap-and-dual-free](episodes/2026-06-12-3-raw-event-tap-and-dual-free.md) | 2026-06-12 | Raw-event tap and dual free-string deferred as non-mechanical | architecture |
| [2026-06-12-3-typed-projection-doctrine-scope-clarified-against](episodes/2026-06-12-3-typed-projection-doctrine-scope-clarified-against.md) | 2026-06-12 | Typed-projection doctrine scope clarified against 'don't type-system-ify open-ended data' warning | architecture |
| [2026-06-12-4-adr-methodology-add-comparative-research-survey](episodes/2026-06-12-4-adr-methodology-add-comparative-research-survey.md) | 2026-06-12 | ADR methodology: add comparative-research survey step before design decisions | workflow |
| [2026-06-12-4-conflict-free-merge-silently-breaks-compilation](episodes/2026-06-12-4-conflict-free-merge-silently-breaks-compilation.md) | 2026-06-12 | Conflict-free merge silently breaks compilation — durable root cause | root-cause |
| [2026-06-12-4-example-compile-gap-class-examples-not](episodes/2026-06-12-4-example-compile-gap-class-examples-not.md) | 2026-06-12 | Example-compile gap class — examples not built by workspace CI | root-cause |
| [2026-06-12-4-legacy-1-6-deletion-required-pin](episodes/2026-06-12-4-legacy-1-6-deletion-required-pin.md) | 2026-06-12 | Legacy {1,6} Deletion Required Pin Re-Derivation from Interest Registry | architecture |
| [2026-06-12-5-gc-honesty-reform-resumable-budgets-disabled](episodes/2026-06-12-5-gc-honesty-reform-resumable-budgets-disabled.md) | 2026-06-12 | GC honesty reform — resumable budgets, disabled LRU ceiling | architecture |
| [2026-06-12-5-podcast-player-latent-push-path-bug](episodes/2026-06-12-5-podcast-player-latent-push-path-bug.md) | 2026-06-12 | Podcast-player latent push-path bug — same defect class as NMP #1084 | root-cause |
| [2026-06-12-5-v0-3-0-shipped-with-android](episodes/2026-06-12-5-v0-3-0-shipped-with-android.md) | 2026-06-12 | v0.3.0 Shipped with Android Completely Dark (V-116) | product |
| [2026-06-12-6-claimed-profiles-decode-was-never-publicly](episodes/2026-06-12-6-claimed-profiles-decode-was-never-publicly.md) | 2026-06-12 | Claimed Profiles Decode Was Never Publicly Exported | root-cause |
| [2026-06-12-6-version-0-4-0-instead-of](episodes/2026-06-12-6-version-0-4-0-instead-of.md) | 2026-06-12 | Version 0.4.0 instead of 0.3.1 — C-ABI break forces major bump | architecture |
| [2026-06-12-6-version-bump-0-3-1-0](episodes/2026-06-12-6-version-bump-0-3-1-0.md) | 2026-06-12 | Version bump 0.3.1 → 0.4.0 for C-ABI break — Android must skip v0.3.0 entirely | reversal |

