# NMP — Nostr Multi-Platform

A Rust framework for building Nostr apps. One core, four shells: iOS (SwiftUI), Android (Compose), desktop (iced), web (wasm). The core is Rust and owns everything that touches the protocol — relays, state, subscriptions, signing, decryption, replaceable-event resolution, time. The shells render. That's it.

Most cross-platform Nostr clients fragment into incompatible bugs because protocol logic gets reimplemented per platform. Three times. Badly. NMP writes it once, tests it once, and ships it everywhere. The division between protocol and presentation is absolute. That's not a guideline. That's the framework.

## The core idea

You don't pick relays per operation. NIP-65 outbox routing is on by default — posts go where they should, reads come from where they live. You don't handle stale replaceable events; the store will not let you hold a stale kind:0, kind:3, or parameterized-replaceable version. You don't write subscription cleanup; when a view goes away, so do its subscriptions. You don't decrypt DMs in Swift or Kotlin; NIP-17 plaintext never leaves the kernel.

Every one of those statements is enforced by the type system and the FFI surface, not by documentation. The Swift code can't get NIP-17 wrong because it never sees NIP-17. The Kotlin code can't mis-route a post because it never picks a relay.

## Architecture

The kernel is [The Elm Architecture](https://guide.elm-lang.org/architecture/) ported to Rust and pinned to a single actor thread. One `AppState`, one set of actions, one pure update function. Platform code calls `dispatch(action)` — fire-and-forget, never blocks, never throws. State arrives back as `reconcile(update)` callbacks. The shell hops to its UI thread and renders. That's the whole contract. Eleven doctrines (D0–D10) make the contract enforceable: no app nouns in the kernel, snapshots bounded by what's open, single writer per fact, no exceptions across FFI, capabilities report but never decide.

## Getting started

```bash
git clone https://github.com/pablof7z/nostr-multi-platform
cd nostr-multi-platform
cargo install --path crates/nmp-cli   # installs the `nmp` binary
nmp init my-app                        # scaffolds an immediately-buildable app
cd my-app && cargo build
```

The scaffold compiles on first try and is wired to the local `nmp-core`. From there, generate the per-app FFI surface with `nmp gen modules` and link it from your platform shell. The registry at [nostr-mp.f7z.io](https://nostr-mp.f7z.io) ships SwiftUI and Compose components you can drop in.

## Where to go

- **[nostr-mp.f7z.io](https://nostr-mp.f7z.io)** — landing page, component registry, doctrine in full.
- **[`docs/aim.md`](docs/aim.md)** — the north star. Read first.
- **[`AGENTS.md`](AGENTS.md)** — contributor guide, file-size rules, planning discipline.

NMP is open source and in active development. Issues and pull requests are read by people who care about correctness. If you find a bug, the framework has a place where that class of bug can never happen again — and we'll put it there.
