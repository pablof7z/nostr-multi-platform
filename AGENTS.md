# Repository Guidance

## File Size

- Keep hand-authored source and documentation files under 300 lines of code where practical.
- Treat 500 lines of code as a hard ceiling for hand-authored files.
- Split files by cohesive ownership when they approach the soft limit. Prefer feature modules, sibling views, or linked docs over large catch-all files.
- Generated, vendored, lockfile, binary, and benchmark-output artifacts are exempt from the LOC ceiling, but keep their producers small and documented.

## Architecture: Rust owns all logic; native is rendering + capabilities only

Per the RMP bible (§2, commandment #4 — `docs/aim.md`):

> **No native business logic.** If you would write an `if` statement in Swift, Kotlin, or any native language that decides what the app should *do* (not how it should *look*), that logic belongs in Rust. Native is rendering plus capability execution. Nothing else.

Native code (Swift, Kotlin, TypeScript, etc.) is allowed to do exactly two things:
1. **Render** — translate Rust-produced state snapshots into UI.
2. **Execute capabilities** — call OS APIs (Keychain, AVPlayer, push, location) and report raw results back to Rust. Never decide policy; never retry; never cache.

Everything else — state, business rules, derived data, routing decisions, error recovery, protocol logic — lives in Rust.

## What belongs in NMP crates vs. app-specific Rust crates

**NMP crates (`crates/`)** provide reusable Nostr infrastructure. A feature belongs in an NMP crate when it is a general building block that any Nostr application — or a meaningful subset of Nostr applications — could use directly. Examples: relay management, signing, NIP implementations, event storage, timeline projection, encrypted messaging, identity. The test: *"would this crate be useful to a completely different Nostr app?"* If yes, it is an NMP crate.

**App Rust crates (`apps/<app>/`)** hold the Rust side of features that are specific to that application's domain and would not generalize to other Nostr apps. Example: a podcast app's audio playback engine, chapter parsing, or feed-subscription state. These belong in the app's own Rust crates, not in NMP. NMP does not accumulate app-specific logic.

The line is not protocol vs. product — a product-level feature (e.g., NIP-29 group chat, Marmot MLS encrypted groups) belongs in an NMP crate if other Nostr apps would use it. The line is **generic Nostr building block vs. this app's proprietary domain**.

## No polling — ever

Polling is forbidden at every layer of the stack. This means no `sleep` + check loops, no `Timer.scheduledTimer` querying state, no `try_recv` + `sleep` spin loops, no `Task { while !cancelled { sleep; checkState() } }` tasks.

Use blocking primitives or event-driven patterns instead:
- **Rust channels**: block with `recv()` / `recv_timeout()`; drain with `try_recv()` (not in a sleep loop).
- **iOS**: consume `ViewBatch` snapshots pushed by the kernel; use `AVFoundation` / `NWPathMonitor` / `NotificationCenter` callbacks for OS events.
- **Background persistence**: piggy-back on an existing event tick with a wall-clock gate — do not spawn a parallel sleep loop.

Full rationale: `docs/builder-guide/06-reactivity-contract.md` §Anti-patterns and Doctrine D8.
