---
title: NostrMultiPlatform Overview
summary: The NostrMultiPlatform (NMP) project is a Rust-native universal Nostr platform implementing the M2 architecture.
date: 2025-05-19
tags: [nostr, rust, m2, nmp, overview]
---

# NostrMultiPlatform Overview

## What is NMP?

NostrMultiPlatform (NMP) is a Rust-native universal Nostr platform implementing the M2 architecture. It aims to provide a native Nostr experience on every computing platform humans use: from embedded devices and mobile phones to desktop computers and servers.

## Key Goals

1. **Platform Universal**: Native Nostr on every computing platform (embedded, mobile, desktop, server)
2. **Rust-Native Core**: Maximum performance with minimal resource usage; embeds in any application
3. **Social Graph as Platform Foundation**: The social graph is the platform — not just a feature layered on top
4. **M2 Protocol**: The M2 architecture is the core protocol that enables this universal platform

## Architecture at a Glance

NMP follows a workspace-based Rust architecture:

```
nostrmultiplatform/
├── apps/           # Application implementations
├── crates/         # Reusable library crates
├── docs/           # Documentation (aim, product-spec, design, decisions)
├── scripts/        # Build and deployment scripts
└── Cargo.toml      # Workspace manifest
```

## Key Tenets

See [[architecture-tenets]] for the full list of design principles.

## Related

- [[project-structure]] — Detailed workspace organization
- [[architecture-tenets]] — Design principles and philosophies
- [[crate-reference]] — Individual crate documentation
