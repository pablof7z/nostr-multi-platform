---
title: NMP Crate Reference
summary: Comprehensive reference for all crates in the NostrMultiPlatform workspace, organized by layer and responsibility.
date: 2025-05-19
tags: [nmp, crates, reference, rust, workspace]
---

# NMP Crate Reference

## Workspace Overview

The NostrMultiPlatform workspace is organized into layers from foundational primitives at the bottom to user-facing applications at the top.

## Layer 1: Core Primitives (`crates/nmp-core`)

### `nmp-core`

**Path**: `crates/nmp-core/`

The foundation of the entire platform. All other crates depend on `nmp-core`.

**Responsibilities**:
- Core data structures (events, keys, relays, filters)
- NIP abstractions and typed event wrappers
- Cryptographic primitives via `secp256k1`
- Serialization/deserialization (JSON, binary)
- Error types and result patterns used across the workspace

**Key Types**:
- `Event`, `PublicKey`, `SecretKey`, `RelayUrl`
- `Filter`, `Subscription`, `Req`
- `NipXX` typed wrappers for standard NIPs

**Dependencies**: External crates only (`serde`, `secp256k1`, `thiserror`, `url`, etc.)

---

## Layer 2: Protocol Implementations

### `nmp-nip01`

**Path**: `crates/nmp-nip01/`

Implements NIP-01: the base protocol for Nostr. All higher-level NIPs build on this.

**Responsibilities**:
- Text notes (kind:1)
- Metadata (kind:0)
- Contacts (kind:3)
- Base relay communication patterns

### `nmp-nip04`

**Path**: `crates/nmp-nip04/`

NIP-04: Encrypted direct messages.

**Responsibilities**:
- AES-256-CBC encryption/decryption
- Shared-secret derivation
- DM event construction and parsing

### `nmp-nip05`

**Path**: `crates/nmp-nip05/`

NIP-05: Mapping Nostr keys to DNS-based identifiers.

**Responsibilities**:
- `.well-known/nostr.json` fetching and validation
- Identifier resolution and caching

### `nmp-nip06`

**Path**: `crates/nmp-nip06/`

NIP-06: Basic key derivation from mnemonic seed phrases.

**Responsibilities**:
- BIP-39 mnemonic handling
- Key derivation paths

### `nmp-nip07`

**Path**: `crates/nmp-nip07/`

NIP-07: Browser extension API for Nostr (window.nostr).

**Responsibilities**:
- Extension detection and communication
- Signing delegation to browser extensions

### `nmp-nip10`

**Path**: `crates/nmp-nip10/`

NIP-10: Conventions for `e` and `p` tags in text notes (reply threading).

**Responsibilities**:
- Thread marker parsing (`root`, `reply`, `mention`)
- Reply chain reconstruction

### `nmp-nip18`

**Path**: `crates/nmp-nip18/`

NIP-18: Reposts.

**Responsibilities**:
- Generic repost (kind:6) and quote post (kind:1 with `q` tag) handling

### `nmp-nip22`

**Path**: `crates/nmp-nip22/`

NIP-22: Comment events.

**Responsibilities**:
- Comment threading on any event kind
- Comment-specific metadata

### `nmp-nip29`

**Path**: `crates/nmp-nip29/`

NIP-29: Group metadata and moderation. See also [[nip29-metadata-signer-trust-model]].

**Responsibilities**:
- Group creation, membership, and roles
- Moderation actions (kind:9000–9006)
- Metadata signer trust model (ADR-0013)

### `nmp-reactions`

**Path**: `crates/nmp-reactions/`

NIP-25: Reactions (emoji responses to events).

**Responsibilities**:
- Reaction event construction (kind:7)
- Reaction aggregation and counting

---

## Layer 3: Content & Rendering

### `nmp-content`

**Path**: `crates/nmp-content/`

Content parsing, transformation, and rendering pipeline.

**Responsibilities**:
- Markdown parsing and sanitization
- Media embedding (images, videos, audio)
- NIP-23 long-form content support
- Rich text rendering
- Content transformation (e.g., auto-linking URLs, hashtag detection)

**Key Design Principle**: Content is parsed once and rendered many times. The pipeline produces an intermediate representation (IR) that any renderer can consume.

**Source**: `docs/design/content-rendering.md`

---

## Layer 4: Threading & Timeline

### `nmp-threading`

**Path**: `crates/nmp-threading/`

Conversation threading and timeline construction.

**Responsibilities**:
- Thread tree reconstruction from reply chains
- Timeline ordering (chronological, engagement-based)
- Event deduplication within threads

---

## Layer 5: MLS / Marmot

### `nmp-marmot`

**Path**: `crates/nmp-marmot/`

MLS (Messaging Layer Security) over Nostr — "Marmot" protocol.

**Responsibilities**:
- MLS group management via Nostr events
- End-to-end encrypted group messaging
- Group state projection and synchronization
- FFI bridge for iOS (Chirp) and Android

**Key Design**: All MLS types stop at the crate boundary. The FFI exposes only hex strings and JSON, never native MLS types. See `AGENTS.md` (Chirp section) for the doctrine on this.

---

## Layer 6: Code Generation

### `nmp-codegen`

**Path**: `crates/nmp-codegen/`

Internal code generation for NMP. Not a runtime dependency.

**Responsibilities**:
- Generate typed NIP modules from specs
- Scaffold new apps (`nmp init`)
- Protocol stub generation

---

## Layer 7: Desktop App

### `nmp-desktop`

**Path**: `crates/nmp-desktop/`

Desktop application crate. Cross-platform GUI using Tauri or similar framework.

**Responsibilities**:
- Window management
- System tray integration
- Desktop-specific storage and notifications

---

## Layer 8: CLI

### `nmp-cli`

**Path**: `crates/nmp-cli/`

Developer-facing command-line interface.

**Responsibilities**:
- `nmp init` — scaffold a new NMP app
- `nmp gen modules` — run code generation
- Development tools and debugging utilities

**Note**: The binary is named `nmp`, not `nmp-cli`. See crate-level docs for the relationship to the legacy `nmp` binary in `nmp-codegen`.

---

## Application Layer

### `nmp-app-chirp`

**Path**: `apps/chirp/nmp-app-chirp/`

Chirp iOS application. A proof-of-concept thin shell demonstrating NMP reusability.

**Key Tenet**: Chirp contains **zero business logic**. All logic lives in NMP crates. Chirp only wires them together and provides FFI symbols for the iOS Swift shell.

**Crate Type**: `staticlib` + `rlib` — produces a static library for iOS linking.

**FFI Symbols**:
- `nmp_app_chirp_register`
- `nmp_app_chirp_snapshot`
- `nmp_app_chirp_snapshot_free`
- `nmp_app_chirp_unregister`

**Source**: `apps/chirp/AGENTS.md` (Chirp-specific agent guidance)

---

## Dependency Flow Summary

```
nmp-core
    ↑
nmp-nip01, nmp-nip04, nmp-nip05, nmp-nip06, nmp-nip07,
nmp-nip10, nmp-nip18, nmp-nip22, nmp-nip29, nmp-reactions,
nmp-content, nmp-threading
    ↑
nmp-marmot, nmp-desktop
    ↑
nmp-app-chirp, nmp-cli
```

## Design Patterns Across Crates

### FFI Doctrine

Crates that expose FFI (like `nmp-app-chirp` and `nmp-marmot`) follow strict rules:
- All FFI symbols are `#[no_mangle] extern "C"`
- Complex types are serialized to JSON at the boundary
- Errors are returned as strings, never Rust `Result`
- Null handles degrade gracefully (return null / `{"ok":false}`)

### Test Support

Many crates expose `test-support` feature flags that unlock internal constructors and helpers for integration testing. These are **never** enabled in production.

### Workspace Inheritance

All crates inherit `version`, `edition`, and `license` from the workspace root:

```toml
[package]
name = "nmp-xxx"
version.workspace = true
edition.workspace = true
license.workspace = true
```

## Related

- [[project-structure]] — How crates fit into the workspace layout
- [[architecture-tenets]] — Design principles governing crate design
- [[nostr-multi-platform-overview]] — High-level project goals
