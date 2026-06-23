# External NMP Consumers

**Decision date:** 2026-06-11 (owner decision, closes PD-033-A [#975](https://github.com/pablof7z/nostr-multi-platform/issues/975))

This document records the known external apps that consume the NMP framework
as an external dependency, serving as evidence that the framework thesis holds
for independent, non-social applications.

---

## Known external consumers (re-verified 2026-06-14)

| App | Description | Evidence |
|-----|-------------|----------|
| **podcast-player** | Podcast client | `~/Work/podcast-player` (external workspace); pins `nmp-core`, `nmp-ffi`, `nmp-defaults`, `nmp-signer-broker`, `nmp-blossom`, `nmp-nip02` by git rev from `github.com/pablof7z/nostr-multi-platform` (bumped to **nmp-v0.7.0 / rev `ce0097cde`** on 2026-06-14, the keystone-series release — ADR-0050/0052/0056). Carries a local `[patch]` redirecting `nmp-blossom` to a `/tmp/nmp-at-<rev>/` extraction because blossom is parked out of the NMP cargo workspace (post-v1 dead island). Contains `apps/nmp-app-podcast` (~56k LOC Rust composing `ffi/register.rs`, `nmp_dispatch.rs`, `android.rs`) and a ~100k-LOC Swift iOS app. Fully on the post-keystone API (register-by-value `ActionModule`, `nmp_defaults::register_defaults`, per-app signer ports). |
| **hl** | Highlighter app | `~/Work/hl` (external workspace). Does **not** pin by git rev — uses local `path` deps to `../../../nostr-multi-platform/crates/*` (nmp-core, nmp-ffi, nmp-defaults, nmp-signers, nmp-nip11, nmp-nip29, nmp-blossom, nmp-content, nmp-kinds), so it tracks whatever the monorepo checkout is at. On the post-keystone API (`NmpAppBuilder`, by-value `register_action`). **Migration note:** the raw event tap (`RawEventObserver` / `nmp_app_register_raw_event_observer`) it used for the nostrdb mirror is retired, and the speculative push-sink replacement (`nmp_app_register_event_sink` / retain-until-ack / `created_at` resync) is gone too. `hl`'s nostrdb mirror migrates to the **pull cursor** (ADR-0058). See the **Host mirror consumption contract** section below. The in-repo boundary is locked: `no_raw_tap_reintroduction` prevents either deleted shape from reappearing. |
| **Olas** | Picture-first social app | `~/Work/Olas` (external workspace). Composes NMP through its `apps/olas/nmp-app-olas` crate and uses the generic `nmp_app_open_interest` seam for kind:20 feeds plus `nmp.publish` / `PublishRaw` for NIP-68 picture posts. Olas is the concrete external consumer that pulled the generic `nmp-nip68` picture-event wrapper forward: reusable NIP-68 parse/build logic belongs in the protocol crate, while Olas keeps app policy such as feed mode, ranking, onboarding, image editing, and UI. |
| **win-the-day** | — | **NOT an NMP consumer as checked out** (`~/Work/win-the-day-app`, 2026-06-14): pure SwiftUI/Watch app, zero Rust / zero `nmp-*` linkage (only `secp256k1.swift` + a `nostrsigner` URL scheme). Previously listed here as an "owner-operated NMP app"; the local checkout shows no NMP dependency. **Owner: reconcile — either a different app was intended, or it was never wired to NMP.** |

---

## Host mirror consumption contract (ADR-0058)

An out-of-tree app that needs a complete, durable copy of the event log (e.g.
`hl`'s nostrdb mirror) uses the **pull cursor**, not a push callback. The
canonical contract, in order:

1. **Register** a `GlobalLog` cursor in `Protected { max_lag_entries }` mode
   via `AdvancePullCursor` (or the FFI equivalent once step 3 of the ladder
   lands). The cursor persists only its `consumer_id` + `scope` + `mode` +
   `after_seq` — restart re-registers with the persisted `after_seq`.

2. **Receive** the `nmp.pull.wake { cursor_id, latest_seq }` typed projection
   (ADR-0037). This is level-triggered: it re-fires while
   `after_seq < latest_seq`. Rust exposes `decode_pull_wake_batch` in
   `crates/nmp-core/src/kernel/pull_wake.rs`; platform-specific host decoder
   glue belongs with the first in-repo consumer (not here — `hl` is
   out-of-tree).

3. **Drain** by calling `nmp_mirror_pull_page` (synchronous, read-only) until
   `has_more == false` or a budget is exhausted. Each `PullPage` carries
   `next_after_seq / latest_seq / has_more`.

4. **Apply** each `StoreLogEntry` to the mirror store:
   - `Inserted` / `Replaced` → upsert the event into the mirror store.
   - `Deleted { Nip09 | Nip40Expiry | AdminPurge }` → advisory signal; a
     durable mirror MUST also apply NIP-09 from every kind:5 `Inserted` row
     and NIP-40 from the `expiration` tag on its held events (the store may
     have retention-evicted a target before a kind:5 arrived, emitting no
     `Deleted` row — the mirror still acts because it holds the kind:5 itself).

5. **Persist** `after_seq` (the `next_after_seq` from the last page) to
   durable storage **after** fully applying the page. This is the crash-recovery
   source of truth; the kernel cursor registration is not durable.

6. **Advance** the cursor via `AdvancePullCursor(consumer_id, after_seq)` so
   the kernel can release the log-GC floor pin up to this position.

**Mirror-as-semantic-superset invariant:** the mirror keeps events the
producing store retention-evicted and never deletes its copy on a retention
eviction — only on a semantic NIP-09 / NIP-40 / AdminPurge delete. See
ADR-0058 §5 for the full contract.

**What is NOT the mirror API:**
- `nmp_app_register_event_sink` / ack-callback / retain-until-ack — this was
  the #1552-deleted native push sink. It is permanently gone.
- `ExternalEventSinkPolicy` / `ExternalEventSinkDispatcher` — these are the
  in-process relay-forwarding policy, not an external consumer API.

---

## Consumption contract

External consumers pin NMP by **git rev** (not a crates.io version). The release
process (cutting a tag, bumping `Cargo.toml` workspace version, updating
`CHANGELOG.md`) is documented in the `nmp-release-and-consumption` memory note
and the release scripts under `release/`. Consumers update their pin to a new
rev when they want to pick up framework changes.

### Pre-v1 compatibility stance

External consumers are conformance evidence, not an API freeze. Before v1, a
consumer pinning a git rev accepts that the next rev may remove, rename, or
reshape public symbols when the current surface is legacy, duplicative,
example-named, or architecturally wrong. NMP does not keep aliases, wrappers,
fallback wire tags, or deprecated schema slots solely to avoid updating a
pre-v1 consumer. The correct response to a real consumer break is to migrate the
consumer to the cleaner framework surface and, when the break reveals a missing
generic capability, fix that capability in NMP.

### BREAKING rename — `nmp-app-template` → `nmp-defaults` (ADR-0046, 2026-06-12)

ADR-0046 ("composition is a library, not a generator") renamed the
composition-root crate `nmp-app-template` → `nmp-defaults` (the public API —
`register_defaults`, `NmpAppBuilder`, every symbol — is unchanged). When a
consumer bumps its git rev across this change it must **rename the dependency**:
`nmp-app-template = { git = … }` → `nmp-defaults = { git = …, package =
"nmp-defaults" }`, and `use nmp_app_template::…` → `use nmp_defaults::…`. The
same ADR deleted the unused `nmp gen modules` scaffolder and the `apps/fixture`
crate; no consumer depended on either.

---

## Framework feedback loop

Conformance feedback from external consumers flows back as framework input. The
`docs/builder-guide/conformance/` catalog tracks known gaps that consumer apps
surfaced (e.g., the EmbedHost and Podcastr findings for kernel-emitting raw
events instead of typed projections — gap referenced as A5 in the conformance
catalog).

This feedback loop is load-bearing evidence for the framework thesis: a gap
surfaced by a real external consumer is a framework defect, not an app defect,
and must be fixed in the kernel or projection layer.

---

## Relation to PD-033-A

[PD-033-A #975](https://github.com/pablof7z/nostr-multi-platform/issues/975)
required a "stateful second-app gate." The owner determined on 2026-06-11 that
the existing external consumer apps — in particular `podcast-player`, which
composes framework seams (`ffi/register.rs`, `nmp_dispatch.rs`, `android.rs`)
at ~56k LOC Rust and ships a full iOS app — satisfy this gate. The issue is
closed.
