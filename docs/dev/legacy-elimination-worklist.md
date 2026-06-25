# Legacy / Parallel-Implementation Elimination Worklist

**Generated:** 2026-06-17  
**Mandate:** zero legacy / parallel / alternative implementations anywhere.  
**Scope searched:** `crates/`, `apps/`, `ios/`, `web/`, `android/` — Rust, Swift, Kotlin, FlatBuffers schemas.  
**Methodology:** broad `rg` pass for markers (`legacy`, `deprecated`, `fallback`, `old_`, `_v1/_v2`, `back?compat`, `for now`, `temporary`, `superseded`, `to be removed`, `removed? (once|after|when)`) + structural review of enum/trait/function duplication + cross-file diff for vendor copies.

---

## Ranked Worklist

Items are ordered by **(architectural severity × blast radius)**. Items where all callers are already migrated are ranked lower regardless of structural severity.

---

### #1 — `web/registry/src/vendor` — Component registry copies diverged from canonical sources

**Severity:** HIGH — structural duplication of rendering components across two sources with no single-truth synchronisation mechanism.

**What it is:**  
`web/registry/src/vendor/` holds a component registry (28 TUI Rust files + 23+ SwiftUI files) that were originally copied from `crates/nmp-cli/registry/` (TUI) and then evolved independently. The registry ships components to consumer projects via `registry.toml`. Because there is no automated sync, the vendor copy has **diverged** from the canonical implementation.

**Diverged TUI Rust files (web vs CLI):**

| File | Web (vendor) | CLI (canonical) |
|---|---|---|
| `tui/content-kind-registry/nostr_kind_registry.rs` | 446 lines | 826 lines |
| `tui/content-kind-registry/kind_renderer.rs` | 28 lines | 65 lines |
| `tui/content-kind-registry/embedded_event.rs` | 75 lines | 107 lines |
| `tui/content-view/nostr_content_widget.rs` | 225 lines | 230 lines |
| `desktop/content-kind-30023/embed_article.rs` | identical | identical |

The `nostr_kind_registry.rs` gap (446 vs 826 lines) means the registry ships a component that is missing ~380 lines of rendering logic, including author byline, rounded-box card layout, and live-profile resolution that the CLI version gained via `author_byline()` / `NostrMentionProfileHost`.

**Diverged SwiftUI files (web/registry vendor vs apps/chirp/ios):**
23 matching filenames; those confirmed diverged: `HighlightEmbed.swift`, `NostrRelayList.swift`, `NostrContentView.swift`, `NostrUserCard.swift`, `NostrMinimalContentView.swift`, `ArticleEmbed.swift`, `RenderIdentifiable.swift`, `ProfileWire.swift`, `NostrAvatar.swift`, `NostrProfileHost.swift`, `NostrNpubChip.swift`, `ContentTreeWire.swift`, `NostrNip05Badge.swift`, `NostrProfileName.swift`, `EmbeddedEvent.swift`, `EmbedKindProjection.swift`, `NostrKindRegistry.swift`, `EmbedChromeContainer.swift`, `NostrQuoteCard.swift`.

**Canonical paths:**
- TUI Rust: `crates/nmp-cli/registry/tui/…` (always-canonical; CLI builds and tests it)
- SwiftUI: `apps/chirp/ios/Chirp/Components/…` (always-canonical; iOS builds and tests it)

**Legacy path:** `web/registry/src/vendor/tui/…` and `web/registry/src/vendor/swiftui/…`

**Callers / consumers:** The `registry.toml` in `web/registry/src/vendor/` describes the distribution metadata; the web registry shell (`web/registry/src/…`) consumes the TUI vendor copy at build time.

**Blast radius:** Every consumer that pulls components from the web registry receives the stale/incomplete versions. The SwiftUI divergence is a documentation risk (the iOS project is canonical; the registry vendor is stale reference material).

**Converge + delete action:**
1. For TUI: make the web registry reference `crates/nmp-cli/registry/` directly (workspace path dep or symlink) so there is ONE source. Delete `web/registry/src/vendor/tui/` entirely.
2. For SwiftUI: either delete `web/registry/src/vendor/swiftui/` (if the registry is not the distribution vehicle for iOS components) or make the registry vendor directory generated from the iOS canonical files.
3. Update `registry.toml` to point at the remaining single source.

---

### #2 — `nmp-nip60/src/relay.rs` — Parallel blocking-WebSocket relay stack outside the kernel

**Severity:** HIGH — second relay I/O path that bypasses the kernel's store/ingest chokepoint entirely.

**File:** `crates/nmp-nip60/src/relay.rs` (entire module, ~180 lines)

**What it is:**  
A full synchronous (blocking) WebSocket relay stack built on `tungstenite` + `rustls`. Implements:
- `open_socket(url)` — raw TCP/TLS connection
- `fetch_events(relay_url, filter)` — send REQ, collect until EOSE
- `fetch_nip65_relays(pubkey)` — fetch kind:10002 from `purplepag.es`
- `publish_event(relay_url, event)` — send EVENT, wait for OK

Called from `crates/nmp-nip60/src/nip60_wallet.rs` (line 37: `use crate::relay::{fetch_events, fetch_nip65_relays, publish_event}`).

**Canonical path:** The kernel's relay pool (`nmp-network::Pool`, `BrowserRelayDriver`) + store/ingest pipeline (`ActorCommand::PushInterest` / `EnsureInterest` + ingest parsers). All relay I/O for in-scope features goes through this chokepoint.

**Status:** `nmp-nip60` is PARKED (excluded from `[workspace].members`); the code is in-tree awaiting issue #1001 (NIP-60 / nutzaps milestone). The tenex-edge proposal #1492 calls for routing through the kernel chokepoint rather than a bespoke relay stack.

**Blast radius:** Currently zero (parked). Becomes a live violation the moment `nmp-nip60` is un-parked if `relay.rs` is not removed first.

**Converge + delete action:**
1. Before un-parking `nmp-nip60`: delete `crates/nmp-nip60/src/relay.rs` entirely.
2. Replace all `relay::fetch_events(…)` calls in `nip60_wallet.rs` with interest-based queries through the kernel store (or a `store: Arc<dyn EventStore>` injected into `Nip60WalletHandle`).
3. Replace `relay::publish_event(…)` with `ActorCommand::PublishUnsignedEvent` / `ActorCommand::PublishSignedEvent` through the existing `NmpApp` dispatch surface.
4. `relay::fetch_nip65_relays` — pull from the `EventStore` (kind:10002 is already indexed); no direct WebSocket fetch needed.

---

### #3 — Legacy interest registration surface (`PushInterest` / `push`/`push_if_changed` / `push_interest_and_serve`)

**Severity:** HIGH — the naming itself calls this out; a fully-designed replacement (`EnsureInterest` + new front-door) exists in `docs/dev/unified-interest-registration-design.md`. **STATUS: IN-PROGRESS design (codex-validated 2026-06-17, implementation not yet started).**

**Files and lines:**

- `crates/nmp-core/src/subs/registry.rs:135` — `Registry::push()` (uses `legacy_identity`)
- `crates/nmp-core/src/subs/registry.rs:149` — `Registry::push_if_changed()` (uses `legacy_identity`)
- `crates/nmp-core/src/subs/registry.rs:221` — `legacy_key()` — mints a `"legacy-interest-id"` keyed slot
- `crates/nmp-core/src/subs/registry.rs:236` — `legacy_identity()` — single owner `"legacy-single-owner"`
- `crates/nmp-core/src/kernel/cache_serve/mod.rs:197` — `Kernel::push_interest_and_serve()` — comment: "Legacy-surface install recipe"
- `crates/nmp-core/src/actor/dispatch.rs:1322` — `ActorCommand::PushInterest` arm — comment: "legacy push install recipe"
- `crates/nmp-ffi/src/lib.rs:1922` — `NmpApp::push_interest()` — exposed to all NMP protocol crates

**Callers (production, not test):**

| Caller | Count |
|---|---|
| `crates/nmp-marmot/src/ffi.rs:480` | giftwrap inbox interest |
| `crates/nmp-marmot/src/fetch.rs:30` | key-package lookup |
| `crates/nmp-marmot/src/projection/state.rs:476,546` | group/key-package interests |
| `crates/nmp-wot/src/runtime.rs:200` | WoT bootstrap |
| `crates/nmp-nip29/src/action/discover.rs:65` | group discovery |
| `crates/nmp-core/src/browse/mod.rs:165` | open-author/open-thread browse |
| `crates/nmp-ffi/src/lib.rs:1923` | public NmpApp Rust API |

**Canonical path:** `ActorCommand::EnsureInterest { identity, interest }` with full `SubIdentity` (owner + key + scope), implemented in `crates/nmp-defaults/src/topic_articles.rs:200`. The new front-door specified in `docs/dev/unified-interest-registration-design.md` replaces `PushInterest` entirely.

**Blast radius:** 15 production call sites across 6 crates. Removing `PushInterest` / `NmpApp::push_interest` requires migrating every caller to the new front-door API.

**Converge + delete action:** Follow the implementation plan in `docs/dev/unified-interest-registration-design.md`. Binding sequencing from §0 amendments: (a) collapse follow-feed to multi-author interest; (b) implement the new front-door with batch form; (c) migrate all 15 callers; (d) delete `Registry::push()`, `push_if_changed()`, `legacy_key()`, `legacy_identity()`, `legacy_scope()`, `Kernel::push_interest_and_serve()`, `ActorCommand::PushInterest`, and `NmpApp::push_interest`.

---

### #4 — `is_discovery_kind` duplication across `nmp-router` (two definitions in same crate)

**Severity:** MEDIUM — same function body in two files of the same crate; divergence risk on kind-range changes.

**Instances:**

| File | Visibility | Signature | Notes |
|---|---|---|---|
| `crates/nmp-router/src/discovery.rs:16` | `pub(crate)` | `fn is_discovery_kind(kind: u32) -> bool` | **Canonical** — doc: "shared by the router so `router.rs` stays under LOC ceiling" |
| `crates/nmp-router/src/nip65_resolver.rs:93` | `pub` | `fn is_discovery_kind(kind: u32) -> bool` | **DUPLICATE** — same logic, `pub` re-export; does NOT call `discovery::is_discovery_kind` |
| `crates/nmp-core/src/kernel/test_router.rs:46` | private | `fn is_discovery_kind(kind: u32) -> bool` | **Acknowledged test copy** — documented: circular-dep constraint prevents linking `nmp-router` |
| `crates/nmp-core/src/kernel/relay_diagnostics/discovery.rs:43` | private | `fn is_discovery_kind(kind: u64) -> bool` | **Different concern** — display-label classification using `u64`, different constant set (`DISCOVERY_KINDS: &[u64]`); cosmetic but duplicates the range logic |

**The real duplicate:** `nip65_resolver.rs:93` — public function in the same crate as `discovery.rs`. `nmp-router/src/lib.rs:68` re-exports `is_discovery_kind` from `nip65_resolver` (not from `discovery`), so external consumers get the resolver's copy.

**Blast radius:** Within `nmp-router`: changing `discovery.rs` without changing `nip65_resolver.rs` silently diverges the two. External consumers (`nmp-core/src/kernel/mailboxes.rs:235`) get the resolver version.

**Converge + delete action:**
1. In `crates/nmp-router/src/nip65_resolver.rs`: delete the local `pub fn is_discovery_kind` definition; replace with `pub use crate::discovery::is_discovery_kind;` (or import and re-export).
2. The `relay_diagnostics/discovery.rs` copy uses `u64` and a hardcoded slice — acceptable as a display-only helper but should add a comment noting it mirrors `nmp-router::discovery::is_discovery_kind`; if the range ever changes both must update. Consider extracting a shared const from `nmp-kinds`.
3. `test_router.rs` copy is intentional and documented — leave it with its existing comment.

---

### #5 — FlatBuffers deprecated wire slots retained in schema after all consumers migrated

**Severity:** LOW-MEDIUM — no production code reads these slots; they occupy schema space and generate zero-byte stubs forever unless the schema is cleaned up.

**Files and fields:**

| Schema | Line | Field | Deprecated since | Consumer status |
|---|---|---|---|---|
| `crates/nmp-core/schema/publish_outbox.fbs` | 64 | `created_at_display:string (deprecated)` | V-115 / ADR-0032 | iOS: computes locally in `NotificationsView+OutboxRow.swift:89`; typed glue skips at line 124 |
| `crates/nmp-core/schema/publish_outbox.fbs` | 80 | `target_summary:string (deprecated)` | V-115 / ADR-0032 | iOS: computes locally; Rust: projection does not emit |
| `crates/nmp-core/schema/profile_card.fbs` | 49 | `npub:string (deprecated)` | V-115 / ADR-0032 | iOS `TypedProjectionGlue.swift:320`: comment "card.npub returns nil/empty. ProfileCard no longer has npub property" |

**Confirmation that Rust encoder does NOT write these slots:**  
`crates/nmp-core/src/kernel/publish_outbox.rs:55` — comment: "`format_timestamp`… and `publish_outbox_target_summary`… are removed". `PublishOutboxItem` struct has no `created_at_display` or `target_summary` fields.

**Blast radius:** Zero production impact. Removal requires: delete the 3 field declarations from `.fbs`, run `flatc` to regenerate the 3 `_generated.rs` files, bump `SCHEMA_VERSION` constants. No caller code changes needed.

**Converge + delete action:**
1. Delete `created_at_display:string (deprecated)` and `target_summary:string (deprecated)` from `crates/nmp-core/schema/publish_outbox.fbs`.
2. Delete `npub:string (deprecated)` from `crates/nmp-core/schema/profile_card.fbs`.
3. Regenerate: `flatc --rust -o crates/nmp-core/src/kernel/typed_projections/generated crates/nmp-core/schema/publish_outbox.fbs profile_card.fbs` using the workspace-pinned flatc version.
4. Bump `PUBLISH_OUTBOX_SCHEMA_VERSION` and `PROFILE_CARD_SCHEMA_VERSION`.
5. Update any golden-fixture hex files that encode these schemas.

---

## Definitely Not Legacy (Checked)

The following were inspected and confirmed NOT to be legacy/parallel paths:

| What | Why it is canonical / acceptable |
|---|---|
| `apps/chirp/chirp-desktop/src/keyring.rs` `legacy_sessions_dir()` / `migrate_legacy_secret()` | Intentional one-way data-migration path: moves existing plaintext session files to OS keyring. Removing it would strand users who haven't yet migrated. |
| `crates/nmp-core/src/kernel/test_router.rs` `is_discovery_kind()` + `classify_kind()` | Documented in-crate test copy; comment explains the `nmp-router` circular-dep constraint that makes this necessary. Both are private to the test module. |
| `crates/nmp-nip01/src/decode.rs:167` — NIP-10 deprecated positional `e` tag form | Protocol interoperability requirement. Old Nostr clients published notes using the positional form; the decoder must parse them. Cannot remove without breaking thread reconstruction. |
| FlatBuffers `#[deprecated]` in `*_generated.rs` files (e.g. `content_tree_generated.rs:11-21`) | Generated by `flatc` for enum min/max constants. Not hand-edited. The compiler emits these as deprecated stubs; suppressed by the per-module `#[allow(deprecated)]` block. |
| `accounts.fbs` `npub:string` field | NOT deprecated — still active. `TypedProjectionGlue.swift:40` reads `row.npub` for the accounts projection (separate from the `profile_card.fbs` deprecated slot). |
| `nostr_kind_registry.rs` `fallback: KindRendererRef` field | Architectural dispatch pattern — the `fallback` renderer handles unknown/unregistered kinds. Not a legacy alternative; it is the canonical forward-compat path. |
| `crates/nmp-wasm/src/publish_path.rs` stubs (`write_path_not_wired_for_kind_reason`, etc.) | Intentional capability gates for the WASM preview build. The comment explicitly says publishing is disabled pending issue #1008; the stubs surface honest errors instead of silently swallowing them. |
| `inserted` / `updated` JSON-only projection fields in `swift_projections_registry.rs` | Per-tick timeline deltas; comment: "no standalone typed sidecar (JSON only)" — by design, not an oversight. |
| `crates/nmp-core/src/publish/policy.rs` `classify_publish_behavior` | Single-source publish policy table (the unified replacement for scattered `kind == 0` checks). This IS the canonical path. |
| `ActorCommand::EnsureInterest` | The MODERN registration path. Not legacy. |

---

## Cross-Reference: Already-Known Items

Items flagged in the audit request, confirmed present or resolved:

| Item | Status |
|---|---|
| Legacy `push`/`push_if_changed`/`push_interest_and_serve` surface | **PRESENT** — worklist #3 above. Design complete in `docs/dev/unified-interest-registration-design.md`; implementation not started. |
| `nmp-nip60` parallel blocking-WebSocket relay stack | **PRESENT** — worklist #2 above. Crate parked; relay.rs must be deleted before un-parking. |
| `is_discovery_kind` triplication in `nmp-router` | **PRESENT** — worklist #4 above. Real duplicate is `nip65_resolver.rs:93` vs `discovery.rs:16`; test copy acknowledged. |
| Protocol-noun publish handlers in `crates/nmp-core/src/actor/commands/publish.rs` | **NOT A PARALLEL PATH** — `publish_profile`, `react`, `follow` are protocol-specific wrappers that call the generic `publish_unsigned_event` path; they are not alternative implementations of the same concern. Each wraps the shared signing + routing path for a specific kind. No elimination needed. |
| Hand-edited `_generated` FlatBuffers files | **NOT PRESENT** — all `*_generated.rs` files begin with `// automatically generated by the FlatBuffers compiler, do not modify` + `// @generated`. No hand-edits found. |
