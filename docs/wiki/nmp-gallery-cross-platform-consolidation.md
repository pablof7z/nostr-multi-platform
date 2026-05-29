---
title: NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog
slug: nmp-gallery-cross-platform-consolidation
summary: The gallery component catalog and profile-merge business logic are duplicated across four platforms with drift; a single compile-time-embedded registry.json in nmp-app-gallery, exposed over C-ABI/JNI, eliminates the duplication and guarantees feature parity.
tags:
  - gallery
  - registry
  - cross-platform
  - catalog
  - profile-merge
  - architecture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog

> The gallery component catalog and profile-merge business logic are duplicated across four platforms with drift; a single compile-time-embedded registry.json in nmp-app-gallery, exposed over C-ABI/JNI, eliminates the duplication and guarantees feature parity.

## Current Gap: Catalog Drift

The gallery's component catalog is defined separately on each platform, and they have already diverged. TUI has 3 sections (User, Content, Embeds & Kinds) with 15 components. Desktop re-exports TUI's list, creating an undesirable desktop-to-TUI crate coupling. iOS has 4 sections, including a relay/relay-list section absent everywhere else, and its label/description strings differ. Android has only 2 sections (User, Content) — missing embeds, relay, and relay-list entirely. [^6a951-1]


Android's gap is deeper than just missing sections. Android still builds display models from raw ClaimedEventWire plus Kotlin tag parsing, which violates ADR-0034. Full embed parity on Android requires the Compose EmbedKindProjection, NostrKindRegistry, and EmbeddedEvent stack (backlog item F-CR-02). Adding registry sections without that infrastructure is fake parity — the components would be listed but not renderable. [^6a951-18]

Feature parity requirement: ALL platforms must render every component in the registry, including relay-list. There are no placeholder-only components. TUI, desktop, and Android all implement relay-list rendering. After consolidation, all platforms start from the same solid baseline so new components can be added to all platforms simultaneously with a single registry.json edit. Android embed parity explicitly requires the Compose EmbedKindProjection, NostrKindRegistry, and EmbeddedEvent stack (backlog item F-CR-02) as a prerequisite — just adding registry sections without that infrastructure is fake parity. [^6a951-41]

The original architectural goal of this consolidation effort is to move business logic into shared Rust, with components that own their own reactivity. PR #787 achieved surface-level catalog parity but did not address the core architecture: identifying where per-platform business logic still lives, defining what "component owns reactivity" means in the nmp-gallery framework, and migrating that logic into shared Rust crates. An Opus agent audit was launched post-PR-merge to read the actual code and answer: where does per-platform business logic still exist, what does "component owns reactivity" look like in this framework, and what are the highest-value tasks that advance the real goal. [^6a951-45]

The Opus agent plan identified three categories of architectural violations: (1) Catalog drift — the component catalog is defined four separate times (tui/src/gallery.rs, RegistrySection.swift, RegistrySection.kt, and desktop borrowing from TUI) with iOS having a relay section others lack and Android missing the entire embed section. (2) Triplicated profile-merge business logic — the merge of claimed_profiles → author_view → mention_profiles is implemented three times in Rust/Swift/Kotlin with divergent precedence, which is business logic in native shells (the sharpest doctrine violation). (3) Polling violations — desktop uses iced::time::every(250ms) and Android uses a nextUpdate(250ms) timeout-loop, both violating D8, while iOS already uses push callbacks correctly. [^6a951-51]

After PR #787, the remaining architectural work is: (1) Migrate all four platforms to read from registry.json at runtime (replacing hardcoded REGISTRY_SECTIONS arrays), achieving the goal of one JSON edit propagating to all platforms. (2) Implement the gallery_profiles Rust projection (Step 7) to collapse the triplicated profile-merge logic in Swift/Kotlin/Rust into a single Rust projection. (3) Identify and eliminate remaining per-platform business logic beyond profile merging — the Opus agent audit (in progress) will enumerate all remaining doctrine violations. (4) Define the exact contract for component-owned reactivity in the gallery framework, so every component declares its data requirements and owns its fetch lifecycle identically across platforms. [^6a951-54]

The overarching goal of nmp-gallery consolidation is to make nmp-gallery a living proof that the NMP framework thesis holds: any Nostr component, once its business logic is written in Rust, renders correctly on every platform without that logic being reimplemented. The Rust kernel owns all data — fetching, merging, precedence, reactivity. Platforms contribute exactly one thing: pixels. Adding a component means writing its data logic once in nmp-app-gallery, then writing four thin render functions. It does not mean writing four separate profile-merge loops, four separate subscription managers, or four separate catalog arrays. When a Nostr component mounts, it declares its data need to the kernel and receives a push-driven, already-shaped result. It does not ask the app shell to pre-warm anything. It does not re-implement precedence rules. It just renders what the kernel hands it. The gallery is consistent not because four files are kept in sync — it is consistent because there is only one place where the logic lives. [^6a951-1]
## Current Gap: Triplicated Profile-Merge Business Logic

The merge of claimed_profiles → author_view → mention_profiles is implemented three separate times in Rust, Swift, and Kotlin — with different precedence in each. Rust applies mention before author_view; Swift and Kotlin do the opposite. This is business logic living in native shells, which is the sharpest doctrine violation. The fix is a single canonical merge precedence (claimed → author_view → mention_profiles-if-absent) implemented once in Rust and exposed as a host-side projection registered via the existing register_snapshot_projection seam. [^6a951-2]


The canonical profile-merge precedence (landed in PR #787, task 5) is implemented in tui/src/data.rs in LiveProfileMap::update_from_snapshot. The three-step order is: (1) claimed_profiles applied first (highest priority), (2) author_view.profile applied second — but only when has_profile is true, (3) mention_profiles applied last with an if !self.profiles.contains_key(pubkey) guard — only-if-absent (lowest priority). This order must be identical across all platforms. The TUI was fixed to match the Swift/Kotlin order; previously Rust applied mention before author_view, which was the wrong precedence. [^6a951-36]

The profile-merge fix in PR #787 (task 5) only fixed the Rust/TUI side to match the Swift/Kotlin precedence. The triplication itself — three separate implementations in Rust, Swift, and Kotlin — was not eliminated. The Swift and Kotlin merge bodies still exist and still contain business logic in native shells. The gallery_profiles projection (Step 7 of the plan, not yet implemented) will collapse all three into a single Rust projection registered via register_snapshot_projection, emitting raw {pubkey, npub, display_name, picture_url, nip05, about} fields so all three native merge bodies collapse to decoding one map. Until Step 7 lands, the doctrine violation persists. [^6a951-52]

The Opus audit after PR #787 confirmed three categories of remaining architectural violations. (1) Profile-merge business logic remains in three languages simultaneously — the claimed > author_view > mention precedence rule is hand-reimplemented identically in tui/src/data.rs:145-184 (Rust), ios/GalleryModel.swift:132-188 (Swift), and android/GalleryModel.kt:113-148 (Kotlin). Every time it changes, it must be fixed in three places. (2) registry.json has zero consumers — the single source of truth has no readers; all four platforms still use their own hardcoded REGISTRY_SECTIONS. The guard test is a deliberate no-op with visit_source_files defined but never called. (3) Desktop still pre-warms data centrally — gallery.rs:167-173 claims all profiles on every snapshot tick regardless of which component is visible, exactly the violation the user named. PR #787's work was mostly cosmetic (catalog sections, SF symbols, composables); the two genuinely architectural fixes were the de-polling tasks (desktop + Android). Everything else widened per-platform code surface. [^6a951-2]
## Current Gap: Polling Violations

Desktop polls via iced::time::every(250ms) → Message::Poll in gallery.rs lines 106-108. Android uses a nextUpdate(timeoutMs = 250L) timeout-loop in GalleryModel.kt lines 76-93. Both violate aim §8. iOS is already correct, using a push callback with no polling — this is the reference pattern. [^6a951-3]


Codex elevated these from cleanup to prerequisite: under repo doctrine, de-polling and the gallery_profiles Rust projection are required for platform consistency. They are not optional follow-ons. Steps 6 and 7 run in parallel with Step 1, not after the registry migration. [^6a951-20]

Desktop de-poll (landed in PR #787, task 3): Replaced the iced::time::every(250ms) → Message::Poll timer with a push-driven subscription using iced 0.14's Recipe API. The implementation requires tokio as a direct dependency (with "sync" feature) and the "advanced" feature flag on iced. The Cargo.toml gains tokio = { version = "1", features = ["sync"] } and iced gains the "advanced" feature. The bridge.rs replaces its Arc<Mutex<Option<Value>>> slot + timer approach with a tokio::sync::mpsc::unbounded_channel(), where the reader thread sends snapshots directly and the bridge exposes a take_snapshot_receiver() method. The gallery.rs subscription uses a SnapshotRecipe struct implementing iced::advanced::subscription::Recipe with a hash method (hashing "gallery-snapshot") and a stream method returning iced::futures::stream::BoxStream<'static, Message>. The stream uses iced::futures::stream::unfold with the unbounded receiver, mapping each recv().await to a Message::Snapshot(value). The subscription function takes the receiver via snapshot_rx.borrow_mut().take() and wraps it in iced::advanced::subscription::from_recipe(SnapshotRecipe(Mutex::new(Some(rx)))). [^6a951-33]

Android de-poll (landed in PR #787, task 4): Changed the bridge.nextUpdate(timeoutMs = 250L) to timeoutMs = 30_000L in GalleryModel.kt. This is a blocking recv — the kernel emits snapshots at ~4 Hz, and the 30s timeout is purely defensive. The change is a single-line parameter edit: the existing blocking recv call was already D8-compliant; it just had an unnecessarily short 250ms timeout that created a polling-like pattern. The 30s timeout means the thread blocks until a snapshot arrives or the defensive timeout fires after 30 seconds of silence. [^6a951-34]

Iced 0.14 subscription API facts: The subscription module is not at the top level — it is at iced::advanced::subscription (requires features = ["advanced"] in Cargo.toml). The module provides Recipe trait and from_recipe(), but not unfold(). The BoxStream type is at iced::futures::stream::BoxStream, not iced::futures::BoxStream. Tokio must be a direct dependency (not just transitive via iced) with features = ["sync"] to use tokio::sync::mpsc. These facts were discovered through compile errors during the task 3 implementation and corrected before the PR landed. [^6a951-39]
## Registry: Compile-Time Embed via include_str!

The registry.json file is embedded into the binary at compile time via include_str!, exactly like showcase-references.json is embedded in showcase.rs. The file only needs to exist at the repo path during compilation — it is never accessed at runtime. Running the desktop binary on another machine after cargo build works without the registry.json file present anywhere on that machine. The file location is apps/nmp-gallery/registry.json, sibling of showcase-references.json. [^6a951-4]


The file location is apps/nmp-gallery/registry.json, a sibling of showcase-references.json. The embed mechanism is const RAW_JSON: &str = include_str!("../../registry.json") in registry.rs, exactly mirroring showcase.rs line 14's pattern. After cargo build, the desktop binary runs on any machine without the registry.json file present — the JSON is baked into the binary at compile time. [^6a951-43]
## Registry: Data Schema

The registry is exposed as a GalleryRegistry struct with: schema (string), sections (vec of RegistrySection). Each RegistrySection has: id (string), label (string), components (vec of ComponentSpec). Each ComponentSpec has: id (stable slug, e.g. 'user-avatar'), label (native type name), description (string), category (ComponentCategory enum: User | Content | Embed | Relay), data (DataContract), variants (vec of VariantSpec — semantic variant data only, no fonts/colors). [^6a951-5]


Codex identified four deficiencies in the original schema that must be addressed: (1) The Profile DataContract must include the full Rust-formatted bech32 npub, not just hex — npub abbreviation stays in shells, but the full npub comes from Rust via a references table. (2) The ContentTree DataContract must capture actual ContentTreeWire templates (note/mention combos) that are currently hardcoded in each platform's pages. (3) The RelayList DataContract needs relay status input — iOS currently invents "connecting"/"connected" labels in the page itself; that logic cannot live in the registry schema without proper inputs. (4) A single label field breaks across platforms — iOS uses NostrMinimalContentView while TUI uses NostrMinimalContent. The schema needs per-platform renderKey entries instead of a single label. [^6a951-17]

The current registry.json (as shipped in PR #787) is a v1 skeleton without the full schema enhancements Codex identified. Four known deficiencies remain to be addressed in a follow-up: (1) Profile DataContract needs the full Rust-formatted bech32 npub via a references table — npub abbreviation stays in shells per platform choice, but the full npub must come from Rust; (2) ContentTree DataContract must capture actual ContentTreeWire templates (note/mention combos) currently hardcoded in each platform's pages; (3) RelayList DataContract needs relay status input fields — iOS currently invents connecting/connected labels in the page itself, and that logic cannot live in the registry schema without proper inputs; (4) A single label field breaks across platforms (iOS uses NostrMinimalContentView, TUI uses NostrMinimalContent) — the schema needs per-platform renderKey entries instead of a single label. [^6a951-40]

The registry.json shipped in PR #787 is a v1 skeleton with four known schema deficiencies identified by Codex: (1) Profile DataContract is missing the full Rust-formatted bech32 npub — it only has pubkey_ref, but shells need the full npub (not hex) for display; npub abbreviation stays in shells per platform choice, but the full npub string must come from Rust via a references table. (2) ContentTree DataContract does not capture the actual ContentTreeWire templates (note/mention combos) currently hardcoded in each platform's pages — the registry cannot drive content rendering without knowing the template structure. (3) RelayList DataContract has no relay status input fields — iOS currently invents connecting/connected labels in the page itself, which is business logic in the shell that the registry schema must accommodate with proper input fields. (4) A single label field breaks across platforms because native type names differ: iOS uses NostrMinimalContentView, TUI uses NostrMinimalContent, and desktop uses different names. The schema needs per-platform renderKey entries instead of a single label. [^6a951-61]
## Registry: C-ABI and JNI Surface

The registry is exposed over C-ABI via nmp_app_gallery_registry_json() returning *const c_char, and over JNI via Java_org_nmp_gallery_bridge_KernelBridge_nativeRegistryJson. This mirrors the existing showcase.rs pattern exactly. [^6a951-6]


Kotlin wrappers must exactly mirror the established showcase pattern with no deviations. The JNI wrapper for registryJson() was initially written with a null-handle guard (if (handle != 0L) ... else "{}") that showcaseReferencesJson() does not have. This inconsistency was caught during Sonnet review and corrected: the final form is fun registryJson(): String = nativeRegistryJson(), matching showcaseReferencesJson() exactly. Any new JNI accessor added to KernelBridge.kt must follow the same pattern — a one-line external fun declaration with no guards or wrappers unless the established precedent already includes them. [^6a951-38]
## Registry: Duplicate Literal Guard Test

A dup-literal guard test (copy of the existing showcase guard) fails CI if any component slug or label from the registry appears in a .swift, .kt, or .rs host file. This prevents platforms from re-drifting by hardcoding component identifiers outside the registry. [^6a951-7]


The original plan called for banning any component slug or label from appearing in .swift, .kt, or .rs host files outside registry.json. Codex identified this as too broad: component slugs and labels must appear in render dispatch code (match component.id, switch component.id, when). The corrected guard only bans REGISTRY_SECTIONS array literals — the catalog data itself — from appearing outside registry.json. Slugs in match arms are fine and necessary. [^6a951-16]

The initial guard test shipped in PR #787 is intentionally weak — it serves as documentation of the intent, not full enforcement. The guard comment states "For now, it serves as documentation of the intent." All four platforms still have hardcoded REGISTRY_SECTIONS arrays that have not been deleted, because the migration to read from the canonical registry at runtime has not been completed yet. Full enforcement activates once platforms are migrated and the hardcoded arrays are removed. [^6a951-31]

Codex identified that the original plan's dup-literal guard was too broad — banning any component slug or label string from appearing in .swift/.kt/.rs files outside registry.json would fire on every legitimate render dispatch match/switch arm. The corrected guard only bans REGISTRY_SECTIONS array literals (the catalog data itself) from appearing outside registry.json. Slugs in match arms, switch statements, and when blocks are fine and necessary — they are render dispatch code, not catalog duplication. The guard test shipped in PR #787 is intentionally weak, serving as documentation of intent until all platforms are migrated to read from the canonical registry at runtime. [^6a951-60]
## Reactivity Contract

Catalog metadata (component IDs, labels, descriptions, section groupings, and variant specs) is pulled once statically at startup via nmp_app_gallery_registry_json(). This is pure config — no data fetching, no pre-warming of kind:0, kind:30023, or any other Nostr events. Every component remains fully responsible for signaling its own data requirements via claim_profile(pubkey), claim_event(uri), etc. when it mounts. The kernel fetches in response to those claims, never proactively. The gallery shell has no right to pre-warm anything. The DataContract field in ComponentSpec is a hint about which showcase reference to pass into the component when rendering it — so the shell knows to wire showcase.profile.pubkey_hex into NostrAvatar — but it does not trigger any fetch. The component's own claim_profile call drives the fetch. [^6a951-8]

## Gallery Profiles Projection

A new gallery_profiles host-side projection is registered via the existing register_snapshot_projection seam. This projector applies the single canonical merge precedence (claimed → author_view → mention_profiles-if-absent) in Rust and emits raw fields: pubkey, npub, display_name, picture_url, nip05, about. All three native merge bodies (Swift, Kotlin, Rust) collapse to decoding one map. [^6a951-9]


The gallery_profiles projection (Step 7 of the consolidation plan) is not yet implemented. Currently the gallery still uses its bespoke nmp_app_gallery_snapshot pull symbol (live at apps/nmp-gallery/nmp-app-gallery/src/lib.rs:164), which is tracked under V-107 for migration to the canonical register_snapshot_projection seam. Moving gallery to the canonical projection registry is a prerequisite for the gallery_profiles projection, since the projection must ride the reactive push frame rather than existing as a standalone pull symbol. [^6a951-53]
## Migration Steps

Step 1: registry.json + registry.rs + C-ABI + JNI + dup-literal guard test (blocker for steps 2-4). Step 2: TUI and Desktop read registry; delete tui/src/gallery.rs catalog; remove desktop-to-TUI coupling. Step 3: iOS reads registry; delete RegistrySection.swift literal array. Step 4: Android reads registry; gains embed and relay sections for free. Step 5: Variant manifest in registry.json; page renderers iterate variants. Step 6: De-poll desktop (iced subscription) and Android (blocking recv). Step 7: gallery_profiles projection; delete triplicated merge in Rust/Swift/Kotlin. Steps 6 and 7 are independent of the registry track and can land in any order. [^6a951-10]


After Codex review, the plan was adjusted in four ways: (1) The dup-literal guard bans REGISTRY_SECTIONS array literals, not component slugs. (2) The schema gains a references table (full npub, full refs), content template inputs, relay row/status inputs, and per-platform renderKey entries. (3) The Android parity track explicitly includes F-CR-02 (Compose kind registry) as a dependency. (4) Steps 6 (de-poll) and 7 (gallery_profiles projection) are reclassified as prerequisites that run in parallel with Step 1, not as optional follow-ons — under repo doctrine, de-polling and the Rust projection are required for platform consistency. [^6a951-19]

Design validation: Before fanning out agents, the plan was submitted to Codex (via codex exec program) for architectural review. Codex identified 3 blockers and 1 high-severity issue: (1) the dup-literal guard was too broad — banning any slug string would fire on legitimate render dispatch match/switch arms; the fix narrows it to ban only REGISTRY_SECTIONS array literals; (2) the schema was insufficient — missing full bech32 npub, ContentTreeWire templates, relay status inputs, and per-platform renderKey entries; (3) Android parity requires more than just adding sections — it needs the Compose EmbedKindProjection/NostrKindRegistry/EmbeddedEvent stack (F-CR-02). Codex also elevated Steps 6+7 from optional follow-ons to prerequisites running in parallel with Step 1. All four adjustments were incorporated before the agent fan-out began. [^6a951-35]

The Opus audit identified three ordered follow-up tasks by leverage: (1) Extract profile-merge into a kernel projection — kernel emits resolved_profiles: {pubkey: ProfileCard} with precedence already applied; Swift/Kotlin/Rust do a dumb decode of one map. (2) Wire platforms to registry.json and delete the 4 hardcoded copies — then re-arm the guard test so copies cannot re-appear. (3) Fix desktop's pre-warming — move claims into each component's render path, matching the TUI sink pattern where components claim their own data when they mount. [^6a951-3]
## Decision: No npub Abbreviation Standardization

Each platform owns npub truncation entirely, including choosing how many characters to show and how to render it. Different displays call for different handling: web might use CSS to truncate, TUI should allow for a smaller amount of npub displayed than Desktop. This stays in shells per aim §2, but each platform should use a common algorithm for computing the abbreviation. [^6a951-11]


The full bech32 npub string must come from Rust (via the registry or snapshot projection), but abbreviation/truncation is entirely platform-owned. Each platform chooses how many characters to show and how to render the truncation — CSS on web, fewer chars on TUI than Desktop, etc. The npub abbreviation algorithm stays in shells per aim §2. [^6a951-42]
## Decision: Variants for All Component Categories in v1

Every section in registry.json gets a full variant manifest in v1. This includes User, Content, Embed, and Relay components — not just User components. [^6a951-12]

## Enforcement: No Duplicate Catalog Definitions

After the migration, no platform may maintain a separate hardcoded catalog of gallery components. The registry.json is the single canonical source. The dup-literal guard test in CI prevents any component slug or label from reappearing in native host files. [^6a951-14]


All four platforms (TUI, Desktop, Android, iOS) render every component listed in the registry. Specifically, TUI, desktop, and Android all implement relay-list rendering — not just listing it as a placeholder. After consolidation, all platforms start from the same solid baseline so new components can be added to all platforms simultaneously. [^6a951-23]



## PR #787: First Integration Batch (11 Tasks Landed on Master)

The first integration batch (PR #787) landed 11 tasks on master, all reviewed by Sonnet agents and merged through the integration branch. Tasks 1-2 established the registry foundation (registry.json + registry.rs + C-ABI + JNI + structural guard, plus JNI nativeRegistryJson with Kotlin wrapper). Tasks 3-4 eliminated polling violations (Desktop via iced Recipe subscription, Android via 30s blocking recv). Task 5 fixed the TUI profile-merge to canonical order (claimed → author_view → mention-if-absent). Tasks 6-7 brought Android to feature parity (relay + embeds sections, NostrRelayList composable, RelayComponentPages). Task 8 added iOS SectionListView SF Symbol names for content/embeds. Task 9 added Android avatar size variants and identicon fallback. Task 10 added TUI relay section with render_relay_list(). Task 11 added Android real embed showcase pages (article/note/highlight/profile). After this batch, the next wave migrates each platform to read from registry.json via the C-ABI/JNI accessor instead of their local hardcoded REGISTRY_SECTIONS arrays — eliminating duplication and making new component additions a one-line JSON edit across all platforms. [^6a951-30]

After PR #787 landed, all four platforms still retain their hardcoded REGISTRY_SECTIONS arrays: TUI at tui/src/gallery.rs:134 (const REGISTRY_SECTIONS), Desktop at desktop/src/gallery.rs:17 (imports from TUI crate), iOS at RegistrySection.swift:25 (let REGISTRY_SECTIONS), and Android at RegistrySection.kt:22 (val REGISTRY_SECTIONS). The registry.json, C-ABI, and JNI infrastructure is in place on master, but no platform reads from it at runtime yet. The core goal — one edit to registry.json propagates to all platforms — is not yet achieved. The next migration wave must replace each platform's hardcoded array with a runtime decode from the canonical JSON. [^6a951-32]

Files created in PR #787: apps/nmp-gallery/registry.json (4 sections, 16 components, schema nmp.gallery.registry/1), apps/nmp-gallery/nmp-app-gallery/src/registry.rs (GalleryRegistry/RegistrySection/ComponentSpec structs, include_str! embed, OnceLock, C-ABI accessor, guard test), apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/NostrRelayList.kt (NostrRelayEditRow data class, NostrRelayList composable with connection status color dots and role badges), apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/gallery/RelayComponentPages.kt (RelayComponentSection routing), apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/gallery/EmbedComponentPages.kt (real embed showcase pages for article/note/highlight/profile using DisposableEffect for claim/release lifecycle). Files modified: lib.rs (added pub mod registry), android.rs (added JNI nativeRegistryJson), KernelBridge.kt (added registryJson), desktop/Cargo.toml (added tokio + iced advanced feature), desktop/src/bridge.rs (mpsc channel replacing Arc<Mutex>), desktop/src/gallery.rs (SnapshotRecipe subscription replacing Poll timer), GalleryModel.kt (250ms→30_000ms timeout), tui/src/data.rs (canonical profile-merge order), RegistrySection.kt (added relay and embeds sections), SectionListView.swift (SF Symbol names for content/embeds), UserComponentPages.kt (avatar size variants + identicon fallback), tui/src/gallery.rs (relay-list in COMPONENTS + RELAY_COMPONENTS), tui/src/render.rs (relay-list case + render_relay_list function). [^6a951-37]

Overview

nmp-gallery Android must achieve feature parity with ALL components loading and rendering correctly — every profile loads, every image loads, everything is properly hooked up to use NMP Android components as expected. The QA phase deploys a Haiku agent with full ADB/emulator access to build both Chirp Android and nmp-gallery Android, launch an emulator, install APKs, screenshot every tab, and check adb logs for errors. Feature parity is not just catalog parity; it means every component renders with real data end-to-end. [^f3d8d-47]
## See Also
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide
- [[reactive-profile-mentions-architecture|Reactive Profile Mentions — LiveProfileMap Architecture]] — related guide
- [[gallery-vs-production-app-distinction|Gallery App Implementations Do Not Satisfy Production Backlog Items]] — related guide
- [[flatbuffers-typed-transport|FlatBuffers Typed Transport — Hybrid Migration Architecture]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[d8-no-polling-ever|D8 — No Polling, Ever]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[opus-architect-workflow|Opus Architect Workflow — Plan, Validate, Execute, Audit]] — related guide
- [[one-way-principle|One-Way Principle — Avoid Multiple Mechanisms for the Same Concern]] — related guide
- [[v-107-bespoke-snapshot-consumer-migration|V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam]] — related guide
- [[cross-platform-qa-code-review-workflow|Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize]] — related guide
- [[architectural-compliance-verification-gate|Architectural Compliance Verification Gate — Verify Before Implementing]] — related guide
