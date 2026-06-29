# NMP — Nostr Multi-Platform

A Rust framework for building Nostr apps. One Rust core, native shells for v1
(iOS SwiftUI, Android Compose, desktop egui), and a browser/wasm runtime whose
per-NIP support is tracked in the v1 support matrix. The core owns everything
that touches the protocol — relays, state, subscriptions, signing, decryption,
replaceable-event resolution, time. The shells render. That's it.

Most cross-platform Nostr clients fragment into incompatible bugs because protocol logic gets reimplemented per platform. Three times. Badly. NMP writes it once, tests it once, and ships it everywhere. The division between protocol and presentation is absolute. That's not a guideline. That's the framework.

## The core idea

You don't pick relays per operation. NIP-65 outbox routing is on by default — posts go where they should, reads come from where they live. You don't handle stale replaceable events; the store will not let you hold a stale kind:0, kind:3, or parameterized-replaceable version. You don't write subscription cleanup; when a view goes away, so do its subscriptions. For the NIP-17 private-message paths that are enabled, plaintext stays in the Rust kernel; Swift, Kotlin, and TypeScript shells do not decrypt DMs.

Every one of those statements is enforced by the type system and the FFI surface, not by documentation. Platform code can't re-route a post because it never picks a relay, and private-flow support is gated by the signer capabilities listed in the NIP matrix.

## Architecture

The kernel is [The Elm Architecture](https://guide.elm-lang.org/architecture/)
ported to Rust and pinned to a single actor thread. One `AppState`, typed read
sessions, typed write workflows, one pure update path. Platform code opens a
session or dispatches a typed write — fire-and-forget, never blocks, never
throws. State arrives back as bounded typed frames. The shell hops to its UI
thread and renders. That's the whole contract. Eleven doctrines (D0-D10) make
the contract enforceable: no app nouns in the kernel, snapshots bounded by
what's open, single writer per fact, no exceptions across FFI, capabilities
report but never decide.

## Getting started

```bash
git clone https://github.com/pablof7z/nostr-multi-platform
cd nostr-multi-platform
cargo install --path crates/nmp-cli   # installs the `nmp` binary
nmp init my-app                        # scaffolds an immediately-buildable app
cd my-app && cargo build
```

The scaffold compiles on first try and is wired through app-owned Rust
composition. From there, generate maintained host bindings/decoders and link a
thin platform shell. The registry at [nostr-mp.f7z.io](https://nostr-mp.f7z.io)
ships SwiftUI, Compose, and web components you can drop in.

## Clean-Room Developer Path

These are the public inputs for building a new app without issue-history context:

- **[`docs/architecture/high-level-app-architecture.md`](docs/architecture/high-level-app-architecture.md)** — start here for the surviving architecture: explicit app composition, typed read sessions, typed write flows, native UniFFI bindings, and browser wasm-bindgen runtime boundaries.
- **[`docs/product-spec.md`](docs/product-spec.md)** — product and DX contract: what the public API promises and what app/native/browser shells must not own.
- **[`docs/builder-guide/00-how-to-read.md`](docs/builder-guide/00-how-to-read.md)** — implementation guide for composing an app, opening typed sessions, dispatching writes, and wiring thin shells.
- **[`docs/ffi-surface.md`](docs/ffi-surface.md)** — binding status and migration notes. Native app code targets UniFFI; browser code targets `nmp-browser-runtime` wasm-bindgen exports.

## Where to go

- **[nostr-mp.f7z.io](https://nostr-mp.f7z.io)** — landing page, component registry, doctrine in full.
- **[`docs/decisions/README.md`](docs/decisions/README.md)** — ADR index. ADR-0069..0073 are the decision spine for the clean-break redesign; use them for rationale and migration history after reading the current API docs.
- **[`docs/nips.md`](docs/nips.md)** — v1 NIP support matrix with platform and signer caveats.
- **[`docs/migration.md`](docs/migration.md)** — v1 runtime and component migration guide.
- **[`docs/aim.md`](docs/aim.md)** — the architectural north star (immutable foundation, not the current API guide).
- **[`AGENTS.md`](AGENTS.md)** — contributor guide, file-size rules, planning discipline.

NMP is open source and in active development. Issues and pull requests are read by people who care about correctness. If you find a bug, the framework has a place where that class of bug can never happen again — and we'll put it there.
