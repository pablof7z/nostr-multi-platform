# Changelog

## nmp-v0.8.3 — 2026-06-27

**BREAKING correction — event-ref render envelopes now use their final
projection key.** Git-rev-pinning consumers should move to this baseline rather
than depending on the retired compatibility name. This patch release also
includes the Marmot write-builder baseline from the post-`v0.8.2` merge window.

### Changed

- The derived render envelope sidecar is now emitted and decoded as
  `refs.event.envelopes` / `RefEventEnvelopes` (schema version 2). `refs.event`
  remains the authoritative raw row-delta source; shells and gallery JSON must
  consume the pre-rendered envelope map from `refs.event.envelopes`, not from
  raw `refs.event` and not from the retired `claimed_event_embeds` key.
- Swift, Kotlin, TypeScript, browser-runtime, Chirp, desktop, Gallery, docs, and
  drift gates all use the final projection name. `claimed_event_embeds` remains
  only as retired ADR terminology.
- Marmot writes now use bytes-only generated builders across Rust, iOS, and
  Android, keeping downstream app write paths on the typed envelope contract
  instead of JSON-shaped shell glue.

## nmp-v0.8.2 — 2026-06-27

### Changed

- NIP-29 group events now exposed via `open_group_events(group_id, kinds)`,
  returning the canonical `GroupEventsProjection` instance that feeds the
  `"nmp.nip29.group_events"` typed sidecar. Rust app shells can read the
  selected group event stream without opening a duplicate per-shell observer.

## nmp-v0.8.1 — 2026-06-27

### Changed

- NIP-29 group discovery and joined-groups doors now expose Rust-side
  reader-returning open methods (`open_group_discovery_with_reader`,
  `open_joined_groups_with_reader`). App crates can compose over the canonical
  discovered/joined projections without opening duplicate observed projections
  or becoming second producers of `nmp.nip29.*` typed sidecar keys.

## nmp-v0.8.0 — 2026-06-27

**BREAKING release — profile-resolution overhaul.** Git-rev-pinning consumers
must adapt the one C-ABI signature change below before bumping their pinned rev.

### Breaking — C-ABI

- **`nmp_app_claim_profile` gains a 5th `liveness` parameter (`c_int`), going
  from 4 to 5 arguments.** It is the client freshness hint mapped onto the
  registered kind:0 interest lifecycle: `0` = **CacheOk** (serve from cache; on a
  miss a OneShot fetch; no live sub — use for feed-row avatars), non-zero =
  **Live** (a Tailing kind:0 sub stays open while claimed, so profile edits arrive
  reactively — use for an open profile screen). Mixed claims on one pubkey resolve
  to Tailing (Live wins). Existing 4-arg call sites must pass the new argument;
  pass `0` for background/list-row claims and non-zero for explicit profile views.

### Changed

- **Profile resolution moved onto the registry/recompile chokepoint.**
  `claim_profile` and F-TTL re-verification now flow through the same
  registration → recompile chain as every other interest, so third-party authors
  (mentions, attributions, standalone names) get their kind:0 resolved via
  **outbox (kind:10002) relay discovery** instead of being silently dropped, with
  **retry-on-miss** when the first fetch returns nothing. Drives the unresolved-
  pubkey rate down substantially for mention/attribution-heavy feeds.
- Web-feed snapshot-loop fix — the web client no longer re-runs the feed
  snapshot loop spuriously.
- **Event embeds now resolve through `refs.event` as the single source of
  truth.** The old `claimed_events` whole-map projection is no longer emitted,
  gallery JSON exposes `refs.profile` plus the derived `refs.event.envelopes`
  render map, and web gallery decodes the same `NEMB` envelope sidecar from
  Rust-resolved `refs.event` rows. Registry shell adapters are renamed around
  `resolveEventRef` / `resolveProfileRef`, so UI components render typed
  projections instead of hand-parsing Nostr event JSON in each shell.

### Release plumbing

- `nmp-blossom` (un-parked in #1428 as a v1 workspace member) is now classified
  as a public crate in `release/nmp-release.toml`, clearing the pre-existing
  release-manifest CI red ("workspace packages missing from release manifest:
  nmp-blossom"). `nmp-nip60` remains parked and excluded; the `nmp-wallet-poc`
  app was deleted (it was a standalone direct-relay PoC consuming nip60's now-removed
  private WebSocket stack — to be rebuilt kernel-integrated at #1001, tracked in #1508).

## nmp-v0.7.1

- fix: parked crates (`nmp-blossom`, `nmp-nip60`) are now standalone-buildable — de-inherited the workspace `version`/`edition`/`license`/`repository` fields (and blossom's `nostr` dep) that are unresolvable for `[workspace].exclude`d crates. Restores the documented `cargo build --manifest-path crates/<crate>/Cargo.toml` escape hatch and unblocks external consumers (hl path-dep, podcast-player `[patch]`) that depend on `nmp-blossom`. No code change.


All notable changes to the NMP workspace are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## nmp-v0.7.0 — 2026-06-14

**BREAKING release — the keystone series (ADR-0050 / ADR-0052 / ADR-0056).**
Git-rev-pinning consumers must adapt to the API changes below before bumping
their pinned rev. Three keystones land together:

- **K1 — signer-session capability port** (ADR-0050)
- **K2 — instance-scoped registration** (ADR-0052)
- **K3 — coverage ledger** (ADR-0056)

### Breaking — APIs consumers must adapt to

- **(a) `ActionModule` is register-by-value.** The trait's methods now take
  `&self` (not `&mut self` / associated statics), and modules are registered with
  `register_action(&mut self, module)` — one owned instance per app. The process-
  global registration path (`ACTIVE_WALLET_RUNTIME` and the static
  register-executor seam) is deleted. Consumers that registered action modules
  via the old global/`&mut`-method surface must construct the module by value and
  call `register_action`.
- **(b) The signer-session port replaces `SignerForSeal` + the per-crate raw-keys
  slots.** The `SignerForSeal` execution model and the raw-`Keys` slots that NIP-59
  / NIP-17 / Marmot held are gone; signing, gift-wrap/unwrap, and decrypt now flow
  through the single three-verb signer-session capability port (uniform across
  local-key and bunker/NIP-46 backends — the backend is invisible to callers).
  Consumers that reached for a raw `Keys` to seal/sign must drive the port.
- **(c) `ProtocolCommandContext::kernel_mut()` is removed.** Ambient mutable kernel
  access from protocol command workers is replaced by narrow, explicit
  capabilities. Protocol/extension code must request the specific capability it
  needs instead of taking `&mut Kernel`.
- **(d) `DispatchHostOp` is merged into the `Protocol` seam.** The separate
  `DispatchHostOp` trait/dispatch is gone; host ops dispatch through the unified
  `Protocol` seam (whole-body `catch_unwind` on `ProtocolCommand`). Consumers that
  implemented `DispatchHostOp` move that logic onto the `Protocol` seam.
- **(e) The five process-global hooks/runtimes are now per-app ports.** The
  ambient process-global extension singletons (wallet runtime, coverage hook, and
  the other global hook/runtime slots) are replaced by per-app instance ports
  wired at app construction. Consumers must wire these per `NmpApp` instance
  rather than installing a global.
- **(f) The coverage-ledger floor is now ACTIVE (behavioral).** The since-floor is
  sourced from a per-`(filter_hash, relay)` coverage ledger, replacing the
  store-presence heuristic. Un-synced shapes are asked for their FULL history (an
  un-floored REQ / full-window negentropy reconciliation) until the ledger records
  completed coverage — so expect **more initial relay traffic** on cold/un-synced
  shapes, in exchange for sound backfill (the H1 "follow-after-stray-reply"
  suppression is fixed). No consumer API change; behavior only.

### Changed

- Coverage-ledger activation rode a single release cut (this release) so
  git-rev-pinning consumers can pin across the change in one step (#1419 default-on
  flip, #1421 presence-heuristic deletion).

### Removed

- The `coverage_ledger_enabled` flag and its plumbing; the presence-floor
  heuristic (`shape_floor`, `watermark_from_queries`) and the Stage-B3
  truncated-serve tracking — all superseded by the coverage ledger (ADR-0056
  Stage E).

## nmp-v0.6.2 — 2026-06-13

**No `nmp_app_*` C-ABI symbol change.** Additive release — existing FFI callers
do not need call-site changes.

### Added

- **NIP-55 (Amber) Android signing complete — ADR-0048 Stage 4 E2E passed**
  (#1183, closes #1124). The registry `login-block` component is now
  `compose: stable`: signer detection (Amber + Primal), "Sign in with Amber"
  driving the full kernel flow (pubkey-only account, 90s interactive deadline,
  `signer_state` projection), DM send via the ADR-0026 seal seam, and an
  emulator-verified publish — kind:1 `11652d49…20a76a` signed by the
  Amber-held key, relay-fetched and schnorr-verified. Amber wire fixes shipped
  in the vendored Kotlin bridge (payload in the `nostrsigner:` data URI,
  type/params as Intent extras, `{"type","kind"}` permissions JSON, full-event
  reply via the `event` extra, `rejected` handling). E2E runbook:
  `docs/testing/adr-0048-nip55-amber-e2e-runbook.md`. Android consumers:
  vendor the three login-block Kotlin files (`ExternalSignerWire.kt`,
  `AmberIntentCodec.kt`, `ExternalSignerCapabilityBridge.kt`) + the
  `<queries>` manifest entries, register the bridge, and call
  `nmp_app_signin_nip55` / `nmp_app_deliver_external_signer_response`
  (`external-signer` feature on `nmp-ffi`).
- **wasm NIP-10 reply write path** (PR-5, #1214).

### Changed

- **D5/D8 runtime enforcement** (#1203): `emit_hz` is clamped to
  `EMIT_HZ_MAX = 60` with a D6 log line on clamp (graceful, not a panic);
  snapshot-bound enforcement per D5. Apps requesting >60Hz now get 60.

## nmp-v0.6.1 — 2026-06-12

**Additive C-ABI change**: one NEW symbol, `nmp_app_probe_relay_info` (no
existing symbol changed or removed). Rust API is additive.

### Added

- **ADR-0051 — first-class NIP-11 relay information** (#1195). New `nmp-nip11`
  protocol crate owns the full NIP-11 lifecycle: when a relay connects, a
  `RelayConnectedHook` (new substrate seam, fanned on `PoolEvent::Opened`)
  fetches the relay's information document on an off-actor worker (`ureq`,
  10s budget, 64 KiB body cap, per-URL 5-minute TTL) and posts it back via
  `ActorCommand::SetRelayInfo`. The parsed document (name, description, icon,
  pubkey, contact, software, version, supported_nips, payment/auth/
  restricted-writes limitation flags) surfaces as the `info` child on every
  `relay_diagnostics` row — serde JSON and the `KRDG` typed FlatBuffers
  sidecar both carry it, so consumer apps render relay names and icons with
  zero HTTP, JSON, or NIP-11 awareness of their own. On-demand
  `nmp_nip11::probe_relay_info` (Rust) and `nmp_app_probe_relay_info` (C-ABI,
  callback-borrowed string) cover add-relay preview flows for relays not yet
  in the pool. `nmp-core` names no NIP-11 noun and imports no HTTP crate
  (D0); `nmp-wasm` stays `ureq`-free.

### Fixed

- **CI hygiene** (#1213): `.file-size-baseline` refreshed for seven
  pre-existing over-cap files grown by recent merges (the #1192/#1196
  ratchet-drift pattern).

## nmp-v0.6.0 — 2026-06-12

**BREAKING (C-ABI)**: `nmp_app_free_string` and `nmp_broker_free_string` are
RETIRED, replaced by a single `nmp_free_string` (V-114 #1044, PR #1135) —
every consumer that frees NMP-returned strings must rename the call (one-line
mechanical change; same semantics). One NEW symbol: `nmp_app_composition_report`.
Rust API is additive; explicit composition behavior is unchanged.

### Added

- **ADR-0049 — defaults yield; composition is observable** (#1185).
  `ActionRegistry::register_default::<M>()` is an entry-or-insert *yielding*
  registration: the canonical defaults (nip02/nip17/nip57/nip65) now back off
  when the app already claimed the namespace, **regardless of call order**
  (Spring `@ConditionalOnMissingBean` posture). App-path `register_action`
  keeps insert semantics but fails loudly (`debug_assert!`) on app-over-app
  collisions in dev/test builds; soft + recorded in release (D6). Every
  AppHost registration (actions, ingest parsers, snapshot projections, the
  last-writer-wins slots, dropped late wiring) is recorded in a composition
  ledger queryable via the new `nmp_app_composition_report` FFI symbol —
  the previously documented-but-absent `LateWiring` diagnostic now exists.
- **Explicit substrate and social composition split + typed config** (#1180).
  `register_substrate(app, gate)` is the always-on correctness floor (routing
  substrate, kind:10002 parser, publish resolver, forward policy, coverage
  hook + NIP-77 negentropy runtime); explicit social feature installers layer
  toggleable social capabilities (`social`/`dms`/`zaps`/`longform`) on top and
  make the previously hardcoded `CoverageGate` and nostrconnect bootstrap relay
  overridable config fields — fixing the gate-desync where overriding the
  coverage hook post-hoc desynced it from the negentropy runtime.
  Non-social consumers can now compose a routable app without the social bundle.
- **NIP-51 mute list in the composition root** (#1181): `MuteListProjection`
  observer + `nmp.nip51.mute_list` diagnostic projection wired into the
  `social` tier; `register_mute_runtime` exported for apps needing the `Arc`
  to wire timeline suppression.
- **ADR-0048 NIP-55 external signer** Stages 2–3 (#1153, #1165, #1166): Android
  login-block + Chirp sign-in, proof lanes, DM send via the seal seam.
- **Android**: Keystore keyring capability + synchronous capability routing +
  identity restore (#1188); Marmot leave/invite/remove/clear_pending parity
  with typed action envelopes (#1186).
- **nmp-wasm**: browser runtime executes without panicking (PR-W1 #1150), real
  wasm build deployed for chirp-web (PR-W2 #1176), interest/contact-feed
  dispatch verbs + viewer-pubkey hand-off (PR-3 #1177).

### Fixed

- Publish to AUTH-required relays parks until `Authenticated` instead of
  failing (#1192).
- Bunker accounts activate WOT/DM-relay/zap-receipt runtimes (pubkey-only
  identity accessor, #1191).
- `DmInboxProjection` cleared on account switch (#1184).
- Pre-verified inject path feeds the `IngestParser` dispatcher — closes the
  #1137 regression (#1160).
- NIP-57: bolt11 amount validated before auto-pay (#1189).
- CI: file-size gate was a silent no-op on PRs (#1178).

### Removed

- **C-ABI**: `nmp_app_free_string` + `nmp_broker_free_string` retired into one
  `nmp_free_string` (V-114 #1044, PR #1135). Migration: rename the call.

---

## nmp-v0.5.0 — 2026-06-12

**BREAKING (C-ABI + Rust API).** Four breaking changes since v0.4.0; all require
migration before pinning this revision (see **Removed** below).

**ADR-0045 COMPLETE — v1 exit criterion satisfied.** Offline / second-launch
rendering now works for every open interest: contact feed, DM inbox, threads,
and long-form articles all render from the local LMDB store before any relay
delivers a single event.

### Added

- **ADR-0045 universal cache-serve — E1+E2+E3, closes #1086** (#1107, #1117). The
  `kernel/cache_serve.rs` seam maps every `InterestShape` to LMDB `StoreQuery`
  variants, scans newest-first up to `CACHE_SERVE_BUDGET_EVENTS = 200`, and
  feeds matched events into the post-store projection-dispatch path (bypassing
  `store.insert`, which would return `Duplicate` and skip all observer fan-out):
  - **E1** (`#1107`): `AuthorKind` / `KindTime` shapes — contact-feed events
    served at `open_interest`/follow-feed-sync time with aggregate per-tick budget
    and one-shot completion gate.
  - **E2** (`#1117`): `Ptag` + kind:1059 → `StoreQuery::Ptag` for DM inbox
    gift-wraps; events fed through `notify_raw_event_observers` so
    `DmInboxProjection` receives the full sig-bearing JSON — one decrypt path
    shared with live relay delivery (ADR-0045 R2.4(f)).
  - **E3** (`#1117`): `Etag` (thread replies), `KindDtag` (addressable /
    long-form), and `Ptag`-mentions. Watermark floors restored for all covered
    shapes; the `e3_structural_floored_implies_served` seam-identity test
    enforces "no floor without serve coverage."
  Universal acceptance test `cache_serve_universal_tests`: populates the store
  with a feed event, DM gift-wrap, thread reply, and long-form article; creates a
  fresh kernel with zero relay connections; asserts all four project into the
  snapshot without any relay delivery.

### Removed (BREAKING)

- **`nmp_app_open_timeline` deleted** — replaced by `nmp_app_open_contact_feed` /
  `nmp_app_close_contact_feed` (ADR-0042 §2 amendment, closes #911; #1108). The
  `{1,6}` social-kind literal that was hardcoded in the generic FFI layer (a D0
  violation) now lives in exactly one place: `HOME_FEED_KINDS_JSON` in
  `nmp-app-chirp`. Two new Chirp wrappers —
  `nmp_app_chirp_open_home_feed` / `nmp_app_chirp_close_home_feed` — wrap
  `nmp_app_open_contact_feed` with that constant. `ActorCommand::OpenContactListSubscription`
  is renamed to `OpenContactFeed { kinds }`.

  **Migration**: replace `nmp_app_open_timeline(app)` with
  `nmp_app_chirp_open_home_feed(app)` for the Chirp home feed, or call
  `nmp_app_open_contact_feed(app, kinds_json)` directly with an explicit kinds
  array. Add a matching `nmp_app_close_contact_feed` (the prior
  `nmp_app_open_timeline` had no symmetric close).

- **`nmp-codegen gen modules` scaffolder and `apps/fixture` deleted** (ADR-0046
  — "composition is a library, not a generator"; #1114). The Rust-shell FFI-crate
  generator (`generate.rs` / `ffi_gen.rs` / `workspace.rs`, the `gen modules`
  subcommand, `gen-modules` / `gen-modules-check` justfile targets) and its sole
  consumer `apps/fixture` (`nmp-app-fixture` + `fixture-todo-core`) are removed.
  A generated `FfiApp` never installed the required composition owners and was a
  non-functional Nostr app; the fixture existed only to give the orphan
  generator a test target.
  The `nmp.toml` manifest parser and the `gen swift` / `gen typed-decoders`
  emitters (live CI gates) are unaffected.

  **Migration for old `nmp-app-template` consumers**: that historical migration
  path is superseded. Current apps should depend on explicit owner crates and
  compose protocol modules directly; the `gen modules` scaffolder was the only
  thing deleted in v0.5.0.

  **What breaks for whom**: any CI or script that invokes `nmp gen modules` will
  fail with "unknown subcommand". No real app consumed the generated output;
  verify with `grep -r 'gen modules' .` in your app.

- **`nmp_app_free_string` and `nmp_broker_free_string` deleted** — collapsed into
  a single canonical `nmp_free_string(char *ptr)` (V-114, closes #1044). Frees any
  `*mut c_char` returned by any NMP FFI function; null-safe no-op (D6).

  **Migration**: replace every `nmp_app_free_string(ptr)` /
  `nmp_broker_free_string(ptr)` call with `nmp_free_string(ptr)`. No semantic
  change — identical CString::from_raw free path; the symbol name is the only
  break.

- **`swap_dm_inbox_observer` removed** from `AppHost` and `NmpApp` (with
  `DmInboxObserverIdSlot` + `new_dm_inbox_observer_id_slot`) — dead since the
  DM inbox moved to the slot-keyed `IngestParser`. **Migration**: use
  `replace_ingest_parser(1059, "nip17.dm_inbox", …)` /
  `unregister_ingest_parser`. C-ABI unchanged.

### Fixed

- **V-115 — display formatting removed from kernel projections** (ADR-0032; #1109,
  closes #1045). Three violations fixed; shells must adopt corresponding changes:
  - `ProfileCard.npub` (bech32 string) removed from the FlatBuffers struct and
    schema; slot marked `(deprecated)` for wire compat. Shells encode bech32 via
    `nmp_app_encode_profile` / the new `KernelHandle.encodeProfile(pubkey:)` Swift
    wrapper.
  - `claimed_profiles` builder no longer bakes a bech32 `to_npub(pubkey)` into the
    wire frame.
  - `PublishOutboxItem` loses `created_at_display: String` and
    `target_summary: String`; both FlatBuffers slots marked `(deprecated)`. A new
    `created_at: uint64` slot (raw Unix seconds) is appended. Shells format
    timestamps and compose the relay-count label themselves.

  Doctrine-lint D19 rule bans `crate::display::` and `format_timestamp` in the
  three kernel files; wired into the doctrine-lint smoke gate.

- **V-118 — expiration index replaces gc Phase-1 cursor** (#1106, closes #1097).
  Phase-1 of `gc_step` previously used a resumable `created_at` cursor that
  permanently stalled when a block of non-expired events shared one `created_at`
  value larger than a single budget pass. Replaced with a dedicated
  `nmp-expiry-index` LMDB sub-db keyed `expiry_ts(8 BE) || event_id(32)`. Phase 1
  is now an O(expired) range scan for `expiry_ts <= now_secs`; non-expired events
  are invisible to it and can never stall progress. Index is maintained on insert,
  supersession, kind:5 deletes, and all bulk-delete paths; backfilled once on
  store open for pre-V-118 databases.

- **Blocking `sign_active` primitive removed** (#1104, closes #972). The
  `sign_active` function blocked the actor thread for up to 5 s via
  `SignerOp::wait` on every NIP-46 remote-signer call — a D8 violation masked by
  `debug_assert!` guards. All three production call sites (`create_account` /
  `publish_initial_follows`) converted to `sign_active_nonblocking(..).poll()`;
  error arms surface a D6 toast via `set_last_error_toast`.

- **V-73 — `nmp_app_chirp_register` validates `viewer_pubkey`** (#1105, closes
  #1011). A non-null malformed `viewer_pubkey` was previously silently replaced
  with an empty-string pubkey, causing all subsystems to run against the zero
  "anonymous" identity. `nmp_app_chirp_register` now returns an
  `NmpRegisterStatus` (`#[repr(u32)]` enum: `Ok` / `NullApp` /
  `InvalidViewerPubkey`); a non-null invalid pubkey returns `InvalidViewerPubkey`
  and does not start the kernel.

- **Golden-fixture drift assertion + Kotlin flatc CI gate** (#1103, closes
  part of #1093). `tier3_golden_fixture_matches_encoder` asserts the Rust
  encoder output matches `update_frame_tier3_golden_v1.fb.hex` byte-for-byte.
  `ci/check-kotlin-flatc-drift.sh` mirrors the Rust flatc-drift gate for the
  Kotlin transport bindings (`apps/chirp/android/app/src/main/java/nmp/transport/*.kt`),
  requiring flatc v25.2.10 (the Android/Kotlin runtime pin); wired into
  `codegen-drift.yml` as a new `kotlin-flatc-drift` job.

### Changed

- **ADR-0045 Rev 2 — single-mechanism cache-serve** (`#1102` — docs-only ADR
  amendment). Supersedes the §9 staged-by-domain rollout: one always-on
  store-serve seam for every `LogicalInterest`, regardless of
  cold/warm/offline/online. Offline is the degenerate case where the network half
  delivers nothing.

### Chores

- Remove dead `seed_accounts` test fixture from `nmp-core` (#1110).
- Drop stale `too_many_arguments` allows on `with_relays_and_bootstrap` /
  `build_event_for_verify` (#1111, #1112).
- Move kernel test modules to end-of-file (#1115).
- Move test-only `ArticleHelpers` before test module in gallery-tui (#1116).
- Move `nmp_app_close_contact_feed` before test module (#1113).
- Replace no-op `repeat(1)` with `to_string` in `nmp-store` gc tests (#1118).

---

## nmp-v0.4.0 — 2026-06-12

**BREAKING (C-ABI).** Four `nmp_app_*` C-ABI symbols are removed (see
**Removed** below). Rev-pinned consumers on v0.3.x must migrate before
pinning this revision.

**Android consumers must skip v0.3.0 and pin v0.4.0 directly.** v0.3.0
shipped with Android completely dark (#1084 / V-116): the Android
`KernelUpdateFrameDecoder` was not rebuilt for the typed-frame wire introduced
in v0.3.0 and emitted no snapshot updates. v0.4.0 fixes this with a full
rebuild from Tier-3 typed fields + sidecars (#1092).

### Removed (BREAKING)

- **Legacy author/thread C-ABI open surfaces deleted** (V-68 / V-112,
  ADR-0042; closes #958, #957). The following `nmp_app_*` symbols are
  **removed** from `nmp-ffi` and `NmpCore.h` (#1100):
  - `nmp_app_open_author`
  - `nmp_app_close_author`
  - `nmp_app_open_thread`
  - `nmp_app_close_thread`

  These carried the hardcoded Chirp social-kind default `{1,6}` inside the
  generic FFI layer (a D0 violation) and drove the kernel-resident
  `AuthorViewState` / `ThreadViewState` state machine, which is also deleted
  along with the `author_view` (KAVW) and `thread_view` (KTVW) typed
  projections, their FlatBuffers schemas (`author_view.fbs`,
  `thread_view.fbs`, `timeline_item.fbs`), and the generated Swift readers.
  The projection registry drops from 36 to 34 total keys (28 with Swift typed
  decode stubs).

  **Migration**: open author/thread feeds through the generic seam —
  `nmp_app_open_interest(filter_json, consumer_id, scope)` with a verbatim
  NIP-01 filter (author feed: `{"kinds":[…],"authors":["<pk>"]}`; thread:
  `{"ids":[…]}` + `{"kinds":[…],"#e":["<root>"]}`), paired with
  `nmp_app_close_interest`. The app composes the view from the feed engine
  (see `nmp_app_chirp_open_author_feed` / `nmp_app_chirp_open_thread_feed`
  for the per-app FlatFeed pattern). Profile hydration is component-owned via
  `nmp_app_claim_profile` / `nmp_app_release_profile`.

  The `claimed_profiles` decode cluster is promoted to the public typed
  surface as part of this migration (#1100 fix round).

- **`nmp gen modules` scaffolder + `apps/fixture` deleted** (ADR-0046 —
  "composition is a library, not a generator"). The Rust-shell FFI-crate
  generator (`nmp-codegen`'s `generate` / `ffi_gen` / `workspace` modules, the
  `gen modules` subcommand, the `gen-modules` justfile targets) and its sole
  consumer `apps/fixture` (`nmp-app-fixture` + `fixture-todo-core`) are removed.
  A generated `FfiApp` never installed the required composition owners and was a
  non-functional Nostr app; the fixture existed only to give the orphan
  generator a test target (Opus review #49). The `nmp.toml` manifest parser and
  the Swift `gen swift` / `gen typed-decoders` emitters (live CI gates) are
  unaffected. `nmp init` now scaffolds a thin composition shell.

### Changed (BREAKING)

- **Legacy composition-template package rename** (ADR-0046, now superseded).
  At this release, the old app-template dependency was renamed as a runtime
  composition library instead of a forkable template. That guidance is no longer
  current: modern consumers should depend on explicit owner crates and compose
  protocol modules directly, without reintroducing a hidden starter bundle. See
  `docs/architecture/external-consumers.md`.

### Fixed

- **Android completely dark at v0.3.0 — rebuilt frame decoder from typed
  channels** (closes #1084, #1092). The Android `KernelUpdateFrameDecoder`
  was rewritten from Tier-3 typed fields and sidecars, with a
  real-kernel-frame golden fixture test to prevent silent regression.
  **Android consumers must skip v0.3.0 and pin v0.4.0.**

- **gc_step honest budgets** — resumable Phase-1 cursor, O(1) Phase-2 count,
  hourly Phase-3 gate (closes #1085, #1094). The LRU event-count ceiling
  (`HOT_EVENT_CEILING`) is disabled until store-claims are wired (re-enable
  tracked in #1090; cursor livelock edge case tracked in #1097). Snapshot
  perf gate tightened ~17× to 15 ms / 8 ms from the prior 250 ms / 150 ms
  stale ceilings (#1094).

- **Author-aware watermark rewrite** — multi-author shapes no longer starve
  new follows' backfill (closes #1087, #1091).

### Added

- **Kernel RAM-tier bounds** — events, profiles, and seed_contacts stores now
  enforce HWM eviction with open-interest pin sets (HWMs: events=1 000,
  profiles=2 000, seed_contacts=32), driven from `run_gc_step` (closes #1088,
  #1096).

- **Bunker connection state surfaced on iOS and Android** — new `KBCS`
  (`bunker_connection_state`) typed projection via FlatBuffers, emitted by the
  actor and decoded by per-platform generated decoders (closes #963 / V-14,
  #1098). Follow-up label/tone ADR-0032 conformance tracked in #1099.

- **ADR-0045 store→projection replay accepted** (staged, #1095 / #1086). The
  ADR is merged; implementation is tracked separately.

### Docs / Product

- Zap work declared post-v1 by owner decision (#1089).

---

## nmp-v0.3.0 — 2026-06-11

**BREAKING.** `nmp_app_*` C-ABI symbols are **unchanged** — shell call-sites compile
without modification. The break is at the **Rust API and wire-frame level**: the generic
`payload:Value` field is gone from every `SnapshotFrame`, the generic Value-codec family
is deleted from `nmp-core`'s public API, and `UpdateEnvelope::Snapshot` now carries a
typed `SnapshotEnvelope`. Any downstream consumer that decoded `payload` as a
`serde_json::Value` tree, or called `decode_snapshot_payload` /
`decode_snapshot_with_typed` / `encode_snapshot_value`, must migrate (see **Migration**
below).

### Removed (BREAKING)

- **`payload:Value` eliminated from `SnapshotFrame`** (#1079 PR-B typed-first, #1082
  PR-B final). `encode_snapshot_with_envelope` no longer emits a `payload` field;
  `UpdateEnvelope::Snapshot` now wraps a typed `SnapshotEnvelope` directly. The generic
  JSON blob that was 31% of every frame's wire weight is gone. Reference frame size:
  **14,504 B → 3,384 B (−76.7%)** on an empty frame. `KERNEL_SCHEMA_VERSION` stays at 1:
  the FlatBuffers vtable slot for `payload` is reserved but empty, so readers at any
  version can still parse the frame safely (no schema-version bump required).

- **Generic Value-codec family deleted** (#1082). The following `nmp_core` public functions
  had zero callers after the typed migration and have been removed:
  - `decode_snapshot_payload`
  - `decode_snapshot_with_typed`
  - `encode_snapshot_value`
  All ~20 workspace call-sites were migrated to typed sidecars or `SnapshotEnvelope`
  as part of this release.

### Migration

Downstream apps pinning a pre-0.3.0 revision must update their snapshot read path:

1. **Per-key typed projections** — use the generated per-platform decoders:
   - *iOS/Swift*: `TypedProjectionDecoders.generated.swift` (generated by `nmp gen
     typed-decoders`); call `decode_<key>(typedProjections)` for each key.
   - *Android/Kotlin*: per-key Kotlin decoder classes generated by the Android codegen
     pass (see `android/nmp/typed/`).
   - *Rust consumers*: `nmp_core::typed_projections::decode_<key>(&typed_bytes)` —
     per-key decode functions are now `pub`.

2. **Tier-3 envelope fields** (rev, running, metrics, relay_statuses, toasts) — use
   `decode_snapshot_envelope(&frame_bytes)` → `SnapshotEnvelope`; access fields
   directly. For apps that receive the full update frame, `decode_update_frame` already
   unpacks `UpdateEnvelope::Snapshot(envelope)`.

3. **Auxiliary producers** that need to round-trip a snapshot — use
   `encode_snapshot_frame(&envelope, &typed_sidecars)`.

4. **Change-gated projections** (introduced in 0.2.10) — if you have a high-cost host
   projection, wrap it with `register_snapshot_projection_gated` /
   `NmpApp::register_snapshot_projection_gated` (pass an `Arc<AtomicU64>` rev counter
   as the `ChangeGate`). This is not new in 0.3.0 but is the standard pattern now that
   re-serialization cost on every emit is the dominant remaining CPU charge.

### Added

- **GC wired to actor idle tick — NIP-40 expiry / LRU eviction / tombstone purge now
  run in production** (#1072, #1078). `gc_step` is invoked by the actor on a 60-second
  wall-clock gate (`GC_TICK_INTERVAL`). `GcBudget::production()` caps eviction at
  `HOT_EVENT_CEILING` (10,000 events) per pass. Kernel snapshot exposes
  `last_gc` / `last_gc_at_ms` as observability signals — apps can surface these as
  health indicators.

- **`rustflatc`-drift CI gate** (#1082). `ci/check-rust-flatc-drift.sh` regenerates the
  Rust FlatBuffers bindings in CI and fails the build on divergence. The gate is wired
  into `codegen-drift.yml`. The deprecated `payload` FlatBuffers accessor was removed
  during this regeneration pass.

- **Zap E2E runtime harness** (#1076, `nmp-testing`). End-to-end zap validation test
  suite with an `#[ignore]` real-wallet path (`real_wallet_nip57_zap_roundtrip`) that
  follows the `#[ignore]`→SKIP convention on missing credentials instead of panicking.

- **F-02 cold-start closure-gate integration test** (#1080). The
  `real_relay_nip17_cold_start_kernel` test exercises the `DmRelayListChanged` trigger
  path against `wss://relay.primal.net` and documents the fix for #1080 (kind:10050
  changes never triggering planner recompile, causing cold-start DM receive to fail for
  fresh accounts).

- **Native test CI — Android JUnit on every PR** (#1070). Android JUnit tests now run
  in CI on every pull request, closing the gap where Android-only regressions were
  invisible until device testing.

- **Remaining `flatc --swift` bindings** (#1075, Wave C). FlatBuffers Swift bindings
  for `action_results`, `action_stages`, `author_view`, and `thread_view` added to the
  registry (keys 30 and 31). All 31 registered projections now have Swift decode stubs.

- **Android typed-decoder sweep** (#1074, F-05 Android). Typed FlatBuffers sidecars
  for all rendered projections wired as Kotlin decoders. `AccountSummary.npubShort`
  regression fixed (was always empty on Android).

### Fixed

- **kind:10050 DM-relay-list changes never triggered planner recompile** (#1080).
  Cold-start DM receive was silently broken for fresh accounts: the wildcard ingest arm
  did not enqueue a `DmRelayListChanged` trigger when the `DmInboxRelayLookup` cache
  transitioned. `on_dm_relays_changed` now enqueues the trigger correctly.

- **`GC_TICK_INTERVAL` wasm-facade build break** (#1077, #1078). The constant was not
  gated behind the `native` feature flag and broke the wasm-facade build. Fixed with a
  `#[cfg(feature = "native")]` guard.

- **`AccountSummary.npubShort` always empty on Android** (#1074). The Android typed
  decoder for the `accounts` projection was not decoding the `npubShort` field; now
  reads the typed sidecar correctly.

- **Real-relay nightly `--features` invocation bug** (#1073). The nightly CI job passed
  `--features native,real_relay` in a form that Cargo rejected for `nmp-testing`; the
  invocation is now correct.

- **`real_wallet` test panic → SKIP convention** (#1076). Tests that require live wallet
  credentials now use `std::env::var` and return `Ok(())` with a printed notice instead
  of panicking when credentials are absent.

---

## nmp-v0.2.10 — 2026-06-11

**No `nmp_app_*` C-ABI symbol change.** Existing FFI callers do not need call-site
changes.

### Added

- **Per-projection change-gating for the snapshot registry.** `SnapshotRegistry::register_gated(key, gate, f)` and `NmpApp::register_snapshot_projection_gated` / `AppHost::register_snapshot_projection_gated` / `NmpAppBuilder::register_snapshot_projection_gated` let a host pass an `Arc<AtomicU64>` rev counter as a `ChangeGate`; the registry skips re-invoking the closure and serves the cached value when the gate is unchanged. The ungated `register` / `register_snapshot_projection` path is unaffected. Fixes the "re-serialize the entire library on every kernel emit" CPU peg (measured: ~57% actor-thread time on a 3.6k-episode library). `ChangeGate` is re-exported as `nmp_core::ChangeGate`.

---

## nmp-v0.2.9 — 2026-06-11

**No `nmp_app_*` C-ABI symbol change.** Existing FFI callers do not need call-site
changes.

### Added

- **Generated typed-sidecar Swift decoders (consumer foundation).** `nmp gen typed-decoders`
  emits `TypedProjectionDecoders.generated.swift` — per-projection-key scaffold that
  reads the typed FlatBuffer sidecar (envelope key + schemaId lookup,
  `getCheckedRoot(fileId:)` into the flatc `--swift` reader struct). Decoders compile
  against the two proof-key `flatc --swift` bindings (`accounts`, `active_account`) shipped
  in this release. Consumer wire-up (switching read sites off JSON `payload`) follows in
  the next batch.

---

## nmp-v0.2.8 — 2026-06-11

**No `nmp_app_*` C-ABI symbol change.** Existing FFI callers do not need call-site
changes. Apps that consume typed FlatBuffers snapshot sidecars should regenerate
their generated NMP boundary after bumping the release pin so the new schema
surface is available.

### Added

- **Typed FlatBuffers sidecars for built-in projections and action lifecycle
  views.** The Wave A/C work after v0.2.7 adds typed sidecars for Wallet,
  NIP-29, NIP-47, NIP-57 zaps, follow lists, NIP-23 longform, WOT bootstrap,
  Marmot, NIP-17 inbox/relay-list, publish/outbox, relay/settings,
  identity/views, profile/event, action-lifecycle/diagnostics, bunker
  handshake, and NIP-46 onboarding projections.

- **Typed Tier-3 snapshot envelope fields.** `SnapshotFrame` now carries the
  typed top-level envelope fields from ADR-0044, completing the typed sidecar
  release slice available to downstream apps pinning this baseline.

### Changed

- CI now frees unused preinstalled SDKs before the workspace cargo test job,
  giving the release train more headroom on GitHub-hosted runners.

- The zap subscription runtime now uses the generic per-tick observer seam
  instead of owning a projection-registry-specific tick hook.

- Release readiness now classifies `nmp-nip60` as a public crate and
  `nmp-wallet-poc` as a private proof-of-concept app, and the configured gate
  list matches the scripts that exist in `ci/`.

---

## nmp-v0.2.7 — 2026-06-10

**C-ABI change (additive, non-breaking).** One new `nmp_app_*` FFI symbol added: `nmp_app_encode_profile`. No existing symbol changed or removed — apps can bump the pin without touching their header or existing call sites; adopt the new symbol to delete hand-rolled NIP-19 encoding.

### Added

- **`nmp_app_encode_profile(app, pubkey_hex) → *char`** — NMP-provided NIP-19 identity encoder (closes app-conformance finding H4: "shell hand-rolls NIP-19 bech32"). Prefers an `nprofile` carrying the pubkey plus up to 3 of its kind:10002 write-relay hints **when the kernel already holds them** (no fetch — reads the same mailbox cache the kind:10002 ingest parser fills); falls back to a plain `npub` when there is no relay hint. Lets app shells stop reimplementing bech32/NIP-19 for display. Free the returned string via `nmp_free_string`.

---

## nmp-v0.2.6 — 2026-06-04

**No C-ABI change.** No `nmp_app_*` FFI symbol added, removed, or changed since v0.2.5 — apps can bump the pin without touching their header or call sites. The change below is internal substrate (`ProtocolCommand` workers only).

### Changed

- **V-78 reconcile — one signing seam for `ProtocolCommand` workers.** `nmp-nip57`'s LNURL zap-request signing now goes through the unified `ActorCommand::SignEventForAccount` port (introduced in v0.2.5) instead of a parallel `ProtocolCommandContext::sign_active_nonblocking` path. The redundant `sign_active_nonblocking` context method, its `LocalSignerAccess` trait method, and all impls were deleted — `SignEventForAccount` is now the single, backend-transparent (local + NIP-46 bunker) signing entry point for protocol-crate workers. Bunker accounts continue to zap correctly. No app-facing behavior change.

### Fixed

- CI hygiene: the FFI-header-drift scan list now covers `nmp-app-chirp`'s `ffi/typed_actions.rs`, and a D6 `.unwrap()` lint violation in that file was corrected.

---

## nmp-v0.2.5 — 2026-06-04

**Non-breaking C-ABI.** No existing FFI signature changed. New capabilities are additive (a new dispatchable action, a new optional JSON field, a new protocol crate).

### Added

- **`nmp-blossom` crate — idiomatic Blossom (BUD-02) media uploads.** Dispatch `nmp_app_dispatch_action("nmp.blossom.upload", json)` with `{ file_path, content_type?, servers, signer_pubkey? }`; the crate streams + SHA-256-hashes the blob off the actor thread, builds and signs a kind:24242 auth event (5-minute TTL), PUTs to each server, and surfaces the blob descriptor (`url`, `sha256`, `size`, `type`, `uploaded`) via `action_results[correlation_id]`. The app never handles keys, base64, headers, or continuation-scanning. HTTP lives in the protocol crate (the `ProtocolCommand` seam, like `nmp-nip57`); `nmp-core` stays HTTP-free and noun-free (D0). v1 is upload-only; the `nmp.blossom.*` namespace is built to extend (ADR-0043).

- **`PublishRaw { …, signer_pubkey: Option<String> }`** — a dispatched `nmp.publish` `PublishRaw` action can now sign with a registered non-active signer (an agent / per-podcast NIP-F4 key registered via `nmp_app_signin_nsec(make_active=0)`) by naming its pubkey. Omitted/`None` signs with the active account (unchanged default). The field is `#[serde(default)]`, so existing payloads are unaffected. Local-vs-bunker is transparent.

- **`ActorCommand::SignEventForAccount` — generic, backend-transparent sign-account port.** Internal substrate seam giving any `ProtocolCommand` worker a uniform "sign this unsigned event with account X, then run this continuation with the `SignedEvent`" capability. Generalizes `PendingSignReturn`: local keys resolve inline, NIP-46 bunkers park and resolve async through the same path — worker code is identical and never touches `active_local_keys` or raw key bytes (D13). This is the single signing entry point for protocol-crate workers going forward.

### Fixed

- **V-78 — NIP-46 bunker accounts can now zap.** `nmp-nip57`'s LNURL zap-request signing moved to the non-blocking sign path, so bunker-backed accounts are no longer blocked from zapping.

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
  switch to the `app-owned feed` card-list model.

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
