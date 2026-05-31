---
title: NMP CLI — Crate Scope, Commands, and Conventions
slug: nmp-cli
summary: The nmp-cli crate must only modify the crates/nmp-cli/ directory, an additive workspace Cargo.toml member entry, and docs/cli.md
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:3afdf0df-923b-46cb-8fa6-acc61358bb75
  - session:b6578d9e-697f-41ae-ab75-5e5643ceff13
  - session:4eb4e0e2-a9b3-4347-a92b-a073af7adfc0
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# NMP CLI — Crate Scope, Commands, and Conventions

## Scope and Boundaries

The nmp-cli crate must only modify the crates/nmp-cli/ directory, an additive workspace Cargo.toml member entry, and docs/cli.md. The nmp-cli crate must NOT modify ios/**, android/**, crates/nmp-core/**, crates/nmp-content/**, crates/nmp-nip23/**, crates/nmp-testing/**, crates/nmp-codegen/**, docs/builder-guide/**, or docs/perf/m10.5/**. Git commits must use the prefix `feat(cli):`. [^3afdf-3]


## Commands

The nmp CLI ships today with `init`, `gen`, `add`, and `update` commands. The `nmp init <app-name>` command creates a Rust workspace only (Cargo.toml, nmp.toml, app-core crate); full multi-platform starter (iOS/Android) is M16 PLANNED. The `nmp init <app-name>` command accepts a `--path DIR` optional argument and a `--nmp-path` optional argument. The `nmp init` command must canonicalize the `--nmp-path` argument so that scaffolded projects compile on first `cargo check`. App scaffolding is performed by running `cargo run -p nmp-cli -- init <app-name> --nmp-path . --path <dir>` from within the NMP repo. Scaffolded apps must not put nouns in nmp-core; the template must demonstrate the kernel boundary correctly with a generic example rather than a Twitter-shaped one. The scaffolded app-core crate uses a generic `EntryRecord` for its DomainModule, ViewModule, and ActionModule, deliberately avoiding social-app shapes to demonstrate doctrine D0. The scaffolded skeleton must pin nmp-core to the checkout's absolute path so that `cargo check` passes green from any directory, including a tempdir. The `nmp gen modules` command must invoke the existing nmp-codegen pipeline. The `nmp gen modules` command must delegate to the unmodified nmp-codegen library, matching the legacy binary's flags and defaults exactly. When new components are added to the web registry TypeScript, corresponding entries must also be added to registry.toml and the JSON export regenerated via `cargo run -p nmp-cli --bin nmp -- export jsrepo`.

<!-- citations: [^f2605-17] [^3afdf-4] [^b6578-5] [^4eb4e-4] [^56d21-4] -->
## Verification

The scaffolded app must compile and pass `nmp gen modules` immediately after `nmp init` runs, verified by scaffolding into a tempdir and running cargo check on it in a test. Integration tests must verify that `nmp init` in a tempdir results in a passing `cargo check`, passing `cargo test`, and a deterministic `nmp gen modules --check` with no drift. [^3afdf-5]

## Documentation

Docs/cli.md must document every CLI command. The codegen-emitted `nmp-app-<name>` crate's monorepo requirement (due to hardcoded `../../../crates/nmp-core` per ADR-0010) must be documented in docs/cli.md and the generated README.md as a known constraint. [^3afdf-6]
## See Also

