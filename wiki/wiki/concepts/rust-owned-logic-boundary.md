---
title: "Rust-Owned Logic Boundary"
summary: "NMP keeps business logic, state, routing, and recovery in Rust while native shells render snapshots and execute capabilities."
tags: [doctrine, ffi, native]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-source-map.md"
---

# Rust-Owned Logic Boundary

NMP's central product boundary is simple: Rust decides what the app does; native
platform code renders and executes platform capabilities. This applies to Swift,
Kotlin, TypeScript, TUI code, and any other host shell.

The boundary is stricter than "share some code in Rust". Protocol logic, state,
relay routing, signing orchestration, derived views, retry/recovery policy, and
capability outcomes all flow through the Rust actor. Native code receives a
snapshot, renders it, dispatches intents, and reports raw OS results back.

## What Native Can Own

Native shells can own:

- UI rendering and layout;
- transient OS handles;
- capability execution, such as Keychain, file picker, push, camera, or network
  monitor calls;
- thread hops needed by the platform UI model.

Native shells cannot decide policy. They do not choose relays, retry failed
Rust operations, maintain derived caches, decide whether missing profile data is
a loading state, or patch state locally after dispatch.

## Why This Is a Doctrine Boundary

The boundary is how NMP makes cross-platform behavior consistent. If Swift and
Kotlin both implement retry policy, relay selection, or profile hydration, they
will eventually differ. If Rust owns those decisions once, every platform gets
the same product behavior.

The boundary also keeps FFI small and auditable. Host code sends actions and
receives state frames. It does not reach into kernel internals for synchronous
answers.

## Common Failure Mode

The failure mode to watch for is "thin shell in name only": Swift or Kotlin
decodes raw data, applies product rules, updates local caches, then asks Rust to
catch up. That inverts D4. The native cache becomes a second writer for facts
that Rust owns.

## See Also

- [[runtime-update-transport|Runtime Update Transport]] ([Runtime Update Transport](../topics/runtime-update-transport.md))
- [[subscription-planning-and-routing|Subscription Planning and Routing]] ([Subscription Planning and Routing](../topics/subscription-planning-and-routing.md))

## Sources

- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
