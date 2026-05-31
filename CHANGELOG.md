# Changelog

All notable changes to the NMP workspace are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
