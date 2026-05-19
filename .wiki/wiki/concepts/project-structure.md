---
title: NMP Project Structure
summary: Workspace layout, crate organization, and application architecture of the NostrMultiPlatform Rust project.
date: 2025-05-19
tags: [nmp, project-structure, rust, workspace, crates]
---

# NMP Project Structure

## Workspace Layout

NMP is organized as a Cargo workspace with the following top-level structure:

```
nostrmultiplatform/
├── Cargo.toml          # Workspace manifest — declares workspace members
├── Cargo.lock          # Frozen dependency graph
├── README.md           # Project overview and quick-start
├── AGENTS.md           # Agent context and coding guidelines
├── LICENSE             # License file
├── rust-toolchain.toml # Rust toolchain specification
├── deny.toml           # Dependency audit configuration
├── typos.toml          # Typo checker configuration
│
├── apps/               # Application implementations (binaries)
│   └── (mobile, desktop, server apps)
│
├── crates/             # Reusable library crates
│   ├── nmp-core/
│   ├── nmp-relay/
│   ├── nmp-content/
│   ├── nmp-rendering/
│   ├── nmp-identity/
│   ├── nmp-crypto/
│   ├── nmp-network/
│   ├── nmp-storage/
│   ├── nmp-p2p/
│   └── nmp-sync/
│
├── docs/               # Project documentation
│   ├── aim.md          # Project vision and goals
│   ├── product-spec.md # Product requirements
│   ├── design/         # Design documents
│   └── decisions/      # Architecture Decision Records (ADRs)
│
├── scripts/            # Build, test, and deployment scripts
├── vendor/             # Vendored dependencies
├── target/             # Build artifacts
└── .wiki/              # LLM-compiled knowledge base (this directory)
```

## Key Directories

### `apps/`

Application-specific code that consumes the reusable crates. Each app targets a different platform or use case (e.g., iOS, Android, desktop, server).

### `crates/`

The heart of NMP. Each crate is a focused, reusable library:

- **nmp-core**: Core data structures, event models, NIP abstractions, shared primitives. All other crates depend on this.
- **nmp-content**: Content parsing, transformation, rendering pipeline. Handles markdown, media, rich text, and NIP-23 long-form content.
- **nmp-relay**: Relay protocol implementation, WebSocket connections, pool management, REQ/FILTER/CLOSE handling.
- **nmp-rendering**: Cross-platform UI rendering, text layout, media display, and content presentation abstractions.
- **nmp-identity**: Key management, profile handling, contact lists, petnames, and NIP-05 verification.
- **nmp-crypto**: Cryptographic primitives, Schnorr signatures, encryption/decryption, and key derivation.
- **nmp-network**: Lower-level networking, connection pooling, backoff strategies, and transport abstractions.
- **nmp-storage**: Persistent storage, database abstractions, event indexing, and query optimization.
- **nmp-p2p**: Peer-to-peer networking, gossip protocols, and direct connections.
- **nmp-sync**: Synchronization logic, conflict resolution, and state reconciliation across devices.

### `docs/`

- **aim.md**: Project vision — what NMP is trying to achieve
- **product-spec.md**: Product requirements and feature specifications
- **design/**: Deep-dive design documents for subsystems (content-rendering, architecture, protocol design)
- **decisions/**: Architecture Decision Records (ADRs) numbered sequentially (e.g., `0001-why-rust.md`, `0006-vertical-slice-first.md`)

## Dependency Flow

```
nmp-core (base — no internal deps)
    ↑
nmp-crypto, nmp-identity, nmp-network, nmp-storage
    ↑
nmp-relay, nmp-content, nmp-rendering
    ↑
nmp-p2p, nmp-sync
    ↑
apps/
```

## Design Philosophy

The workspace follows **Rust best practices**:

- **Small, focused crates**: Each crate has a single responsibility
- **Minimal cross-crate coupling**: Dependencies flow upward; no circular deps
- **nmp-core as the foundation**: Shared types and primitives live here, keeping the rest loosely coupled
- **Platform-agnostic libraries**: Crates are pure Rust; platform-specific code lives in `apps/`
- **Documentation as code**: Design docs and ADRs live alongside source code

## Related

- [[nostr-multi-platform-overview]] — High-level project overview
- [[architecture-tenets]] — Design principles governing this structure
- [[crate-reference]] — Detailed crate-by-crate documentation
