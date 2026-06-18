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
| **hl** | Highlighter app | `~/Work/hl` (external workspace). Does **not** pin by git rev — uses local `path` deps to `../../../nostr-multi-platform/crates/*` (nmp-core, nmp-ffi, nmp-defaults, nmp-signers, nmp-nip11, nmp-nip29, nmp-blossom, nmp-content, nmp-kinds), so it tracks whatever the monorepo checkout is at. On the post-keystone API (`NmpAppBuilder`, by-value `register_action`). **Migration note:** the raw event tap (`RawEventObserver` / `nmp_app_register_raw_event_observer`) it used for the nostrdb mirror is retired, and the speculative push-sink replacement is gone too. `hl`'s nostrdb mirror will migrate to the forthcoming bounded **pull-cursor** consumption API (a store ingest-log cursor). Until that lands it has no live-delivery FFI (tracked as follow-up). |
| **win-the-day** | — | **NOT an NMP consumer as checked out** (`~/Work/win-the-day-app`, 2026-06-14): pure SwiftUI/Watch app, zero Rust / zero `nmp-*` linkage (only `secp256k1.swift` + a `nostrsigner` URL scheme). Previously listed here as an "owner-operated NMP app"; the local checkout shows no NMP dependency. **Owner: reconcile — either a different app was intended, or it was never wired to NMP.** |

---

## Consumption contract

External consumers pin NMP by **git rev** (not a crates.io version). The release
process (cutting a tag, bumping `Cargo.toml` workspace version, updating
`CHANGELOG.md`) is documented in the `nmp-release-and-consumption` memory note
and the release scripts under `release/`. Consumers update their pin to a new
rev when they want to pick up framework changes.

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
