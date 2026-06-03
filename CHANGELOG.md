# Changelog

All notable changes to the NMP workspace are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## nmp-v0.2.4 — 2026-06-03

**Non-breaking C-ABI. Existing callers of `nmp_app_signin_nsec`, `nmp_app_signin_bunker`, and `nmp_app_create_new_account` must add the new `make_active` argument (see below).**

### Added

- **`nmp_app_sign_event_for_return(app, account_pubkey_hex, unsigned_json) → correlation_id`**: sign any unsigned event draft with the named account's signer (local nsec or NIP-46 bunker) and park the result in `projections["signed_events"][correlation_id]` on the next snapshot tick. The host never touches raw key bytes (D13). Works for both local keys (resolves synchronously) and NIP-46 bunkers (parks on the non-blocking `PendingSignReturn` idle loop, resolves within 5s or surfaces an error verdict).

### Changed

- **`nmp_app_signin_nsec(app, secret, make_active: u8)`** — added `make_active` parameter. Pass `1` for the normal sign-in path (was the only behaviour before). Pass `0` to register the signer without activating it — for agent/secondary keys that sign via `nmp_app_sign_event_for_return` without disturbing the active account. **All callers must add `, 1`.**

- **`nmp_app_signin_bunker(app, uri, make_active: u8)`** — same `make_active` treatment as `signin_nsec`. **All callers must add `, 1`.**

- **`nmp_app_create_new_account(app, profile_json, relays_json, mls, make_active: u8)`** — added `make_active` parameter. Pass `1` for the standard onboarding flow; `0` to create an agent/secondary account without switching to it. **All callers must add `, 1`.**

---

## nmp-v0.2.3 — 2026-06-03

**Non-breaking C-ABI. FFI callers using `open_author`/`open_thread`/`open_firehose_tag` or `PublishNote` must migrate (see below).**

20 commits since 0.2.2 (tagged 2026-06-03).

### Fixed

- **NIP-42 auth race on app relays** (#931): when a relay required auth and
  closed REQs that were sent before the challenge arrived, subscriptions were
  never re-established after `Authenticated`. `handle_auth_ok` now calls
  `lifecycle.handle_reconnect(relay_url)` on `Authenticated` to replay the
  full current plan to the relay, covering both the pre-challenge race and the
  properly-buffered case. Fixes TENEX Android / `relay.tenex.chat` black-hole.

### Added

- **Generic `nmp_app_open_interest(filter_json, consumer_id, scope)` FFI** (#923,
  M2 phase 2): replaces the three bespoke `open_author` / `open_thread` /
  `open_firehose_tag` symbols with a single JSON-filter–driven interest entry
  point. Apps compose hydration. Old symbols are removed; callers must migrate.

### Removed / Breaking

- **`PublishNote` action removed** (#916–#919): all first-party callers (iOS,
  Android, Rust) migrated to `PublishRaw { kind: 1, ... }`. Third-party callers
  using `PublishNote` must switch to `PublishRaw`.

- **`open_author` / `open_thread` / `open_firehose_tag` removed** (#923): replaced
  by `nmp_app_open_interest`. Callers using the old symbols will not link.

- **`verify_signer` removed** (#907): signer verification now happens at
  `AddSigner` time. Remove any post-sign-in `verify_signer` calls.

### Changed

- **Signer API unified under `AddSigner`** (#907/#908/#912): `nmp_app_signin_nsec`
  and the bunker sign-in path converge on a single `AddSigner` actor command.
  `signer_pubkey` is now the publish-path authority.

- **Timeline/feed projection cleanup** (#922/#924–#929): legacy `TimelineItem`
  cluster deleted, `TimelineEventCard` stripped of render-decision fields.
  Shell read paths for `inserted` / `updated` / `removed` projection keys must
  switch to the `nmp.feed.home` card-list model.

- **chirp-repl app deleted** (#921): internal tooling only; no external impact.

---

## nmp-v0.2.2 — 2026-06-03

**Non-breaking C-ABI. No migration required.**

11 commits since 0.2.1 (tagged 2026-07-07).

### Added

- **Replaceable event freshness / TTL tracking** (F-ttl): a new subsystem in
  `nmp-core` and `nmp-nostr-lmdb` that assigns `freshness: "fresh"` to newly
  ingested replaceable events, computes TTL from `kind → freshness → epoch`, and
  tracks three freshness levels (`fresh` / `stale` / `expired`). Events enter
  the LMDB store through `timeline_insert_events` (single) and
  `timeline_insert_event_batch` (batch) — `ReplaceableTtlActor` manages the
  transition lifecycle asynchronously.

- **iOS Home Feed view wiring** (iOS): `HomeFeedView` is bridged into the
  `RootShell` with a native SwiftUI timeline backed by the kernel snapshot
  and `TimelineItem` rendering identity.

### Changed

- **`timeline_insert_events` → `timeline_insert_event_batch`** (C-ABI): the
  singular `timeline_insert_events` FFI entry point is renamed to
  `timeline_insert_event_batch`. All first-party shells (iOS `KernelBridge.swift`,
  chirp-tui, chirp-desktop) have been updated; third-party callers using the old
  symbol will fail to link on 0.2.2 until updated.

---

## nmp-v0.2.1 — 2026-06-01

**Non-breaking C-ABI. One projection-key rename requires shell updates (see below).**

20 commits since 0.2.0 (tagged 2026-05-31).

### Migration required

- **`"relay_edit_rows"` → `"configured_relays"` projection key** (#900): any shell
  that reads `projections["relay_edit_rows"]` from the kernel snapshot must rename
  the key to `"configured_relays"`. The corresponding Rust type is renamed
  `RelayEditRow` → `AppRelay` and the slot alias `RelayEditRowsSlot` → `AppRelaySlot`.
  All first-party shells (iOS Swift `KernelBridge.swift`, Android Kotlin, web
  TypeScript `snapshot.ts`, chirp-tui `feature_snapshot.rs`, chirp-desktop) have
  been updated in this release. Third-party shells reading the old key will receive
  `null` on 0.2.1 until updated.

### Added

- **App-owned relay configuration** (#900): apps now declare their relay defaults in
  Rust via `NmpAppBuilder::with_relay(url, role)` and `with_relays(vec![...])`.
  Defaults persist across restarts via a JSON sidecar file
  (`{storage_dir}/.nmp-relay-config.json`). Zero hardcoded relay URLs remain in
  nmp-core; all relay configuration is app-supplied. `ActorCommand::Start` carries
  `initial_relays: Vec<(String, String)>` populated by the builder.

- **`TopicArticlesModule`** (#898): `nmp-app-template` ships a canonical example
  of the action → kernel subscription pattern for `kind:30023` topic-filtered
  articles. Documents the Claim/Release lifecycle and explains the `dispatch_capability`
  anti-pattern.

- **Marmot/MLS Android** (#888, V-109): MLS group messaging wired into the Android
  build via the existing C-ABI seam; Android gains a Groups tab backed by the Marmot
  push projection.

- **Marmot canonical push projection** (#881/#884, V-107): `nmp-marmot` now delivers
  snapshot and messages through the standard push-projection seam (ADR-0039).
  `MarmotBridge` reads push projections on iOS; legacy pull symbols are deleted.

- **Web component-owned claim seam** (#885, F-CR-00): `nmp-wasm` and `chirp-web`
  gain the self-claim pattern for profiles and events — components subscribe and
  release their own claims without app-level pre-fetch loops.

- **Framework-level render-identity** (#7b6f06f7, C3): generated render-identity
  eliminates idle re-renders at the framework layer; registry scaffold included.

### Fixed

- **F-CR-00 capstone — proactive kind:0 fetch removed** (#887): the kernel no longer
  fires a kind:0 fetch when any event is ingested. `NostrAvatar` / `NostrProfileName`
  components self-claim; the proactive pre-fetch was the last violation of the
  component-owned claim model.

- **V-110 OpenView/CloseView warnings** (#886): both commands now emit `tracing::warn`
  and a `clientRuntime` diagnostic when the resolver is absent instead of silently
  no-oping.

- **V-76 Worker fallback diagnostic** (#895): chirp-web emits a `clientRuntime`
  diagnostic and `console.warn` on Worker fallback so the degraded code path is
  observable in production.

- **ADR-0041 presentation strings stripped from kernel** (#890): `role_label` and
  `role_tint` fields removed from the relay kernel state; chirp-desktop and the
  nmp-cli scaffold template updated accordingly.

- **V-57 kind-constant consolidation** (#896): remaining kind-constant duplicates
  across workspace crates migrated to `nmp-kinds`; one declaration per kind.

- **Registry keyed Show** (#883): component page no longer retains stale content
  when the route changes; `Show` is now keyed on the route parameter.

---

## nmp-v0.2.0 — 2026-05-31

**Non-breaking: C-ABI is unchanged from 0.1.0. No symbol migration required.**

156 commits since 0.1.0 (tagged 2026-05-29).

### Added

- **`resolved_profiles` projection** (#812): the kernel now pre-merges
  `claimed_profiles` + `author_view.profile` + `mention_profiles` into a single
  `projections["resolved_profiles"]` map delivered on every snapshot tick.
  All three platform shells (iOS #817, Android #815/#818, TUI #816, desktop #809,
  gallery #813) have been migrated to read this key; their hand-rolled merge loops
  are deleted. New apps should read `resolved_profiles` directly — no merge code needed.

- **`claimed_events` and `claimed_profiles` projections** (#795/#803): the kernel
  now surfaces the full set of component-owned events and profiles via
  `projections["claimed_events"]` and `projections["claimed_profiles"]`.
  `TimelineItem` gains an `authorDisplayName` field (#823) populated from the
  `resolved_profiles` merge so the shell never needs a secondary lookup.

- **`bunker_connection_state` projection** (#864, V-14): `projections["bunker_connection_state"]`
  carries the relay-layer health of an active NIP-46 bunker session — `state`
  (`"connected"` / `"reconnecting"` / `"failed"`), `is_connected`, `is_reconnecting`,
  `is_failed`, `reason`. Available in Rust at HEAD; iOS/Android shell decoding is
  forthcoming — the JSON key is present in every snapshot and can be read via the raw
  projection dictionary today.

- **`NMP_MARMOT_MOCK_KEYRING` environment variable** (#872): set to `1` (or `true`,
  `yes`, `on`) to route MLS key storage through an in-memory mock instead of the
  OS keychain. Enables headless CI testing of Marmot (MLS-over-Nostr encrypted
  group) flows.

- **V-51 relay classification** (#876, chirp-tui): zero-count relay classification
  and indexer discovery-kind targeting in the TUI relay tab.

- **V-42 NIP-51 mute list** (#834): `kind:10000` mute-list subscription with
  timeline suppression — muted pubkeys are filtered from feed results.

- **V-52 single-relay browsing** (#836): relay-origin tracking in store + router;
  enables single-relay browsing mode with per-relay cache provenance.

- **V-60 LRU eviction** (#841): `nmp-store` gains LRU eviction in `gc_step` using
  kernel-clock timestamps; prevents unbounded store growth under long sessions.

- **V-94 NmpAppBuilder typestate** (#858): `nmp-app-template` enforces pre-start
  lifecycle ordering at compile time via typestate; misconfigured app assembly is
  now a compile error, not a runtime panic.

- **`nmp-kinds` Layer-0 crate** (#857, V-57): Nostr kind constants extracted to a
  dependency-free crate, eliminating duplicate declarations across workspace crates.

- **Component-owned kind:0 claiming** (#833/#838/#837/#839): gallery and Chirp iOS
  embed renderers now self-claim author `kind:0` events — the component fetches and
  holds the profile, apps no longer pre-fetch for every embed.

- **Android UI screens** (#862/#863/#856/#815/#818): DM screen (NIP-17), wallet
  screen (NWC/NIP-47 with balance), profile screen, relay-management screen,
  sign-in screen (nsec / local account / bunker), and zap button on note cards.

- **chirp-desktop feature additions**: DM infrastructure, NIP-57 zap support,
  NIP-46 bunker login UI, outbox tab, OS keyring capability, diagnostics tab.

- **Typed Rust client API** (#68-typed-api series): `nmp-app-chirp` now exports
  pure action JSON builder functions (`typed_api`) used by both the desktop bridge
  and by `nmp-testing` parity tests; eliminates duplicated JSON construction.

- **Registry system** (#787/#819/#863): `nmp-gallery` ships a `registry.json` +
  C-ABI accessor (`registryJson()`) cataloguing every supported content kind with
  cross-platform rendering samples.

- **Performance: O(1) snapshot hot path** (#873): `estimated_store_bytes` changed
  from O(store) to O(1); eliminates the twice-per-emit linear scan that was
  serializing inside the snapshot path.

### Fixed

- **Marmot group invites for uncached peers** (#874): key-package fetch was wired
  to a dead `OpenView` stub; now routes through `push_interest`. Inviting a peer
  whose key package is not in local cache now works end-to-end.

- **Actor-thread freeze — bunker DM sends** (#861, V-90 Site 1): `nmp-nip17`
  gift-wrap `op.wait()` was called on the actor thread, blocking all kernel
  processing during NIP-46 remote-signer round trips. Moved to a capability worker
  off-actor (ADR-0040).

- **Actor-thread freeze — Keychain dispatch** (#870, V-90 Site 2): a second
  synchronous capability call (OS keychain) on the actor thread similarly blocked
  the kernel; also moved off-actor via the capability-worker seam.

- **D1 startup ordering** (#835, V-87): first kernel snapshot no longer depends on
  relay I/O; apps receive an initial snapshot immediately on launch even when offline.

- **`mention_profiles` snapshot now correct under claim races** (#843, V-87):
  claimed `kind:1` events are surfaced in `claimed_events` so the gallery embed
  claim-teardown race no longer produces stale profile resolutions.

- **Claim send-gate** (#852): the relay-hint dialing path now uses
  `any-relay-connected` gate instead of the old primary-relay-only gate; events
  with a `wss://` hint in their `e`/`a` tag are dialed before the claim resolves.

- **NWC heartbeat + reconnect** (#783, V-79): `nmp-nip47` now attempts reconnect on
  connection drop and emits a `connection_state` projection; previously silent.

- **NoConfiguredRelays diagnostic** (#782, V-66): kernel emits an explicit
  diagnostic instead of silently substituting a fallback relay when no relays are
  configured.

- **NOSTRCONNECT default relay** (#780, V-65): moved from a hardcoded substrate
  constant to a host bootstrap capability; apps control the NIP-46 relay default.

- **NIP-57 zap amount picker** (#792, V-106): removes the hardcoded 21 000-msat
  default; the amount picker is now required before a zap is sent.

- **Android sign-in routing** (984599bb): `signInNsec`, `switchAccount`,
  `removeAccount` now route through direct C-ABI symbols instead of the broken
  dispatch path that caused silent failures.

- **V-68 {1,6} kind migration** (#840/#877/#878): `kind:1` / `kind:6` (notes and
  reposts) social-kind filters moved from the substrate (`nmp-core`) into the FFI
  shim, completing the D0 Stage 1–3 substrate-purity migration. D17 doctrine-lint
  rule added to enforce the ban.

- **NIP-47 encode failures surfaced** (#774, V-63/V-64): NWC encode errors and
  orphaned pending payment entries are now reported instead of silently dropped.

- **`hex_to_bytes32` returns `Option`** (#775, V-70): prevents silent all-zeros
  fallback when a hex string is malformed.

- **Rate-limited CLOSED backoff** (#778, V-58): relay reconnect now backs off with
  a longer delay when the CLOSED reason indicates rate limiting.

- **V-75 per-lane route attribution** (#777): `RouteAttempt` events include the
  lane number and the empty-set Lane 7 fallback case; enables accurate routing
  diagnostics.

- **Chirp-tui / chirp-desktop file-based session storage** (#797/#796): OS keychain
  removed from TUI/desktop sessions; file-based storage enables headless CI and
  CI-friendly local testing.

- **V-56 feed content-extracted mentions** (#788): NMP feed now feeds
  content-extracted profile pubkeys into the discovery engine so mentioned profiles
  resolve without a separate subscription.

- **NIP-47 sentinel double-stamping** (#829, V-89): DM and zap builders no longer
  double-stamp the sentinel field.

- **Kernel clock threaded into EventStore** (#828, V-59): `SystemTime::now()` calls
  removed from store internals; store is now fully deterministic under test and
  consistent with kernel-clock time.

- **FlatBuffers version-pin check extended to Android `nmp/*` tree** (#781, V-86):
  CI now validates the `FLATBUFFERS_25_2_10()` guard across both the gallery and
  the main Android app trees. (The FlatBuffers version pin itself is unchanged at
  `25.12.19` / `25.2.10` — see Upgrade Guide.)

### Changed

- **`V-68` {1,6} kinds moved to FFI shim**: the social note kinds are no longer
  registered inside `nmp-core`; they are now injected by the FFI layer. C-ABI is
  unchanged — this is a layering refactor with no effect on callers.

- **`nmp-desktop` dead crate removed** (#776): `chirp-desktop` (egui) is the
  desktop app; the dead `nmp-desktop` husk is deleted.

- **Orphan ingest files deleted** (#825, V-68): `ingest/event.rs` and
  `ingest/eose.rs` in `nmp-core` were uncompiled after V-68 Stage 1; removed.
  This is not a regression — the files had zero callers.

- **ONNX model cache removed from git** (3db5946b): `android/.fastembed_cache`
  (90 MB ONNX model) is now gitignored and excluded from the repository.

- **`V-57` kind constants in `nmp-kinds`**: kind constants previously scattered
  across crates centralised in `nmp-kinds`; all callers updated. No public API
  change.

- **`nmp-app-chirp` now exports shared snapshot types** (#52): `RelayRow`,
  `RelayWireSubRow`, `InterestRow`, `ActionResult`, `ActionStageRow`,
  `RuntimeMetrics` re-exported so `chirp-tui` and the desktop share a single
  definition.

### Deprecated

- **`nmp_marmot_snapshot` / `nmp_marmot_group_messages`** (pull-model Marmot
  symbols): these C-ABI pull symbols remain functional. Per ADR-0039 the Marmot
  projection layer is being migrated to the push-projection seam (same as every
  other kernel projection). New apps building on Marmot (MLS) group support should
  prefer the push path; the pull symbols will be removed in a future minor release.

---

## nmp-v0.1.0 — 2026-05-29

First coordinated release-train baseline. See
[`docs/wiki/release-process.md`](docs/wiki/release-process.md#nmp-v010--first-release-2026-05-29)
for the full list of what was included.

Key items: OP-centric feed (V-80), D5 snapshot bounding (V-46), silent-failure
hardening (V-61/62/63/64/67/69/70/72), D0 substrate purity Stage 1 (V-68),
V-75 router lane attribution, V-58 rate-limited backoff.

---

## Upgrade Guide — nmp-v0.1.0 to nmp-v0.2.0

### 1. Re-pin to 0.2.0

```
nmp init --nmp-version 0.2.0
```

Or update the version pin in your app's manifest file.

### 2. No C-ABI migration required

The C header (`NmpCore.h`) is byte-for-byte identical to 0.1.0.
Every existing `nmp_*` FFI call continues to work without change.

### 3. FlatBuffers — nothing to do

The FlatBuffers pin is unchanged:

| Layer     | Version   |
|-----------|-----------|
| Rust      | 25.12.19  |
| iOS (SPM) | 25.12.19  |
| Android   | 25.2.10   |
| Web       | 25.9.23   |

No `.fbs` schema changes were made in this release. If you generated
bindings against 0.1.0 they remain valid against 0.2.0.

### 4. Adopt `resolved_profiles` (optional, recommended)

The kernel now delivers a pre-merged profile map. Instead of merging
`claimed_profiles`, `author_view.profile`, and `mention_profiles` yourself,
read a single key:

**iOS (Swift)**

```swift
// In your NmpUpdate / snapshot apply handler:
let profiles = snapshot.projections?.resolvedProfiles ?? [:]
// profiles: [String: ProfileCard] keyed by hex pubkey
```

**Android (Kotlin)**

```kotlin
val profiles: Map<String, ProfileCard> = snapshot.resolvedProfiles
// Replaces your manual merge of claimedProfiles + mentionProfiles
```

**TUI / any JSON consumer**

```
projections["resolved_profiles"]  →  { "<hex-pubkey>": { "display": "...", "picture_url": "..." }, ... }
```

Your old merge code still works — `mention_profiles` and `claimed_profiles`
continue to be emitted on every snapshot. Migration is purely optional but
eliminates boilerplate and ensures you get the same merge precedence as the
built-in shells.

### 5. Read `bunker_connection_state` for NIP-46 session health (optional)

`projections["bunker_connection_state"]` is now emitted on every snapshot:

```json
{
  "state": "connected",
  "is_connected": true,
  "is_reconnecting": false,
  "is_failed": false,
  "reason": null
}
```

Read it from the raw projections dictionary in your shell's apply handler
to show a reconnecting indicator or prompt re-auth on relay flap.
Generated Swift/Kotlin decodables for this key are planned for the next
minor release.

### 6. What you get for free (no action required)

- Marmot group invites now work for peers whose key package is not
  locally cached (previously silently failed).
- The actor thread no longer freezes during NIP-46 bunker DM sends or
  OS Keychain dispatches (V-90 Sites 1 and 2).
- The kernel delivers an initial snapshot offline at launch without
  waiting for a relay connection (V-87 D1 startup fix).
- NWC connections now reconnect automatically and expose their state
  via the `connection_state` projection.
- The snapshot hot path is O(1) instead of O(store-size) for
  `estimated_store_bytes`.
