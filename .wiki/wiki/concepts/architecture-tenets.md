---
title: NMP Architecture Tenets
summary: Core design principles and philosophies governing the NostrMultiPlatform project architecture.
date: 2025-05-19
tags: [nmp, architecture, tenets, design-principles, rust]
---

# NMP Architecture Tenets

## Overview

The NostrMultiPlatform project is built on a set of core design principles that guide every architectural decision. These tenets ensure the project remains maintainable, performant, and aligned with its mission of providing universal Nostr support.

## 1. Rust-Native Core

**Tenet**: The entire platform is built in Rust, from the lowest-level cryptographic primitives to the highest-level application code.

**Rationale**:
- **Performance**: Rust provides zero-cost abstractions and memory safety without garbage collection
- **Embedded viability**: Rust's minimal runtime makes it suitable for resource-constrained devices
- **Cross-platform**: Rust compiles to virtually every target platform (iOS, Android, WebAssembly, embedded, server)
- **Safety**: Ownership and borrowing eliminate entire classes of bugs at compile time

**Implications**:
- All crates are pure Rust unless absolutely necessary
- FFI boundaries are carefully managed and documented
- Memory layout is explicitly considered for cache efficiency

**Source**: `docs/aim.md`, `docs/decisions/0001-why-rust.md`

## 2. Social Graph as Platform Foundation

**Tenet**: The Nostr social graph (follows, followers, contact lists, petnames) is not a feature — it is the foundational platform layer.

**Rationale**:
- Nostr's power comes from its social graph, not any single application
- Applications should build on the graph, not recreate it
- Cross-application identity and relationships are core value propositions

**Implications**:
- Identity and graph management are first-class concerns in nmp-core and nmp-identity
- The graph is queryable and traversable by all applications
- Profile and contact list events are treated as system-level primitives

**Source**: `docs/aim.md`

## 3. Vertical Slice First

**Tenet**: Features are built as complete vertical slices — from UI to storage — rather than layer-by-layer horizontal development.

**Rationale**:
- Delivers working, shippable features faster
- Prevents "layer cake" anti-pattern where nothing works until everything is done
- Enables early user feedback and course correction
- Reduces integration risk

**Implications**:
- A new feature might touch crates, apps, and docs all in one changeset
- Cross-crate APIs evolve together, not in isolation
- The "twitter clone on iOS" (Decision 0008) was chosen as the first vertical slice

**Source**: `docs/decisions/0006-vertical-slice-first.md`, `docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md`

## 4. Platform Agnostic Libraries, Platform-Specific Apps

**Tenet**: Library crates (`crates/`) are platform-agnostic; platform-specific code lives only in `apps/`.

**Rationale**:
- Enables maximum code reuse across platforms
- Prevents platform lock-in at the library level
- Simplifies testing and CI (libraries can be tested on any platform)

**Implications**:
- Crates use abstract interfaces (traits) for platform capabilities
- Platform-specific implementations are injected at the app level
- UI rendering abstractions in nmp-rendering allow different backends (Metal, Vulkan, WebGPU, etc.)

**Source**: `docs/design/architecture.md`

## 5. NIP-First Design

**Tenet**: Nostr Implementation Possibilities (NIPs) are treated as specifications, not suggestions. The architecture implements NIPs fully and correctly.

**Rationale**:
- Interoperability with the broader Nostr ecosystem is paramount
- Partial or buggy NIP implementations fragment the ecosystem
- NIPs represent hard-won consensus; deviating risks isolation

**Implications**:
- Each NIP has a dedicated tracking issue or implementation plan
- NIP-29 (group metadata) trust model is explicitly designed (Decision 0013)
- Content rendering pipeline supports NIP-23 (long-form content) natively

**Source**: `docs/decisions/0013-nip29-metadata-signer-trust-model.md`, `docs/design/content-rendering.md`

## 6. Event-Centric Architecture

**Tenet**: Nostr events are the atomic unit of the system. All state is derived from events; there is no separate "database schema" that diverges from the event log.

**Rationale**:
- Aligns with Nostr's fundamental data model
- Enables easy synchronization and conflict resolution
- Simplifies debugging — any state can be reconstructed from events
- Supports offline-first and eventual consistency naturally

**Implications**:
- nmp-core defines the event types and validation rules
- Storage layer (nmp-storage) indexes events but does not invent new data models
- All user actions are represented as events (or event sequences)

**Source**: `crates/nmp-core/src/lib.rs`, `docs/design/architecture.md`

## 7. Incremental Compilation of Knowledge

**Tenet**: Documentation and knowledge are treated as code — versioned, reviewable, and incrementally compiled.

**Rationale**:
- Prevents knowledge silos and bus factor
- Enables LLM-assisted development (this wiki!)
- Makes onboarding new contributors tractable

**Implications**:
- ADRs are mandatory for architectural changes
- Design documents are kept in sync with code
- This `.wiki/` directory exists for LLM-compiled knowledge

**Source**: `docs/decisions/README.md`, `.wiki/config/config.md`

## 8. Security by Design

**Tenet**: Security is not a feature added later; it is woven into every layer from cryptography to UX.

**Rationale**:
- Nostr handles real user funds (zaps) and private data
- Key compromise is catastrophic and irreversible
- Users deserve safe defaults

**Implications**:
- nmp-crypto uses audited, well-known libraries (e.g., secp256k1)
- Private keys never leave secure enclaves where possible
- NIP-29 metadata signer trust model explicitly addresses adversarial scenarios (Decision 0013)
- All network connections use TLS

**Source**: `docs/decisions/0013-nip29-metadata-signer-trust-model.md`

## 9. Developer Ergonomics

**Tenet**: The codebase should be pleasant to work in. Build times are fast, tests are reliable, and tooling is excellent.

**Rationale**:
- Rust's compile times are a known pain point; mitigations are worth investment
- Good tooling reduces cognitive load and bugs
- Developer happiness correlates with code quality

**Implications**:
- Workspace uses `cargo-deny` for dependency auditing (`deny.toml`)
- `typos.toml` catches spelling errors
- Comprehensive agent context in `AGENTS.md`
- CI/CD pipelines enforce formatting, linting, and tests

**Source**: `AGENTS.md`, `deny.toml`, `typos.toml`

## 10. Open Source, Open Protocol

**Tenet**: NMP is open source and builds on open protocols. There are no proprietary extensions or walled gardens.

**Rationale**:
- Nostr's value proposition is openness and permissionlessness
- Lock-in harms users and the ecosystem
- Open source enables community contribution and audit

**Implications**:
- All code is licensed under an OSI-approved license
- No proprietary NIP extensions
- Contributions welcome via standard open source workflows

**Source**: `LICENSE`, `README.md`

## Related

- [[nostr-multi-platform-overview]] — High-level project overview
- [[project-structure]] — How these tenets manifest in the workspace layout
- [[crate-reference]] — How individual crates embody these principles
