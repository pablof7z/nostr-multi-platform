# External NMP Consumers

**Decision date:** 2026-06-11 (owner decision, closes PD-033-A [#975](https://github.com/pablof7z/nostr-multi-platform/issues/975))

This document records the known external apps that consume the NMP framework
as an external dependency, serving as evidence that the framework thesis holds
for independent, non-social applications.

---

## Known external consumers (owner-verified 2026-06-11)

| App | Description | Evidence |
|-----|-------------|----------|
| **podcast-player** | Podcast client | `~/Work/podcast-player` (external workspace); pins the composition-root library plus `nmp-core`, `nmp-ffi`, `nmp-signer-broker` at git rev `104c3f76` from `github.com/pablof7z/nostr-multi-platform` (at that rev the composition-root crate was still named `nmp-app-template`; it is `nmp-defaults` from ADR-0046 onward — see the rename note below). Contains `apps/nmp-app-podcast` (~56k LOC Rust composing `ffi/register.rs`, `nmp_dispatch.rs`, `android.rs`) and a ~100k-LOC Swift iOS app. |
| **win-the-day** | Goal/habit tracker | Owner-operated NMP app. |
| **hl** | Highlighter app | Owner-operated NMP app. |

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
