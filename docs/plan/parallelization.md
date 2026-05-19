# Parallelization opportunities

> Part of the [Build & Validation Plan](../plan.md).

The ladder above is the **dependency order** — what must precede what — not a wall-clock schedule. Genuine parallel work tracks:

- **[M2](m2-subscription-compilation.md) (outbox), [M3](m3-persistence.md) (LMDB), [M4](m4-negentropy.md) (negentropy)** can pipeline tightly: M3 + M4 are almost mechanically pluggable once M2's compiled-plan abstraction exists.
- **[M5](m5-nip42.md) (NIP-42)** is independent of M3/M4 and can be done alongside.
- **[M6](m6-signers-write.md) (signer + write path) is a serialization point** — most downstream milestones ([M7](m7-interaction-loop.md), [M8](m8-multi-account.md), [M9](m9-messaging.md), [M10](m10-blossom.md), [M12](m12-wallet.md)) depend on it. Land this fast.
- **[M10.5](m10.5-ffi-hardening.md) (FFI hardening)** is itself parallelizable: the stress harness, the iPhone-12 perf rerun, the UI-script Sonnet-agent fleet, and the FFI surface audit are four independent workstreams.
- **[M11](m11-podcast.md) (podcast app)** starts only after M10.5 passes. Its own internal parallelism is wide: the copy step + each `*-core` Rust extension crate + each view-wiring batch can be split across agents (one per view group: Library, Feed, Player, Insights, Ask, Settings, Components, plus one agent per LLM/RAG/feeds module).
- **[M15](m15-cross-platform.md) (Android + Desktop + Web)** is three parallel tracks once [M14](m14-uniffi.md) (UniFFI) lands.

A team of two could run M5 alongside the M2–M4 sequence with no integration risk. With parallel-agent execution (this session's mode), the practical limit is conflict surface: independent crates, independent docs, and independent platform shells fan out cleanly; shared mutable files (e.g. `nmp.toml`, the codegen output, `Cargo.toml`) serialize.

## Worktree hygiene

Every parallel worker that mutates source operates in its own git worktree under `.claude/worktrees/`. **On merge, the worktree is removed** (`git worktree remove --force` + branch cleanup) by the worker before the parent acknowledges done — otherwise DerivedData and `target/` clones blow out the disk fast. The known precedent is podcast-rmp's `~/Library/Developer/Xcode/DerivedData/Podcastr-*` sprawl; we share `CARGO_TARGET_DIR` and `-derivedDataPath` across worktrees from the start to avoid it.
