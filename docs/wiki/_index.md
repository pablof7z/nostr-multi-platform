# Wiki Index

> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.

Last updated: 2026-07-04

## action-dispatch (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [action-dispatch](guides/action-dispatch.md) | Action Registration and Dispatch | Registering an action means implementing one trait; the framework owns dispatch | capture | warm | 2026-06-29 | action-dispatch |

## adr-governance (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [adr-governance](guides/adr-governance.md) | ADR Directory Governance | ADR-0073 keeps the ADR directory current-only; obsolete decisions move surviving rules to current owners and are deleted | capture | warm | 2026-06-29 | adr-governance |

## agent-coordination (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [agent-invitation](guides/agent-invitation.md) | Agent Invitation Criteria and Format | Agents in the tenex-edge fabric should not invite other agents solely because they are available; collaboration should have real payoff â specialization, para | capture | warm | 2026-07-03 | agent-coordination |
| [fabric-snapshot](guides/fabric-snapshot.md) | Fabric Snapshot: Agent Ambient Awareness | The fabric snapshot is a hook-provided ambient awareness block that tells an agent its identity, current channel, nearby agents, recent changes, and invitable a | capture | warm | 2026-07-03 | agent-coordination |
| [triage-workflow](guides/triage-workflow.md) | Triage Workflow and Agent Dispatch | Issue triage uses a two-tier agent strategy: sonnet agents PR + land straightforward slam-dunks, while opus agents review subjective issues with authority to cl | capture | warm | 2026-07-04 | agent-coordination |

## app-accounts (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-accounts](guides/chirp-accounts.md) | Chirp Account Flows and Default Follows | Chirp account-creation (`nmp_app_chirp_create_new_account` / `ChirpApp::create_new_account`) publishes its own kind:3 contact list via `chirp_default_follows()` | capture | warm | 2026-07-04 | app-accounts |

## app-codegen (4 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-action-builders](guides/chirp-action-builders.md) | Chirp Action-Builders Registry and Codegen | The NMP CLI no longer has the `nmp gen swift` subcommand or the `nmp-core` `codegen-schema` feature | capture | warm | 2026-07-04 | app-codegen |
| [chirp-concept-reads](guides/chirp-concept-reads.md) | Chirp Concept-Reads Registry and Codegen | Chirp's concept-reads registry is `crates/nmp-app-chirp/concept-reads.json` (iOS, facade=ChirpApp) and `crates/nmp-chirp-android-ffi/concept-reads.json` (Androi | capture | warm | 2026-07-04 | app-codegen |
| [chirp-flatbuffers-codegen](guides/chirp-flatbuffers-codegen.md) | Chirp FlatBuffers Codegen and Wire Schema | Each Chirp app runs `flatc` locally and checks in the generated FlatBuffers wire-type decoders rather than consuming a prebuilt bindings package from NMP | capture | warm | 2026-07-04 | app-codegen |
| [chirp-search](guides/chirp-search.md) | Chirp Search: NIP-50 Full-Text and Entity Navigation | Chirp iOS includes a search feature filed as chirp#71 | capture | warm | 2026-07-04 | app-codegen |

## app-defined-kinds (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [app-defined-kinds](guides/app-defined-kinds.md) | App-Defined Event Kinds: First-Class Support and Codegen | An app should be able to define its own made-up event kind â number, schema, builder â and have it be a first-class citizen in the app's own codebase, on pa | capture | warm | 2026-06-29 | app-defined-kinds |

## app-dms (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-dms](guides/chirp-dms.md) | Chirp DMs: NIP-17 Gift-Wrap and Relay Routing | iOS DMs are sent via NIP-17 gift-wrap (kind:1059) over wss-only relay targets | capture | warm | 2026-07-04 | app-dms |

## app-feed (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-feed-params](guides/chirp-feed-params.md) | Chirp FeedParams Shape and Compilation | Chirp's FeedParams JSON shape uses the following fields: `shape`, `source`, `order`, `key`, `item_projection`, `primary_kinds`, `admission`, and `window` | capture | warm | 2026-07-04 | app-feed |
| [feed-scope-composition](guides/feed-scope-composition.md) | Feed Scope Composition and Active-User Degradation | Chirp's home feed uses a Difference(follows, mute) source composition with RootIndexed primary_kinds [1] and All admission for the Android/desktop/TUI Rust shel | capture | warm | 2026-07-04 | app-feed |

## app-groups (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-groups](guides/chirp-groups.md) | Chirp Groups: Marmot Runtime and Encrypted Keyring Storage | chirp#48 is the Groups/Marmot dead-backend bug: a complete, well-built UI wired to a Marmot runtime that the migration removed and never reinstalled | capture | warm | 2026-07-04 | app-groups |

## app-lifecycle (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [app-lifecycle](guides/app-lifecycle.md) | NmpApp Lifecycle and Shutdown | The UniFFI runtime object exposes an explicit idempotent `shutdown()` method (not `close`, to avoid Kotlin friction from #2149) | capture | warm | 2026-06-29 | app-lifecycle |
| [lifecycle-reset-desync](guides/lifecycle-reset-desync.md) | Lifecycle Reset Desync Bug (#2932) | NMP#2932 is a latent desync bug where LifecycleCommand::Reset rebuilds the kernel with an empty active_account slot and nulls the shared slot, but does not touc | capture | warm | 2026-07-04 | app-lifecycle |
| [signer-state-slot](guides/signer-state-slot.md) | Signer State Slot and Remote Signer Health Display | The `signer_state` slot is a kernel-owned global slot with no per-account identity or keying | capture | warm | 2026-07-04 | app-lifecycle |

## app-platform-bindings (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-android-jni](guides/chirp-android-jni.md) | Chirp Android JNI and EFI Bindings | Android's `KernelBridge.kt` JNI extern declarations use plain `external fun` (public), not `internal external fun`, because Kotlin mangles internal JVM method n | capture | warm | 2026-07-04 | app-platform-bindings |

## app-projection (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-projection-cache](guides/chirp-projection-cache.md) | Chirp Projection Cache and Rev Tracking | App-owned projection keys (non-manifest keys like `chirp.timeline.home`) are absent from the kernel's builtin rev manifest, so without a fix they emit `projecti | capture | warm | 2026-07-04 | app-projection |
| [chirp-projection-keys](guides/chirp-projection-keys.md) | Chirp Projection Keys and Timeline Namespace | Chirp's `nmp.feed.home`/`.author.<pubkey>`/`.thread.<event_id>` projection keys are renamed to `chirp.timeline.home`/`chirp.timeline.author.<pubkey>`/`chirp.tim | capture | warm | 2026-07-04 | app-projection |

## app-web (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-web-packages](guides/chirp-web-packages.md) | Chirp Web Packages and Build Pipeline | Chirp web's npm package dependencies are `@nmpis/runtime-web` and `@nmpis/components-web` at `^1.0.0-rc.1`, replacing the old `@nmp/*` scope names | capture | warm | 2026-07-04 | app-web |

## autonomous-loop (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [autonomous-loop](guides/autonomous-loop.md) | Autonomous Refactor Loop | The autonomous loop runs once per hour | capture | warm | 2026-06-29 | autonomous-loop |

## channel-management (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [channel-management](guides/channel-management.md) | Channel Management and Namespacing | The tenex-edge skill is the coordination mechanism agents use to join channels, read and write chat, list sessions, invite other agents, and navigate channels | capture | warm | 2026-07-03 | channel-management |

## ci-gates (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [ci-gates](guides/ci-gates.md) | CI Gate Policies During Migration | During the migration, CI checks that help identify issues as we build are kept, but unnecessary CI gates that slow things down while things are supposed to be b | capture | warm | 2026-06-29 | ci-gates |
| [release-readiness](guides/release-readiness.md) | Release Readiness Pipeline | The `release-readiness.yml` workflow is the exit criterion for the release pipeline | capture | warm | 2026-07-03 | ci-gates |
| [workspace-test-policy](guides/workspace-test-policy.md) | Workspace Test Policy | Running `cargo test --workspace` must not be run | capture | warm | 2026-07-03 | ci-gates |

## codex-usage (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [codex-usage](guides/codex-usage.md) | Codex Usage Policy | Codex is used sparingly, only at major milestones | capture | warm | 2026-06-29 | codex-usage |

## component-registry (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [component-registry](guides/component-registry.md) | Component Registry Unification | The project maintains two component registries that have historically existed as separate artifacts: the gallery showcase catalog (`apps/nmp-gallery/registry.js | capture | warm | 2026-06-29 | component-registry |

## composition-root (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [composition-root](guides/composition-root.md) | Explicit Composition Root and register_defaults Elimination | Per ADR-0069, production apps compose named owners directly | capture | warm | 2026-07-02 | composition-root |

## crate-ownership (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [crate-ownership](guides/crate-ownership.md) | NMP Crate Ownership and Helper Policy | Anything a second platform would have to reimplement to stay correct â relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning â | capture | warm | 2026-06-29 | crate-ownership |

## data-persistence (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [browser-storage-init](guides/browser-storage-init.md) | Browser Durable Storage Initialization | Browser durable storage (Worker/OPFS) must initialize before the product starts | capture | warm | 2026-06-29 | data-persistence |

## deletion-ledger (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [deletion-ledger](guides/deletion-ledger.md) | Architecture Deletion Ledger and Ratchets | Each architecture slice carries a deletion ledger that tracks old doors deleted or privatized, new concepts introduced, and the net change in permanent concepts | capture | warm | 2026-06-29 | deletion-ledger |

## dx-proof (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [dx-proof](guides/dx-proof.md) | DX Clean-Room Proof Gate | Issue #2256 is the clean-break DX gate: a clean-room onboarding proof | capture | warm | 2026-06-29 | dx-proof |
| [validation-evidence](guides/validation-evidence.md) | Validation Evidence and Screenshot Policy | Validation is performed by driving the real apps on simulators, emulators, and Playwright â not merely compiling or running unit tests | capture | warm | 2026-07-04 | dx-proof |
| [xray-diagnostics](guides/xray-diagnostics.md) | X-Ray Diagnostic Tool | X-Ray is a developer diagnostic tool that answers questions like "why is my feed empty?" or "why did this subscription close?" using recorded receipts | capture | warm | 2026-07-03 | dx-proof |

## head-coordinator (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [head-coordinator](guides/head-coordinator.md) | Head Coordinator and PR Workflow | Before fanning out agents, the head-coordinator checks master CI health and open PRs (resync root checkout with origin/master, then check PRs before fanout) | capture | warm | 2026-06-29 | head-coordinator |

## issue-queue (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [issue-queue](guides/issue-queue.md) | Issue Queue as Canonical Tracker | The issue queue is the single canonical temporal tracker for the project â not a museum | capture | warm | 2026-06-29 | issue-queue |

## notifications (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [notification-system](guides/notification-system.md) | System Notifications via say Command | Important notifications are sent to the user by running the `say` command. | capture | warm | 2026-07-04 | notifications |

## project-status (4 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [in-tree-harnesses](guides/in-tree-harnesses.md) | In-Tree Conformance Harnesses and Extracted Apps | Gallery stays in-tree as a cross-platform conformance and regression harness â a storybook proving every NMP component decodes and renders on every platform a | capture | warm | 2026-06-29 | project-status |
| [nmp-doctor](guides/nmp-doctor.md) | nmp doctor: Scope, Contract, and Modes | `nmp doctor` is an approved diagnostic command with a narrow scope: dependency/source coherence, retired-crate detection, path-dep checks, and informational-onl | capture | warm | 2026-07-03 | project-status |
| [project-status](guides/project-status.md) | NMP Project Status: NIP Scope and ADR Spine | The v1 public name for the project is kept as NMP | capture | warm | 2026-06-29 | project-status |
| [v1-release-train](guides/v1-release-train.md) | V1 Release Train and Publish Gate | Issue #2690 is the v1 release-train epic | capture | warm | 2026-07-04 | project-status |

## publish-workflow (1 guide)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [draft-builders](guides/draft-builders.md) | Draft Builder Composability and Side-Effect Limits | Event construction is composable: template event builders (such as react_to_event or reply_to_event) produce unsigned draft events, and the publish action may t | capture | warm | 2026-06-29 | publish-workflow |

## read-door (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [nip-ad-resolution](guides/nip-ad-resolution.md) | NIP-AD URL Resolution and AdResolutionPolicy | NIP-AD resolution is app-configurable via an injected `AdResolutionPolicy`; the framework ships no default on/off/WoT decision, and the app picks at its composi | capture | warm | 2026-07-04 | read-door |
| [read-door](guides/read-door.md) | The Read Door: Typed Read Sessions and API Surface | The read door follows the typed sessions architecture established in ADR-0070 | capture | warm | 2026-06-29 | read-door |
| [trellis-substrate](guides/trellis-substrate.md) | Trellis Reconciliation Substrate | Trellis (ADR-0075) is an in-memory, per-session, reactive read-side reconciliation substrate for dependency-graph transactions and deterministic replay that liv | capture | warm | 2026-07-03 | read-door |

## test-seams (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [fixed-clock-test-seam](guides/fixed-clock-test-seam.md) | FixedClock Test Seam for Deterministic Timestamp Tests | The flaky test `auto_arm_finalizes_before_parking_remote_sign` (#2962) is caused by a real wall-clock race: two live `kernel.now_secs()` reads straddle a second | capture | warm | 2026-07-04 | test-seams |
| [test-seams](guides/test-seams.md) | Test Seams and Bypass Patterns | Issue #2970 (NIP-17 wss-only gate blocks `nak serve`) must NOT have its parser gate relaxed | capture | warm | 2026-07-04 | test-seams |

## ui-components (5 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-profile-resolution](guides/chirp-profile-resolution.md) | Chirp Profile Resolution and Change Publisher Subscription | Profile resolution failing â rows showing raw hex pubkeys instead of names and avatars â is a core bug, not a cosmetic issue | capture | warm | 2026-07-04 | ui-components |
| [chirp-ui](guides/chirp-ui.md) | Chirp UI: Navigation, Social Bar, and Zap Removal | The Android nav bar has 5 tabs plus a More screen instead of 8 cramped tabs with mid-word wrapping. | capture | warm | 2026-07-04 | ui-components |
| [embed-kind-projection](guides/embed-kind-projection.md) | EmbedKindProjection: Typed Content-Kind Projection and Cross-Platform Dispatch | EmbedKindProjection is the per-kind typed projection struct in nmp-content that maps a raw Nostr event to a renderable embed, dispatched through a single match | capture | warm | 2026-07-04 | ui-components |
| [inline-video-player](guides/inline-video-player.md) | Inline Video Player Component | Inline video players in note content views use a dedicated `NostrInlineVideoPlayer` view with `@State` so the `AVPlayer` is constructed exactly once per view id | capture | warm | 2026-07-04 | ui-components |
| [publish-in-flight-toast](guides/publish-in-flight-toast.md) | Stale Publish In-Flight Toast Bug | A stuck 'publish already in flight' toast must clear when the referenced event's publish acknowledges, not persist on screen and block UI elements | capture | warm | 2026-07-04 | ui-components |

## uniffi-migration (2 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [facade-codegen](guides/facade-codegen.md) | Facade Codegen: Registry Fields and Accessor Shapes | The codegen `FacadeRow` registry carries a `rust_module` field with a serde default of `"facade"`, so existing registries that omit the field produce byte-ident | capture | warm | 2026-07-04 | uniffi-migration |
| [uniffi-migration](guides/uniffi-migration.md) | M14 UniFFI Native Surface Migration | The M14 epic (#2125) collapses the native public binding surface to UniFFI: one public UniFFI surface serves iOS and Android, with FlatBuffers `Vec<u8>` bytes r | capture | warm | 2026-06-29 | uniffi-migration |

## wallet-architecture (6 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [check-state-reconciliation](guides/check-state-reconciliation.md) | Check-State Pass (NUT-07) and Money-Safe Reconciliation | The check-state pass groups held proofs by canonical mint and calls `MintClient::check_state` once per distinct mint | capture | warm | 2026-07-04 | wallet-architecture |
| [cross-mint-transfer](guides/cross-mint-transfer.md) | Cross-Mint Nutzap Transfer Saga | NMP implements cross-mint nutzap funding via Lightning: when no recipient-accepted mint has balance, it gets a mint-quote (bolt11) from the recipient's target m | capture | warm | 2026-07-04 | wallet-architecture |
| [mint-url-canonicalization](guides/mint-url-canonicalization.md) | Mint URL Canonicalization | The `canonicalize_mint_url` function lowercases only the scheme and authority (split at the first of `/`, `?`, or `#`), strips exactly one trailing slash from t | capture | warm | 2026-07-04 | wallet-architecture |
| [nutsack-wallet-poc](guides/nutsack-wallet-poc.md) | Nutsack Wallet PoC and Test Harness | The nutsack PoC repo lives at `/Users/pablofernandez/Work/nutsack` | capture | warm | 2026-07-03 | wallet-architecture |
| [nutzap-verification-gate](guides/nutzap-verification-gate.md) | Nutzap Verification Gate: DLEQ, P2PK, and Fail-Closed Checks | The redeem verification gate checks mint-trust â pubkey â P2PK-lock â privkey â DLEQ â fold/publish in that order | capture | warm | 2026-07-04 | wallet-architecture |
| [wallet-architecture](guides/wallet-architecture.md) | Wallet Architecture and Money-Safety | The wallet architecture uses a single WalletBackend trait with two backends: NWC (NIP-47) for Lightning/BOLT-11 and Cashu (NIP-60) for ecash | capture | warm | 2026-07-03 | wallet-architecture |

## write-pipeline (3 guides)

| Slug | Title | Summary | Tags | Volatility | Verified | Topic |
|------|-------|---------|------|------------|----------|-------|
| [chirp-publish-bug](guides/chirp-publish-bug.md) | Chirp Publish Bug: Silent Write Failures (chirp#69) | chirp#69 is a systemic write-publish bug: replaceable event kinds â kind:0 (profile edit), kind:3 (follow/unfollow), and kind:6 (repost) â silently fail to | capture | warm | 2026-07-04 | write-pipeline |
| [relay-provenance](guides/relay-provenance.md) | Relay Route Provenance and Access Control | Every explicit relay route must carry a typed provenance class: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic | capture | warm | 2026-06-29 | write-pipeline |
| [write-pipeline](guides/write-pipeline.md) | The Write Pipeline: Construction, Signing, Publishing | The 'door' metaphor refers to the app-facing API surface: the read door is how an app consumes data (typed read sessions), and the write door is how an app prod | capture | warm | 2026-06-29 | write-pipeline |

## Research Records (28 records)

| Record | Date | Finding | Agent |
|--------|------|---------|-------|
| [2026-06-29-1-codegen-extensibility-trace-can-app-owned](research/2026-06-29-1-codegen-extensibility-trace-can-app-owned.md) | 2026-06-29 | Codegen extensibility trace: can app-owned event kinds get first-class generated typed builders? Verdict: NO — codegen is hardcoded to NMP's own NIPs with no external registry hook | a96c931d134848ae1 |
| [2026-06-29-1-codegen-pipeline-trace-investigating-whether-app](research/2026-06-29-1-codegen-pipeline-trace-investigating-whether-app.md) | 2026-06-29 | Codegen pipeline trace investigating whether app-owned event kinds can get first-class generated typed builders; verdict: NO — codegen is hardcoded to NMP's schemas, no extensibility hook exists | a96c931d134848ae1 |
| [2026-06-29-1-codegen-seam-investigation-tracing-whether-app](research/2026-06-29-1-codegen-seam-investigation-tracing-whether-app.md) | 2026-06-29 | Codegen seam investigation tracing whether app-owned event kinds get first-class generated builders; verdict: NO, with file:line evidence across 5 evidence sections | a96c931d134848ae1 |
| [2026-06-29-1-codegen-seam-trace-investigation-verdict-no](research/2026-06-29-1-codegen-seam-trace-investigation-verdict-no.md) | 2026-06-29 | Codegen seam trace investigation: verdict NO — app-owned event kinds cannot get first-class typed write builders today, with file:line evidence across 5 evidence sections and gap analysis | a96c931d134848ae1 |
| [2026-06-29-1-investigation-of-whether-app-defined-event](research/2026-06-29-1-investigation-of-whether-app-defined-event.md) | 2026-06-29 | Investigation of whether app-defined event kinds can get first-class typed builders in their own codebase; verdict: NO, codegen pipeline is hardcoded to NMP only | a96c931d134848ae1 |
| [2026-06-29-1-uniffi-vs-c-abi-callback-byte](research/2026-06-29-1-uniffi-vs-c-abi-callback-byte.md) | 2026-06-29 | UniFFI vs C-ABI callback byte-push transport A/B microbench, methodology_sound, verdict COLLAPSE (pure UniFFI adoption confirmed, weighted-p99 delta 2142ns = 0.013% of 60fps budget, 390× below collapse threshold) | main |
| [2026-06-29-1-uniffi-vs-c-callback-byte-push](research/2026-06-29-1-uniffi-vs-c-callback-byte-push.md) | 2026-06-29 | UniFFI-vs-C-callback byte-push A/B benchmark (#2388): pre-registered COLLAPSE/KEEP/ESCALATE decision rule against 60fps budget thresholds; measured surcharged weighted-p99 delta = 2,142 ns (0.013% of 16.67ms budget), verdict COLLAPSE — pure UniFFI confirmed, ~390× below threshold | main |
| [2026-06-29-1-uniffi-vs-c-callback-byte-transport](research/2026-06-29-1-uniffi-vs-c-callback-byte-transport.md) | 2026-06-29 | UniFFI vs C-callback byte-transport A/B microbenchmark — COLLAPSE verdict: pure UniFFI sufficient, surcharged weighted-p99 delta 2,142 ns = 0.013% of 60fps budget, ~390× below pre-registered threshold | subagent (workflow #2388) |
| [2026-06-29-1-uniffi-vs-c-lane-byte-push](research/2026-06-29-1-uniffi-vs-c-lane-byte-push.md) | 2026-06-29 | UniFFI vs C-lane byte-push transport A/B microbench; pre-registered decision rule and verdict bands; COLLAPSE verdict (pure UniFFI sufficient) | workflow |
| [2026-06-29-1-uniffi-vs-c-lane-byte-transport](research/2026-06-29-1-uniffi-vs-c-lane-byte-transport.md) | 2026-06-29 | UniFFI vs C-lane byte transport benchmark: COLLAPSE verdict — surcharged weighted-p99 delta 2,142 ns is 390× below pre-registered threshold across 3 runs | main |
| [2026-07-03-1-independent-read-only-coherence-audit-of](research/2026-07-03-1-independent-read-only-coherence-audit-of.md) | 2026-07-03 | Independent read-only coherence audit of NIP-60/61 wallet surface (design↔epic↔crates↔nips.md), verdict: COHERENT-WITH-FINDINGS, four ranked findings F1–F4 plus ordered remaining-work list W1–W13 | wallet-audit |
| [2026-07-03-1-read-only-coherence-audit-of-nip](research/2026-07-03-1-read-only-coherence-audit-of-nip.md) | 2026-07-03 | Read-only coherence audit of NIP-60/61 wallet implementation vs design doc; verdict: COHERENT-WITH-FINDINGS, 4 ranked defects (F2 HIGH PaymentPort ownership drift, F1 MEDIUM double-exclusive claim, F3/F4 stale-doc), plus cleared non-findings and ordered remaining-work list | wallet-audit |
| [2026-07-03-1-wallet-coherence-audit-read-only-verification](research/2026-07-03-1-wallet-coherence-audit-read-only-verification.md) | 2026-07-03 | Wallet coherence audit: read-only verification of design↔epic↔crates↔nips.md alignment, verdict COHERENT-WITH-FINDINGS, 4 ranked findings (F2 PaymentPort ownership drift, F1 double-exclusive claim, F3/F4 stale docs) plus cleared non-findings and ordered remaining-work plan | wallet-audit |
| [2026-07-03-1-wallet-spine-subagent-completion-report-codex](research/2026-07-03-1-wallet-spine-subagent-completion-report-codex.md) | 2026-07-03 | Wallet-spine subagent completion report: codex-design-first build of journal/reducer/trail spine with 4 pre-registered invariants mapped to tests, 4 bugs caught by codex review, 23+2 integration tests passing, compat aliases hard-broken | wallet-spine |
| [2026-07-04-1-fable-subagent-verdict-on-demand-driven](research/2026-07-04-1-fable-subagent-verdict-on-demand-driven.md) | 2026-07-04 | Fable subagent verdict on demand-driven projection decode design: NOT worth pursuing, citing prior benchmark threshold and code-level invariant analysis | a6a0378d22b82e1bc (Fable) |
| [2026-07-04-1-ios-sweep-batch-2-s11-s17](research/2026-07-04-1-ios-sweep-batch-2-s11-s17.md) | 2026-07-04 | iOS sweep Batch 2 (S11-S17): real sim testing of timeline, empty/loading states, scroll, pull-refresh, live note insertion — 5 PASS, 2 PARTIAL, 1 bug filed | ios-tester-b2-sonnet |
| [2026-07-04-1-ios-validation-batch-2-sonnet-report](research/2026-07-04-1-ios-validation-batch-2-sonnet-report.md) | 2026-07-04 | iOS validation Batch 2 (Sonnet) report: 7 scenarios S11-S17 executed on sim with xcode MCP tools, verdicts 5 PASS / 2 PARTIAL, bug #68 filed for missing pull-to-refresh indicator | ios-tester-b2-sonnet |
| [2026-07-04-1-ios-validation-batch-4-s28-s36](research/2026-07-04-1-ios-validation-batch-4-s28-s36.md) | 2026-07-04 | iOS validation Batch 4 (S28–S36): scenarios executed on simulator, results table with PASS/FAIL verdicts and empirical findings (counts, zap-button verification, tap-target measurements) | ios-tester-b4 |
| [2026-07-04-1-ios-validation-sweep-batch-2-s11](research/2026-07-04-1-ios-validation-sweep-batch-2-s11.md) | 2026-07-04 | iOS validation sweep Batch 2 (S11–S17): 7 scenarios executed on simulator with real screenshots/video, 5 PASS / 2 PARTIAL, bug #68 filed | ios-tester-b2-sonnet |
| [2026-07-04-1-v1-blocker-triage-of-4-issues](research/2026-07-04-1-v1-blocker-triage-of-4-issues.md) | 2026-07-04 | v1-blocker triage of 4 issues (#2864, #2858, #2927, #2974): verdict NONE are genuine v1 code blockers; CLOSE #2927, DEFER-POST-V1 for rest | review-v1-scope |
| [2026-07-04-1-v1-scope-triage-of-4-issues](research/2026-07-04-1-v1-scope-triage-of-4-issues.md) | 2026-07-04 | V1-scope triage of 4 issues: grep-verified crate/code presence on master against docs/nips.md criteria, verdict NONE are v1 blockers (DEFER-POST-V1/CLOSE) | review-v1-scope |
| [2026-07-04-2-ios-sweep-batch-5-s37-s40](research/2026-07-04-2-ios-sweep-batch-5-s37-s40.md) | 2026-07-04 | iOS sweep Batch 5 (S37-S40, S62-S64): profile/accounts testing — found systemic write-publish failure where UI lies about success; mostly PASS with FAIL dispatch verdicts | ios-tester-b5 |
| [2026-07-04-2-ios-validation-batch-2-retry-s11](research/2026-07-04-2-ios-validation-batch-2-retry-s11.md) | 2026-07-04 | iOS validation Batch 2 retry (S11–S17): 7 scenarios executed with xcode MCP tools, verdicts 5 PASS / 2 PARTIAL, bug #68 filed, scroll-performance video captured | ios-tester-b2-sonnet |
| [2026-07-04-2-render-fix-reverification-of-pr-73](research/2026-07-04-2-render-fix-reverification-of-pr-73.md) | 2026-07-04 | Render-fix reverification of PR #73: 4/6 bugs FIXED with screenshot proof (a11y 0→138 elements, pbpaste hashtag tap test), 2/6 honestly STILL BROKEN root-caused to NMP#3016, CI green, merge recommendation | ios-render-fixer |
| [2026-07-04-2-triage-of-2970-nip-17-wss](research/2026-07-04-2-triage-of-2970-nip-17-wss.md) | 2026-07-04 | Triage of #2970 (NIP-17 wss-only gate) and #2993 (NIP-55 split): both DEFER post-v1 with empirical code-path analysis | review-misc-2970-2993 |
| [2026-07-04-3-render-fix-reverification-of-6-bugs](research/2026-07-04-3-render-fix-reverification-of-6-bugs.md) | 2026-07-04 | Render-fix reverification of 6 bugs (#62-67): 4 FIXED with screenshots+live tests, 2 STILL BROKEN root-caused to NMP embed-resolver; CI green, merge recommended for 4/6 | ios-render-fixer |
| [2026-07-04-4-ios-sweep-batch-7-groups-s45](research/2026-07-04-4-ios-sweep-batch-7-groups-s45.md) | 2026-07-04 | iOS sweep Batch 7 Groups (S45-53): NIP-29 and Marmot/MLS group testing — found Marmot completely dead on iOS, 3 PASS / 4 FAIL / 2 BLOCKED, 3 bugs filed | ios-tester-b7-groups |
| [AGENTS](research/AGENTS.md) |  |  |  |

## Episode Cards (21 cards)

| Card | Date | Title | Salience | Status |
|------|------|-------|----------|--------|
| [2026-06-29-1-collapse-uniffi-performance-assumption-unify-to](episodes/2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.md) | 2026-06-29 | Collapse UniFFI performance assumption; unify to single-surface architecture | reversal | active |
| [2026-06-29-1-disable-perf-gates-during-clean-break](episodes/2026-06-29-1-disable-perf-gates-during-clean-break.md) | 2026-06-29 | Disable perf-gates during clean-break migration | architecture | active |
| [2026-07-03-1-nmp-wallet-crate-already-on-master](episodes/2026-07-03-1-nmp-wallet-crate-already-on-master.md) | 2026-07-03 | nmp-wallet crate already on master — W3 unblocked, wave sequencing corrected | root-cause | active |
| [2026-07-03-1-npm-scope-renamed-from-nmp-to](episodes/2026-07-03-1-npm-scope-renamed-from-nmp-to.md) | 2026-07-03 | npm scope renamed from @nmp to @nmpis | reversal | active |
| [2026-07-03-1-read-model-collapse-concept-owned-active](episodes/2026-07-03-1-read-model-collapse-concept-owned-active.md) | 2026-07-03 | Read-model collapse: concept-owned active reads as sole public model | architecture | superseded |
| [2026-07-03-1-trellis-excluded-as-substrate-for-wallet](episodes/2026-07-03-1-trellis-excluded-as-substrate-for-wallet.md) | 2026-07-03 | Trellis excluded as substrate for wallet money-safety journal | architecture | active |
| [2026-07-03-1-wallet-builder-doc-re-grounded-phase](episodes/2026-07-03-1-wallet-builder-doc-re-grounded-phase.md) | 2026-07-03 | Wallet builder doc re-grounded: Phase-1 code already landed | root-cause | active |
| [2026-07-03-1-walletbackend-trait-deleted-false-pay-invoice](episodes/2026-07-03-1-walletbackend-trait-deleted-false-pay-invoice.md) | 2026-07-03 | WalletBackend trait deleted — false pay_invoice surface removed from nmp-nip60 | architecture | active |
| [2026-07-03-2-kind-17375-relay-tags-demoted-from](episodes/2026-07-03-2-kind-17375-relay-tags-demoted-from.md) | 2026-07-03 | kind:17375 relay tags demoted from authoritative to legacy hints | product | active |
| [2026-07-03-2-nmp-doctor-scoped-into-existence-with](episodes/2026-07-03-2-nmp-doctor-scoped-into-existence-with.md) | 2026-07-03 | nmp doctor scoped into existence with narrow diagnostic charter | product | active |
| [2026-07-03-2-read-lifecycle-engine-spine-one-internal](episodes/2026-07-03-2-read-lifecycle-engine-spine-one-internal.md) | 2026-07-03 | Read-lifecycle engine spine: one internal engine with concept-crate doorways | architecture | active |
| [2026-07-03-2-wallet-milestone-deferral-silently-reversed-by](episodes/2026-07-03-2-wallet-milestone-deferral-silently-reversed-by.md) | 2026-07-03 | Wallet milestone deferral silently reversed by merged PR #2854 | reversal | active |
| [2026-07-03-3-identity-free-concept-reads-raw-output](episodes/2026-07-03-3-identity-free-concept-reads-raw-output.md) | 2026-07-03 | Identity-free concept reads: raw output only, no viewer parameter | product | active |
| [2026-07-03-3-internal-path-dep-version-pinning-unblocks](episodes/2026-07-03-3-internal-path-dep-version-pinning-unblocks.md) | 2026-07-03 | Internal path-dep version pinning unblocks crates.io publishing | root-cause | active |
| [2026-07-03-4-rc-release-strategy-confirmed-and-release](episodes/2026-07-03-4-rc-release-strategy-confirmed-and-release.md) | 2026-07-03 | RC release strategy confirmed and release pipeline validated end-to-end | direction | active |
| [2026-07-03-4-static-readspec-cannot-route-nip-09](episodes/2026-07-03-4-static-readspec-cannot-route-nip-09.md) | 2026-07-03 | Static ReadSpec cannot route NIP-09 retractions of not-yet-seen events | root-cause | active |
| [2026-07-04-1-flatbuffervector-slow-accessor-neutering-per-byte](episodes/2026-07-04-1-flatbuffervector-slow-accessor-neutering-per-byte.md) | 2026-07-04 | FlatbufferVector slow-accessor neutering: per-byte copy banned, bulk-pointer copy mandated | root-cause | active |
| [2026-07-04-1-v1-scope-boundary-four-ambiguous-epics](episodes/2026-07-04-1-v1-scope-boundary-four-ambiguous-epics.md) | 2026-07-04 | V1 scope boundary: four ambiguous epics ruled non-blockers, v1 = owner-gated publish only | direction | active |
| [2026-07-04-2-nip-17-wss-only-parser-gate](episodes/2026-07-04-2-nip-17-wss-only-parser-gate.md) | 2026-07-04 | NIP-17 wss-only parser gate is a security invariant — rejected test-convenience relaxation | architecture | active |
| [2026-07-04-3-signer-onboarding-projection-split-deferred-duplicate](episodes/2026-07-04-3-signer-onboarding-projection-split-deferred-duplicate.md) | 2026-07-04 | Signer onboarding projection split deferred — duplicate-path risk with existing projections | architecture | active |
| [2026-07-04-4-kind-39000-embed-projection-is-a](episodes/2026-07-04-4-kind-39000-embed-projection-is-a.md) | 2026-07-04 | kind:39000 embed projection is a cross-cutting wire change, not a registry add | root-cause | active |

## Nouns (163 entities)

| Noun | Name | Origin | Definition |
|------|------|--------|------------|
| [a-read](nouns/a-read.md) | a read | extracted | Opened through an explicit concept-owned API, executed by one shared internal lifecycle engine, and rendered from typed output. Apps never assemble interests, observers, projections, source reducers, generic sessions, or Trellis resources. |
| [a-read-clean-architecture](nouns/a-read-clean-architecture.md) | a read (clean architecture) | extracted | Opened through an explicit concept-owned API, executed by one shared internal lifecycle engine, and rendered from typed output. Apps never assemble interests, observers, projections, source reducers, generic sessions, or Trellis resources. |
| [about](nouns/about.md) | --about | extracted | A durable room description used when creating a channel — short, descriptive, and stable; not status or current-plan text. |
| [action-builders](nouns/action-builders.md) | ACTION_BUILDERS | extracted | A hardcoded public const array of hand-coded ActionBuilder struct entries — the schema source-of-truth for the action-builder codegen pipeline. Contains only NMP's built-in NIPs with no mechanism for external/app-provided entries. |
| [actionmodule](nouns/actionmodule.md) | ActionModule | extracted | the per-NIP adapter that owns write action execution; implements the ActionModule trait with execute() entry point; receives generic signer capability and routes by provenance |
| [adr-directory](nouns/adr-directory.md) | ADR directory | extracted | The current architecture decision surface. It contains only ADRs that still own |
| [adrs](nouns/adrs.md) | ADRs | extracted | The decision spine — they are not the fast developer architecture guide. The ADR file itself is authoritative for its own status; the index is only a navigation aid. |
| [app-owned-kinds](nouns/app-owned-kinds.md) | App-owned kinds | extracted | event kinds (with custom schemas/numbers) defined and owned within an app's own codebase, distinct from NMP's built-in NIPs; executed via ActionModule registration at composition root |
| [architecture-decision-record-adr](nouns/architecture-decision-record-adr.md) | Architecture Decision Record (ADR) | extracted | a document that preserves durable decision context; must be edited in place to describe the current design rather than preserving outdated guidance |
| [available-unavailable](nouns/available-unavailable.md) | @available(*, unavailable) | extracted | A Swift compiler attribute (normally used for platform/version availability) whose `unavailable` variant says the symbol can never be referenced from Swift source on any platform — enforced as a hard compiler error, not a warning. In this project, applied to generated FlatBuffers slow byte-vector accessors to make misuse a compile error pointing at the fast `withUnsafePointerToPayload` accessor instead. |
| [builtin-registry-sections](nouns/builtin-registry-sections.md) | BUILTIN_REGISTRY_SECTIONS | extracted | include_str! of per-platform `nmp add` install manifests (registry.toml, registry.swiftui.toml, etc.), not a hardcoded section list duplicating the gallery catalog. |
| [cachedeventlookup](nouns/cachedeventlookup.md) | CachedEventLookup | extracted | nmp-wallet's narrow capability to read another account's kind:10019 (and other events) without nmp-core learning wallet/Cashu/nutzap nouns — provides event_by_id and latest_author_kind on ProtocolCommandContext, attached via a builder, mirroring the ZapProfileLookup precedent |
| [cashu-crate-vs-cdk](nouns/cashu-crate-vs-cdk.md) | cashu crate (vs CDK) | extracted | The modular audited `cashu` crate (pure BDHKE/DLEQ/P2PK primitives, zero I/O) — viable in NMP because its secp256k1 stack dedupes to the exact versions already in-tree via nostr; distinct from the async `cdk` wallet crate which bundles its own networking/storage and doesn't fit the substrate |
| [causal-trail](nouns/causal-trail.md) | causal trail | extracted | An in-memory, annotated delta log answering 'why is the wallet state this shape' — a bounded time-ordered ring of post-observation facts (token added/deleted, mint probe verdicts, nutzap redeemed, saga transitions) plus a per-atom last-cause index; rebuildable, non-money-critical, never a rebuild authority |
| [channels](nouns/channels.md) | channels | extracted | Rooms of shared attention; the current channel is where messages, context, and coordination belong by default. |
| [chirp](nouns/chirp.md) | Chirp | extracted | A real Nostr client product extracted to an external repository as proof of consumability from outside the monorepo |
| [chirp-pane](nouns/chirp-pane.md) | Chirp pane | extracted | The visible visual X-Ray panel inside the Chirp app — the flagship consumer of the X-Ray diagnostic surface. |
| [chirp-x-ray-pane](nouns/chirp-x-ray-pane.md) | Chirp X-Ray pane | extracted | The actual visual X-Ray panel rendered in the Chirp app; the flagship consumer of the X-Ray diagnostic surface. |
| [clean-break-nmp-app-architecture-migration-epic-ns-001](nouns/clean-break-nmp-app-architecture-migration-epic-ns-001.md) | Clean-break NMP app architecture migration (EPIC-NS-001) | extracted | the single overarching north-star epic; a coordinated three-reform redesign (read door / composition root / write door) applied lockstep across native/browser/starter targets with doctrine-lint ratchets |
| [clean-break-redesign](nouns/clean-break-redesign.md) | clean-break redesign | extracted | A coordinated three-reform redesign governed by ADR spine 0069–0073: kill register_defaults() (explicit composition), retire raw interest/feed C ABI in favor of typed sessions (read door), and classify publish routes with explicit pre-sign provenance (write door), applied to native, browser, and starter targets with doctrine-lint ratchets locking each slice shut. |
| [cli-install-registry](nouns/cli-install-registry.md) | CLI install registry | extracted | per-platform component install manifests; what `nmp add` scaffolds |
| [cli-install-registry-nmp-cli-registry](nouns/cli-install-registry-nmp-cli-registry.md) | CLI install registry (nmp-cli registry) | extracted | crates/nmp-cli/registry/*.toml — per-platform install manifests for `nmp add` (version + source files + deps), split by platform target for the 500-LOC gate. A legitimately separate concern from the gallery showcase catalog. |
| [collapse-verdict](nouns/collapse-verdict.md) | COLLAPSE verdict | extracted | The measured benchmark result that pure UniFFI has sufficient performance for all surfaces including the hot update-sink push lane, eliminating the need for an internal byte-ABI exception |
| [composition-root](nouns/composition-root.md) | composition root | extracted | The only place where a production app chooses product policy; explicitly installs substrate, reusable Nostr protocol features, app-owned product features, shell capability contracts, then starts the runtime |
| [concept-owned-active-read](nouns/concept-owned-active-read.md) | concept-owned active read | extracted | A kept-live query with a close handle; it opens demand, replays cache/store, pushes typed output while mounted, then closes exact demand. Each concept crate owns its own open_<concept> doorway; NMP core does not grow a slot per concept. |
| [concept-owned-ffi-bridge](nouns/concept-owned-ffi-bridge.md) | concept-owned FFI bridge | extracted | Each concept crate ships the FFI-shaped half of its own doorway (round-trippable handle parts, scalar/flat inputs, typed errors), and nmp-codegen generates each app's #[uniffi::export] facade slice plus Swift/Kotlin wrappers from a per-app registry file listing only the concepts that app composes. No central crate gains a concept dependency. |
| [concept-read-codegen](nouns/concept-read-codegen.md) | concept-read codegen | extracted | Each concept crate ships the FFI-shaped half of its own doorway (round-trippable handle parts, scalar/flat inputs, typed errors), and nmp-codegen generates each app's #[uniffi::export] facade slice plus Swift/Kotlin wrappers from a per-app JSON registry listing only the concepts that app composes. No central crate gains a dependency on any concept crate — codegen emits text naming concept symbols, it never links them. |
| [conceptread](nouns/conceptread.md) | ConceptRead | extracted | A three-stage read lifecycle: (1) Demand — what events/resources must be alive; (2) Admission + model — which matching facts matter and how they update state; (3) Output — what typed data is emitted to the host. The internal pattern concept owners parameterize, never exposed as a universal public trait. |
| [content-driven-projection-rev](nouns/content-driven-projection-rev.md) | content-driven projection_rev | extracted | For app-owned (non-manifest) keys, a per-key counter that increments when the payload fingerprint changes — so the rev advances iff content changed. Cleared rows keep rev 0; built-in keys derive rev from source-version write-chokepoint counters instead. |
| [deletion-ledger](nouns/deletion-ledger.md) | deletion ledger | extracted | Per-architecture-slice accounting of old doors deleted/privatized, new concepts, and net permanent concepts — tracks the concept/surface collapse that LOC numbers understates |
| [dispatch-success](nouns/dispatch-success.md) | Dispatch ≠ success | extracted | the doctrine that dispatching an event to publish pipeline is orthogonal from confirming receipt on relays; Rust owns tracked status intent that survives offline |
| [dispatchenvelope](nouns/dispatchenvelope.md) | DispatchEnvelope | extracted | FlatBuffers encoding for a typed action, pushed through dispatch_action — the single FFI command lane (legacy decision 0064), identical in shape across native and browser |
| [dispatchoutcome](nouns/dispatchoutcome.md) | DispatchOutcome | extracted | Typed result of action dispatch carrying correlation_id, error, and code field; replaces raw JSON for typed command execution |
| [doc-vocabulary-ratchets](nouns/doc-vocabulary-ratchets.md) | doc/vocabulary ratchets | extracted | CI lint gates that stop old register_defaults/raw-projection vocabulary from creeping back into docs and code. They are pro-migration — the opposite of friction-for-nothing — enforcing the clean-break by failing CI if old vocabulary re-enters. |
| [docs-nips-md](nouns/docs-nips-md.md) | docs/nips.md | extracted | The v1 pre-release truth source — the authoritative surface for determining which NIPs and features are in-scope for v1 versus post-v1. |
| [door-read-door-write-door](nouns/door-read-door-write-door.md) | door (read door / write door) | extracted | Metaphor for the app-facing API surface. Read door = how an app consumes data (typed read sessions). Write door = how an app produces and emits Nostr events (construct → sign → publish pipeline). |
| [dx-clean-room-proof-2256](nouns/dx-clean-room-proof-2256.md) | DX clean-room proof (#2256) | extracted | A DX gate requiring a fresh developer to build a small NMP app in ≤2h from published repo docs and starter artifacts alone, using only the clean-break public model (explicit composition root, typed read sessions, generated typed writes). It supersedes a prior clean-room proof that validated the wrong (pre-reset) model. Passing it unblocks release blocker #2121 and is the migration-readiness starting line. |
| [embedkindprojection](nouns/embedkindprojection.md) | EmbedKindProjection | extracted | An enum in nmp-content (embed_projection/variants.rs) with a typed XProjection struct per event kind, dispatched by a single match on event.kind (resolve_embed_projection); every variant must also be wired through FlatBuffers embed_sidecar wire, platform renderers, gallery previews, and registry manifests. |
| [env-rev-kernelsnapshot-rev](nouns/env-rev-kernelsnapshot-rev.md) | env.rev (KernelSnapshot.rev) | extracted | A frame-level counter that bumps on every emitted kernel snapshot tick; hosts gate on `env.rev > rev` to skip stale frames. Frozen env.rev means the host received no new frames. |
| [epic-ns-001](nouns/epic-ns-001.md) | EPIC-NS-001 | extracted | The single overarching north-star epic (#2340) driving the clean-break NMP app architecture migration — elimination of register_defaults(), raw open_interest, ObservedProjection/ReducedSource, platform-specific ABI choices, and compatibility shims; replaced with explicit composition, typed read sessions, construction/signing/publishing separation, and one UniFFI native surface. |
| [epic-ns-001-clean-break-redesign](nouns/epic-ns-001-clean-break-redesign.md) | EPIC-NS-001 (clean-break redesign) | extracted | The single overarching north-star p0 epic (#2340) driving the clean-break NMP app architecture migration. It governs a coordinated three-door reform — kill register_defaults, retire raw interest/feed C ABI in favor of typed sessions, classify publish routes with explicit pre-sign provenance — applied to native, browser, and starter targets in lockstep with doctrine-lint/doc ratchets locking each slice shut. |
| [explicit-composition-adr-0069](nouns/explicit-composition-adr-0069.md) | Explicit Composition (ADR-0069) | extracted | explicit wiring of app composition root at build time, replacing `register_defaults()` as the production path |
| [fabric](nouns/fabric.md) | fabric | extracted | The shared world: identity, presence, channels, relationships, and coordination that continue outside any single turn. |
| [fabric-snapshot](nouns/fabric-snapshot.md) | fabric snapshot | extracted | A hook-provided ambient-awareness blob telling the agent who it is, which channel it is in, who else is around, what changed recently, and which agents can be invited. |
| [feed](nouns/feed.md) | feed | extracted | Not a profile resolver, reply-count engine, thread hydrator, UI dependency planner, or general content augmentation surface. A feed owns primary item acquisition, source/perspective resolution, repost wrapper inclusion, windowing/order, and feed row output only. Counts, profiles, reactions, zaps, and thread hydration are mounted as separate concept-owned reads. |
| [feedsessions](nouns/feedsessions.md) | FeedSessions | extracted | The shipped public struct behind NmpApp::feeds() — the app-facing feed-session API whose type name leaks 'session' as public vocabulary. Targeted for rename to Feeds before the v1 freeze. |
| [first-class-for-app-owned-kinds](nouns/first-class-for-app-owned-kinds.md) | First-class (for app-owned kinds) | extracted | full generated-builder treatment (typed Rust builder, generated Swift/Kotlin/TS bindings, drift gating) within an app's own codebase, on par with NMP's built-in kinds; not hand-rolled ActionModules |
| [gallery-showcase-catalog](nouns/gallery-showcase-catalog.md) | Gallery showcase catalog | extracted | The platform-agnostic component catalog defined in `apps/nmp-gallery/registry.json` with `sections[]` structure, containing the canonical list of components the gallery demonstrates |
| [gallery-showcase-catalog-registry-json](nouns/gallery-showcase-catalog-registry-json.md) | gallery showcase catalog (registry.json) | extracted | platform-agnostic component showcase catalog; what the gallery demos |
| [host](nouns/host.md) | host | extracted | The current body the agent inhabits — Codex, Claude Code, opencode, or another harness. |
| [internal-lifecycle-engine](nouns/internal-lifecycle-engine.md) | internal lifecycle engine | extracted | A small internal engine with optional hooks for exceptional concept-owned phases — the genericity that #2508 rejected at the public surface moves one layer below it. Owns handle registry, replay-before-live, live activation, exact-demand withdrawal, reverse teardown, tombstoning, coalesced emission. Not a god-trait. |
| [is-known-valueless-mint](nouns/is-known-valueless-mint.md) | is_known_valueless_mint | extracted | A denylist function that excludes the testnut.cashu.space host family from cross-mint source candidacy — necessary because testnut is protocol-indistinguishable pre-melt (valid quote, only PENDING after the irreversible melt), making selection the only point a fake mint can be kept out of a real Lightning melt. |
| [issue-queue](nouns/issue-queue.md) | issue queue | extracted | The single canonical temporal tracker — supposed to hold deferred and future work. Not a museum; 'predates the epic' is not by itself a reason to remove anything. The test for each issue is whether it is still true, still independent, and not contradicted by the reset. |
| [keyedrefcache](nouns/keyedrefcache.md) | KeyedRefCache | extracted | A generated per-key row cache for keyed reference projections (refs.profile / refs.event), decoding nmp.refs.RefRowDeltaBatch payloads and merging row deltas under five invariants, byte-for-byte semantically identical to nmp_core::refs::RefRowCache and the Kotlin twin. Generated by nmp-codegen from swift_keyed_cache.rs template. |
| [keyedrefcache-generated-swift](nouns/keyedrefcache-generated-swift.md) | KeyedRefCache.generated.swift | extracted | A generated Swift file implementing ADR-0063 Lane A (#1671): a per-key row cache for keyed reference projections (refs.profile / refs.event). Decodes the nmp.refs.RefRowDeltaBatch payload and merges row deltas under five invariants — byte-for-byte semantically identical to nmp_core::refs::RefRowCache and the Kotlin twin. Generated by the nmp-codegen Rust tool, not raw flatc. |
| [kind-10019](nouns/kind-10019.md) | kind:10019 | extracted | NIP-61 nutzap info event — advertises this wallet's accepted mints and relays; the authoritative relay source for nutzap operations. |
| [kind-17375](nouns/kind-17375.md) | kind:17375 | extracted | NIP-60 wallet configuration event — encrypted with NIP-44 keyed to the owner's pubkey, stores the wallet's Cashu private key and the list of mint URLs. Its `relay` tags are demoted to a non-authoritative legacy hint (renamed `legacy_relay_hint`); kind:10019 + NIP-65 fallback is the authoritative relay source. |
| [kind-17375-wallet-event](nouns/kind-17375-wallet-event.md) | kind:17375 wallet event | extracted | The NIP-60 encrypted wallet configuration event — stores the wallet's Cashu private key and the list of mint URLs, encrypted with NIP-44 keyed to the owner's pubkey. Its relay tags were demoted from authoritative relay selection to a non-authoritative legacy hint; NIP-65 fallback is the design doc's authoritative source. |
| [kind-38172](nouns/kind-38172.md) | kind:38172 | extracted | NIP-88 Cashu mint announcement event — a mint publishes it to advertise itself on Nostr (mint URL, relay preferences, fees, contact info); used for mint discovery via Nostr search rather than hardcoded URLs. |
| [kind-7374](nouns/kind-7374.md) | kind:7374 | extracted | NIP-60 optional deposit quote tracking event. |
| [kind-7375](nouns/kind-7375.md) | kind:7375 | extracted | NIP-60 token event — unspent Cashu proofs. |
| [kind-7376](nouns/kind-7376.md) | kind:7376 | extracted | NIP-60 optional spending history event. |
| [kind-9321](nouns/kind-9321.md) | kind:9321 | extracted | NIP-61 nutzap event — carries P2PK-locked Cashu proofs sent to a recipient. |
| [legacy-relay-hint](nouns/legacy-relay-hint.md) | legacy_relay_hint | extracted | The renamed `relays` field on Nip60WalletHandle/WalletConfig — wallet metadata carrying the kind:17375 `relay` tags, now explicitly non-authoritative; callers should fall back to NIP-65 relays when absent. |
| [lifecycle-read-lifecycle](nouns/lifecycle-read-lifecycle.md) | lifecycle (read lifecycle) | extracted | The common skeleton identical regardless of what is being read: register demand with the kernel, replay cache/store before going live, subscribe live, admit/filter arriving events, fold into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically. What differs per concept is only the semantics (demand, admission, reducer, output encoder). |
| [lifecycle-read-skeleton](nouns/lifecycle-read-skeleton.md) | lifecycle (read skeleton) | extracted | The common spine identical across all concept reads: register demand with the kernel, replay cache/store before going live, subscribe live, admit/filter arriving events, fold into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically. What differs per concept is only the semantics (demand, admission, reducer, output encoder). |
| [lifecycle-the](nouns/lifecycle-the.md) | lifecycle (the) | extracted | The skeleton identical regardless of whether the thing being read is feed rows or zap totals: register demand, replay before live, subscribe live, admit/filter, fold into bounded model, emit typed output with coalescing, and on close tear down symmetrically. |
| [m14](nouns/m14.md) | M14 | extracted | The epic to collapse the native public binding surface to a single UniFFI surface for iOS/Android, replacing the transitional raw C/JNI ABI (nmp-ffi); FlatBuffers Vec<u8> bytes remain payload, wasm-bindgen stays separate for browser. |
| [m14-collapse-native-public-binding-surface-to-uniffi](nouns/m14-collapse-native-public-binding-surface-to-uniffi.md) | M14 – Collapse native public binding surface to UniFFI | extracted | post-v1 epic to consolidate native (iOS/Android) public FFI to one UniFFI surface; eliminate legacy C/JNI public lanes; 56 symbols migrate, zero internal-ABI exceptions |
| [migration-note](nouns/migration-note.md) | migration note | extracted | Consumer-facing migration checklist for a specific release tag; complements the durable target guide in docs/migration.md by naming the concrete breaks a pinned consumer must handle when crossing that release. |
| [mintannouncement](nouns/mintannouncement.md) | MintAnnouncement | extracted | Decoded kind:38172 mint announcement event — contains the mint URL (d tag as identifier), relay preferences, fees, and contact info. |
| [neuter-slow-flatbuffer-accessors-sh](nouns/neuter-slow-flatbuffer-accessors-sh.md) | neuter-slow-flatbuffer-accessors.sh | extracted | An idempotent script in chirp's apps/ios/scripts/ that finds every FlatbufferVector<UInt8> accessor with a fast sibling (withUnsafePointerTo<Name>) in Generated/*.generated.swift and prepends an @available(*, unavailable, ...) annotation, turning any future per-byte-copy misuse into a compile error. |
| [nmp-cli-component-install-registry](nouns/nmp-cli-component-install-registry.md) | nmp-cli component-install registry | extracted | Per-platform install manifests in `crates/nmp-cli/registry/*.toml` that define what components the `nmp add` scaffolding tool can install for each platform (swiftui, compose, tui, desktop, web) |
| [nmp-core-rule-rust-owns-durable-behavior](nouns/nmp-core-rule-rust-owns-durable-behavior.md) | NMP core rule (Rust owns durable behavior) | extracted | NMP inherits RMP's core rule: Rust owns durable behavior and each platform renders native UI. Anything a second platform would have to reimplement to stay correct (relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning) belongs in Rust. |
| [nmp-defaults](nouns/nmp-defaults.md) | nmp-defaults | extracted | Deleted composition bundle. Production and scaffold app roots compose explicit |
| [nmp-devtools](nouns/nmp-devtools.md) | nmp-devtools | extracted | A dev-only sidecar crate in this project, blessed as the diagnostic surface layer over Trellis reconciliation receipts. |
| [nmp-ffi-nmp-native-runtime-crate-roles](nouns/nmp-ffi-nmp-native-runtime-crate-roles.md) | nmp-ffi / nmp-native-runtime (crate roles) | extracted | nmp-ffi is C-ABI glue; nmp-native-runtime owns lifecycle. Under the UniFFI collapse, nmp-native-runtime also owns the FFI-free dispatch core that both C-ABI and UniFFI consume. |
| [nmp-gallery](nouns/nmp-gallery.md) | nmp-gallery | extracted | A conformance/regression harness — a storybook that proves every NMP component decodes and renders on every platform at HEAD, with value in breadth + currency, not shippability |
| [nmp-nip59-decrypt-only](nouns/nmp-nip59-decrypt-only.md) | nmp-nip59-decrypt-only | extracted | A PARKED crate excluded from the default workspace (issue #2289), mapping to a NIP-59 decrypt-only staticlib; not a workspace member. |
| [nmp-nip60](nouns/nmp-nip60.md) | nmp-nip60 | extracted | NMP crate for NIP-60 Cashu wallet + NIP-61 NutZap + NIP-88 mint discovery event codecs, Cashu proof/DLEQ/P2PK/rollover types, and pure shape validation. NIP mechanics only — backend selection, the wallet operation journal, and the WalletBackend seam live in nmp-wallet. Performs zero relay I/O; the kernel fetches events and feeds them in via ingest_* methods. |
| [nmp-nwc](nouns/nmp-nwc.md) | nmp-nwc | extracted | The NIP-47 Nostr Wallet Connect codec crate — owns URI parsing, NIP-44 encrypted request/response, kind:23194 builder, and kind:23195 decoder. Distinct from nmp-nip47 which owns the wallet runtime; nmp-nip47 depends on nmp-nwc. |
| [nmp-read-session](nouns/nmp-read-session.md) | nmp-read-session | extracted | A new Layer-4 crate that owns the single implementation of the read-lifecycle mechanics: ReadSessionRegistry (handle alloc + open/close + reverse teardown + one leak audit), open_read/close_read (replay-before-live, exact demand withdrawal, reverse teardown, typed-output tombstone), and the ReadHost seam. Required as a separate crate so the dependency arrow runs concept → engine ← runtime. |
| [nmp-uniffi](nouns/nmp-uniffi.md) | nmp-uniffi | extracted | Retired crate name. `crates/nmp-uniffi` was deleted in #2763 after the |
| [nmp-wallet](nouns/nmp-wallet.md) | nmp-wallet | extracted | A proposed new reusable Layer-4 composition crate that owns the Rust-side WalletBackend seam, backend selection, wallet projection, operation journal, and PaymentPort adapter. |
| [nmpgallery](nouns/nmpgallery.md) | NmpGallery | extracted | A cross-platform conformance/regression harness (storybook) proving every NMP component decodes and renders on every platform at HEAD — not a shippable product. Its value is breadth + currency, which is why it stays in-tree unlike product apps like Chirp. |
| [nostrinlinevideoplayer](nouns/nostrinlinevideoplayer.md) | NostrInlineVideoPlayer | extracted | A dedicated SwiftUI view that holds an AVPlayer in @State, constructed exactly once per view identity, replacing the previous inline `VideoPlayer(player: AVPlayer(url:))` pattern inside NostrContentView's body that rebuilt the entire AVPlayerViewController (with full KVO observer churn) on every SwiftUI re-render. |
| [notefeeditem](nouns/notefeeditem.md) | NoteFeedItem | extracted | The renamed TimelineEventCard — same fields (id, author_pubkey, kind, created_at, content, content_tree, relay_provenance), carries reposted_by: Option<RepostAttribution>, minus only relation_counts (removed by the collapse) plus a new hosted_group field. Lives in nmp-note-feed behind nmp-feed-session. |
| [nutsack](nouns/nutsack.md) | nutsack | extracted | A new external NIP-60/NIP-61 wallet PoC that consumes NMP by git-rev pin, serving the same external-consumer role as podcast-player / hl / 29er. |
| [nwc-backend-and-cashu-backend](nouns/nwc-backend-and-cashu-backend.md) | NWC backend and Cashu backend | extracted | The two backends behind the WalletBackend seam: NWC (NIP-47) is the Lightning/BOLT-11 backend; Cashu (NIP-60) is the ecash backend. |
| [observedprojection](nouns/observedprojection.md) | ObservedProjection | extracted | Private machinery (not app vocabulary) under ADR-0070's typed read sessions; carries replay-before-live and scoped delivery lessons but is no longer app-facing. |
| [observedprojection-reducedsource](nouns/observedprojection-reducedsource.md) | ObservedProjection / ReducedSource | extracted | Private machinery, not app vocabulary. They survive as internal replay/source-arrival mechanisms but are not exposed as the way app developers assemble product screens. |
| [one-internal-engine](nouns/one-internal-engine.md) | one internal engine | extracted | The public surface has many concept-shaped doors (open_feed, open_replies, etc.), but there is exactly one implementation of the lifecycle skeleton; each concept owner supplies only its semantic parameters (demand, admission, reducer, output), never the lifecycle mechanics. |
| [open-interest](nouns/open-interest.md) | open_interest | extracted | Acquisition-only substrate demoted from app-facing read model; the typed read session owns the complete lifecycle above it. |
| [parked-crates](nouns/parked-crates.md) | PARKED (crates) | extracted | A project status for crates excluded from the default workspace build and not workspace members — post-v1 crates tracked in GitHub Issues, built on demand via --manifest-path rather than as part of the normal workspace. |
| [paymentport](nouns/paymentport.md) | PaymentPort | extracted | The seam through which NIP-57 zaps emit a 'pay this' intent (PaymentIntent); the selected wallet backend fulfills it. Currently nmp-nip47 owns the PaymentPort implementation (WalletPaymentPort) injected into the zap chain at composition time. |
| [perf-gates](nouns/perf-gates.md) | perf-gates | extracted | Modelled (deterministic, no live relays) perf benchmarks promoted to PR-blocking gates — runs reactivity-bench and firehose-bench with reduced event volume to keep wall-clock under ~5 min while exercising all absolute gate thresholds. |
| [pre-signed-verbatim-publish](nouns/pre-signed-verbatim-publish.md) | Pre-signed verbatim publish | extracted | Publishing an already-signed event without re-signing, routed via the event's own pubkey outbox. Needed for protocol-owned events (e.g. Marmot/MLS wire events); WRITE-005 restricts it to those protocol seams so it can't be used as a general app write door. |
| [pre-v1](nouns/pre-v1.md) | pre-v1 | extracted | Defined by #2690's exit criteria — issues that must be completed and proven before the v1 publish act. In this session, only #2690 (the release train itself) and #2711 (upstream-blocked RUSTSEC) carry the pre-v1 label; all other open issues were ruled post-v1. |
| [presence](nouns/presence.md) | presence | extracted | Active channel membership; an agent may be idle or busy while present, but once it leaves membership it is offline for that room. |
| [projection-rev](nouns/projection-rev.md) | projection_rev | extracted | A per-key monotonic u64 that advances when content changes (ADR-0070 Rung 2 wire contract). App-owned keys absent from the kernel's builtin rev manifest must derive a content-driven rev so rev-aware host caches don't skip changed frames. |
| [projection-rev-app-owned-keys](nouns/projection-rev-app-owned-keys.md) | projection_rev (app-owned keys) | extracted | ADR-0070 Rung 2 wire contract: rev advances on content change. For app-owned (non-manifest) keys, a per-key content-fingerprint counter that increments when the payload changes — because the kernel has no write-chokepoint visibility into opaque host-registered projection payloads. Built-in (Tier-2) keys derive rev from SourceVersions counters instead. |
| [projection-rev-contract](nouns/projection-rev-contract.md) | projection_rev contract | extracted | Monotonic u64 per key that advances when content changes; Rung 3 omits Unchanged. Built-in keys derive rev from SourceVersions write-chokepoint counters; app-owned keys (absent from the kernel's builtin rev manifest) derive a content-driven rev via fingerprint comparison so the rev advances iff content changed. |
| [provenance-class](nouns/provenance-class.md) | Provenance class | extracted | A typed enumeration that classifies the source and permission basis for explicit relay routes: automatic, host-pin, verified-private-inbox, manual, imported, or diagnostic |
| [provenance-in-publish-context](nouns/provenance-in-publish-context.md) | Provenance (in publish context) | extracted | explicit classification of why/how a publish is routed (outbox relays, NIP-29 group, etc.); non-anonymous and predetermined; cannot default |
| [publish-intent](nouns/publish-intent.md) | publish intent | extracted | Separable construction → finalization → signing → publishing stages as one actor-owned workflow, with typed provenance class for each route (automatic / host-pin / verified-private-inbox / manual / imported / diagnostic) |
| [publish-provenance-class](nouns/publish-provenance-class.md) | publish provenance class | extracted | typed enum for the source/path of an explicit relay route: automatic / host-pin / verified-private-inbox / manual / imported / diagnostic |
| [publish-route-provenance-adr-0071](nouns/publish-route-provenance-adr-0071.md) | Publish Route Provenance (ADR-0071) | extracted | explicit classification of publish routes in core; finalization moved to pre-sign gate; dispatch orthogonal from success |
| [publish-signed-event-pre-signed-verbatim-publish](nouns/publish-signed-event-pre-signed-verbatim-publish.md) | publish_signed_event (pre-signed verbatim publish) | extracted | Publishing an already-signed event without re-signing, routed via the event's own pubkey outbox — needed for protocol-owned events (Marmot/MLS wire events); WRITE-005 restricts it to protocol-owned seams so it can't be used as a general app write door |
| [publishraw](nouns/publishraw.md) | PublishRaw | extracted | A generic low-level 'publish any bytes' seam. WRITE-001 banned it from DX/starter paths as the normal way to write; it remains underneath as the generic substrate builders are built on, but typed intentful writes are the sanctioned app-facing door. |
| [ratchets](nouns/ratchets.md) | ratchets | extracted | Mechanism that prevents old-pattern counts from rising — each slice must keep old public-surface counts flat-or-decreasing, or update the owning ADR in place |
| [read](nouns/read.md) | read | extracted | Opened through an explicit concept-owned API, executed by one shared internal lifecycle engine, and rendered from typed output. Apps never assemble interests, observers, projections, source reducers, generic sessions, or Trellis resources. |
| [read-architecture-concept](nouns/read-architecture-concept.md) | read (architecture concept) | extracted | Opened through an explicit concept-owned API, executed by one shared internal lifecycle engine, and rendered from typed output. Apps never assemble interests, observers, projections, source reducers, generic sessions, or Trellis resources. |
| [read-door](nouns/read-door.md) | Read door | extracted | The app-facing API surface for how an app consumes data — typed read sessions ('given the store, what should the UI see?'), replacing raw open_interest/ObservedProjection/ReducedSource which are now crate-private. |
| [read-lifecycle](nouns/read-lifecycle.md) | read lifecycle | extracted | The skeleton shared by every concept read: register demand with the kernel, replay what cache/store already has before going live, subscribe live, admit/filter arriving events, fold them into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically — withdraw exact demand in reverse order, plus tombstone output on account switch. Identical machinery regardless of whether the thing being read is feed rows or zap totals. |
| [read-lifecycle-engine](nouns/read-lifecycle-engine.md) | read lifecycle engine | extracted | One shared internal implementation of the read skeleton (demand registration, replay-before-live, live activation, admission delivery, typed output registration, coalesced emission, handle registry, symmetric close, account/source-switch tombstoning). Concept owners supply only semantic parameters (spec, demand compiler, admission predicate, reducer, output encoder, teardown policy); they never implement lifecycle code. The engine is private plumbing; public APIs stay concept-shaped. |
| [read-lifecycle-the-skeleton](nouns/read-lifecycle-the-skeleton.md) | read lifecycle (the skeleton) | extracted | The common spine every concept read shares: register demand with the kernel, replay cache/store before going live, subscribe live, admit/filter arriving events, fold them into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically — withdraw exact demand, in reverse order, plus tombstone on account switch. One implementation behind many concept-shaped doors. |
| [read-the-architecture-sentence](nouns/read-the-architecture-sentence.md) | read (the architecture sentence) | extracted | A read is opened through an explicit concept-owned API, executed by one shared internal lifecycle engine, and rendered from typed output. Apps never assemble interests, observers, projections, source reducers, generic sessions, or Trellis resources. |
| [readhost](nouns/readhost.md) | ReadHost | extracted | A small host/context seam that runtimes implement once and concept crates consume. It is the interface through which a concept-crate doorway (e.g. open_replies) drives the shared engine without depending on any specific runtime crate. The dependency arrow runs concept-crate → engine ← runtime. |
| [readhost-seam](nouns/readhost-seam.md) | ReadHost seam | extracted | A small host/context seam that runtimes implement (NmpApp implements it once, generically) and concept crates consume, so the dependency arrow runs concept-crate → engine ← runtime, never concept-crate → runtime. |
| [readspec](nouns/readspec.md) | ReadSpec | extracted | A fixed demand set compiled at open_read time — the concept owner supplies target/spec type, event demand compiler, admission predicate, event reducer, and typed output encoder as a static spec the engine drives. |
| [receipt-stream](nouns/receipt-stream.md) | receipt stream | extracted | The data substrate: ordered open/close/refresh events with NMP-owned shape, produced by the live feed reconciler. |
| [redesign-spine](nouns/redesign-spine.md) | Redesign spine | extracted | ADRs 0069 through 0073; the current set of architectural decision records that define the clean-break app architecture direction |
| [reducedsource](nouns/reducedsource.md) | ReducedSource | extracted | Private machinery (not app vocabulary) carrying source-arrival/source-withdrawal lessons — demoted from app-facing read architecture to internal implementation under typed read sessions. |
| [regen-flatbuffers-sh](nouns/regen-flatbuffers-sh.md) | regen-flatbuffers.sh | extracted | A script in chirp's apps/ios/scripts/ that regenerates the checked-in Swift FlatBuffers types for Chirp's typed wire decoders. Created (chirp#35) to make reproducible what was previously done by hand-running flatc --swift against the relevant .fbs schemas from the pinned NMP checkout. |
| [register-defaults](nouns/register-defaults.md) | register_defaults() | extracted | A preset/function that provided an opaque bundle of protocol features, runtimes, projections, and policy; the hidden composition model that app developers could call without understanding what they received |
| [registry-json](nouns/registry-json.md) | registry.json | extracted | The single source of truth for the NMP Gallery component showcase catalog; Rust hosts read the typed value directly, iOS reads the same JSON through a C-ABI accessor, and Android reads it through JNI |
| [registry-ui-components](nouns/registry-ui-components.md) | registry UI components | extracted | App-owned source packages in this project — they may render and report visible claim/release lifecycle through the app-level host/provider, but must not import platform runtimes, C ABI/JNI/WASM workers, or kernel. |
| [release-manifest](nouns/release-manifest.md) | release manifest | extracted | The release-readiness source of truth (release/nmp-release.toml); the dry-run runs `cargo package -p` over every public_crate, and external consumers pin public crates by git rev + version. |
| [release-manifest-nmp-release-toml](nouns/release-manifest-nmp-release-toml.md) | release manifest (nmp-release.toml) | extracted | Operational configuration, not a planning document — lists public crates, public npm packages, required gates, tag pattern, and publish mode for the NMP release train. |
| [rung3-omit](nouns/rung3-omit.md) | rung3_omit | extracted | A Rust kernel module (rung3_omit::omit_unchanged) that drops every Unchanged row before the frame is built — absence IS Unchanged. Chirp declares this capability via declareIncrementalApply; assertion-failure if not present. |
| [runtime-accessor-shape](nouns/runtime-accessor-shape.md) | runtime_accessor_shape | extracted | A concept-reads codegen registry field on FacadeRow with values 'ref' (default, emits self.<accessor>()) or 'closure' (emits self.<accessor>(\|app\| <concept_fn>(app, ...))); closure mode enables Android's guarded AppHandle::with_app accessor to participate in concept-read codegen. |
| [session](nouns/session.md) | session | extracted | Runtime bookkeeping, not a public noun. The word 'session' is internal vocabulary for the machinery that keeps a read alive; it must not appear in app-facing types, docs, or generated helpers. |
| [shared-lifecycle-engine-one-internal-engine](nouns/shared-lifecycle-engine-one-internal-engine.md) | shared lifecycle engine (one internal engine) | extracted | Exactly one implementation of the read-lifecycle skeleton (handle registry, replay-before-live, live activation, exact-demand withdrawal, reverse teardown, tombstoning, coalesced emission). The public surface has many concept-shaped doors, but the genericity lives one layer below — specific outside, generic inside. |
| [signer-state](nouns/signer-state.md) | signer_state | extracted | A pure recomputed output whose sole writer is project_signer_state (doctrine D4); the published wire/FFI shape is unchanged by the pending_signer_onboarding Option<SignerStateDto> field that bridges onboarding. |
| [signerprovenance](nouns/signerprovenance.md) | SignerProvenance | extracted | A typed enum that replaces the loose raw optional signer_pubkey in publish commands — names which identity is signing and whether it is known, with structured failure for unknown signers. |
| [source-reducer](nouns/source-reducer.md) | source reducer | extracted | Source resolution inside the read owner — private-only internal machinery, not taught in public docs. |
| [status-fact](nouns/status-fact.md) | status fact | extracted | A Rust-owned local publish intent / status object tracking the lifecycle of a write: pending → signed → stored → planned → sent → failed/exhausted. It makes dispatch ≠ success — the app gets honest, offline-first write state instead of fire-and-forget. |
| [tenex-edge](nouns/tenex-edge.md) | tenex-edge | extracted | An identity and awareness fabric for the coding agents you already run. |
| [time-travel](nouns/time-travel.md) | time travel | extracted | A feature that records and lets you scrub reconciliation transaction-by-transaction, delivered as trace capsules. |
| [trellis](nouns/trellis.md) | Trellis | extracted | A private reconciliation substrate below typed read sessions. Trellis owns generic mechanics (graph transactions, dependency identity, collection diffs, scoped teardown, output-frame lifecycle, deterministic replay); NMP owns Nostr and product meaning. Its graphs are in-memory, per-session, and die with the process — trellis-core has zero persistence. |
| [trellis-tenex-edge-channel](nouns/trellis-tenex-edge-channel.md) | trellis-tenex-edge channel | extracted | Live execution log channel for the Trellis adoption epic (#202): full cutover of subscriptions, status, and hook-context reconciliation onto Trellis with no split-brain, then container validation via the tenex-edge-dev live lab. |
| [typed-output](nouns/typed-output.md) | typed output | extracted | What the host renders — the typed data emitted from a read to the app/shell. It is the only public output noun; projections, snapshots, and observed sinks are private machinery behind it. |
| [typed-read-session](nouns/typed-read-session.md) | typed read session | extracted | One typed session descriptor+handle owning the complete read lifecycle (acquisition → route policy → bounded replay → live sink → admission → typed output → wake sources → teardown); `open_interest` and `ObservedProjection`/`ReducedSource` become private machinery, not app vocabulary |
| [typed-read-sessions](nouns/typed-read-sessions.md) | Typed read sessions | extracted | The app-visible read model: one typed session descriptor and handle owning the complete lifecycle from acquisition through route policy, bounded replay, live sink, admission predicates, typed output, wake sources, to teardown |
| [typed-read-sessions-adr-0070](nouns/typed-read-sessions-adr-0070.md) | Typed Read Sessions (ADR-0070) | extracted | the sole app-facing read API; replaces raw `open_interest`, `ObservedProjection`, `ReducedSource`, and raw feed/interest C-ABI with typed session doorways |
| [uniffi](nouns/uniffi.md) | UniFFI | extracted | A binding surface generator; not the data payload path, which FlatBuffers owns. The binding surface and the data payload are separable — FlatBuffer frames can be passed through UniFFI as bytes. |
| [uniffi-in-this-project](nouns/uniffi-in-this-project.md) | UniFFI (in this project) | extracted | A binding surface generator for native platforms; not the data payload path (FlatBuffers owns that). The decision is one UniFFI surface for all native (iOS/Android/desktop); browser stays wasm-bindgen. |
| [uniffi-nmp-native-strategy](nouns/uniffi-nmp-native-strategy.md) | UniFFI (NMP native strategy) | extracted | One unified UniFFI surface for all native platforms (iOS/Android/desktop); browser stays wasm-bindgen; FlatBuffers remains the typed payload passed THROUGH UniFFI bindings |
| [updatesink](nouns/updatesink.md) | UpdateSink | extracted | A UniFFI callback trait that receives typed update frames with proven quiescence semantics, verified to handle shutdown during in-flight callbacks without deadlock or use-after-free |
| [v1](nouns/v1.md) | v1 | extracted | The owner-gated publish act: name the framework, bump to 1.0.0, rehearse an RC, publish nmp-* crates to crates.io and @nmp/* to npm, and prove external consumption. The framework is done in substance but has never been published or externally consumed at a released version. |
| [v1-v1-release-train](nouns/v1-v1-release-train.md) | v1 (v1 release train) | extracted | The owner-gated publish act: name the framework, rehearse an RC, tag 1.0.0, publish nmp-* crates to crates.io and @nmp/* to npm, and prove external consumption — not a set of code features but an irreversible owner-authorized publishing milestone. |
| [wallet-nmp](nouns/wallet-nmp.md) | Wallet (NMP) | extracted | A Nostr event — wallet config (kind:17375) and token balances (kind:7375) live as NIP-44-encrypted events on relays; there is no local-only wallet, state syncs across devices via Nostr |
| [wallet-operation-journal](nouns/wallet-operation-journal.md) | wallet operation journal | extracted | A durable write-side saga with states Draft→MintPending→MintSettled→PublishPending→Settled (plus Unknown/Failed), persisted through NMP storage. Its defining requirement is surviving process death after an irreversible external effect (a mint spend) and reconciling against the mint as external authority to prevent double-spend. |
| [wallet-operation-journal-saga](nouns/wallet-operation-journal-saga.md) | wallet operation journal (saga) | extracted | A durable write-side saga (Draft→Prepared→MintPending→MintSettled→PublishPending→Settled/Unknown/Failed) whose entire purpose is at-most-once money safety — surviving process death after an irreversible external effect such as a mint spend |
| [wallet-project-sense](nouns/wallet-project-sense.md) | wallet (project sense) | extracted | Wallet state is Nostr events: wallet config (kind:17375) and token balances (kind:7375) live as NIP-44-encrypted events on relays, syncing across devices. There is no local-only wallet. |
| [wallet-projection](nouns/wallet-projection.md) | wallet projection | extracted | A bounded, screen-shaped typed snapshot ('wallet') showing backend id + readiness, capability flags, balances, and pending operations — what the UI renders; native/browser shells never choose mints, relays, or retry policy. |
| [walletbackend](nouns/walletbackend.md) | WalletBackend | extracted | A Rust trait representing the wallet backend seam. NWC (NIP-47) is the Lightning/BOLT-11 backend; Cashu (NIP-60) is the ecash backend. A composition layer selects which backend handles a given action. |
| [walletbackend-trait](nouns/walletbackend-trait.md) | WalletBackend trait | extracted | The command-shaped backend seam in nmp-wallet (backend.rs:82) with methods id, capabilities, snapshot, start_intent, on_wallet_event, on_mint_result — no blocking FFI calls, includes a reference stub impl. |
| [walletconfig](nouns/walletconfig.md) | WalletConfig | extracted | Decrypted content of a kind:17375 wallet event — holds the wallet's Cashu private key (hex) and the list of mint URLs, encrypted with NIP-44 keyed to the owner's pubkey. |
| [walletfact](nouns/walletfact.md) | WalletFact | extracted | A post-observation typed fact in the event-sourced wallet reducer stream; each carries a WHY and provenance. Variants include TokenAdded, TokenDeleted, MintProbed, NutzapRedeemed, SagaTransition, and StateRebuilt. Folded two ways: a bounded time-ordered delta ring (causal trail) and a per-atom last-cause index. |
| [write-door](nouns/write-door.md) | Write door | extracted | the app-facing API for producing/emitting Nostr events; structured as orthogonal construct → sign → publish phases with explicit provenance classification; dispatch ≠ success |
| [x-ray](nouns/x-ray.md) | X-Ray | extracted | A developer diagnostic tool that lets you or an agent ask why a feed is empty or why a subscription closed, and get a real answer from recorded receipts. |
| [x-ray-channel-trellis](nouns/x-ray-channel-trellis.md) | x-ray channel (trellis) | extracted | NMP X-Ray diagnostics coordination channel for issue #2858, scoped under the trellis project. |

