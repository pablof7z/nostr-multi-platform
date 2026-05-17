# Lessons from `podcast-rmp`

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).
> Source repo: `/Users/pablofernandez/src/podcast-rmp` (working WIP at task time).

The podcast-rmp project is a parallel attempt to rebuild a podcast app on the RMP (Rust Multi-Platform) skeleton. **Important context**: its reference Swift app is `/Users/pablofernandez/Work/podcast` (which is not present on this filesystem and is larger than ours — Wiki, Watch, CarPlay, NIP-46, briefings). Their experience does **not** transfer feature-for-feature, but it transfers in three concrete ways: build hygiene, architectural commandments learned the hard way, and the specific shape of their drift.

This doc is structured: **what they tried · what worked · what didn't · what we change.**

---

## 1. The DerivedData sprawl problem (build hygiene)

### 1.1 What happened

Their `CLAUDE.md` says it directly:

> Each agent worktree that invokes `xcodebuild` … creates a separate DerivedData directory under `~/Library/Developer/Xcode/DerivedData/`, keyed by the absolute path of the `.xcodeproj`. Over time this has produced 10+ `Podcastr-*` directories (~1 GB each).

This is a real disk-pressure problem in parallel-agent execution. Multiplied across our planned parallel-lane mode for M11 (Library, Feed, Player, Insights, Ask, Settings, Components — 7+ worktrees concurrently), this is 10+ GB of redundant build state per day, and the iCloud Backup background daemon thrashes when these directories are not gitignored.

### 1.2 What we change — day-one mitigation

**Cargo target dir:** the workspace `justfile` and every worktree's environment export

```bash
export CARGO_TARGET_DIR="$HOME/.cargo-shared-target"
```

so all Rust artifacts share one tree. Set via a wrapper in `tools/env.sh` sourced by every `just *` recipe.

**Xcode DerivedData:** every `xcodebuild` call passes

```bash
-derivedDataPath "$HOME/.cargo-shared-target/xcode-derived-data"
```

so all iOS builds share one tree. Set in `justfile` recipes that invoke `xcodebuild` (`build-ios`, `run-ios`, future `build-ios-podcast`, `run-ios-podcast`).

**Worktree TTL:** the parent agent's worktree-spawning helper enforces

```bash
git worktree remove --force "$WT" && git branch -D "$BR" || true
```

on completion (`docs/plan.md` §4 "Worktree hygiene" already mentions this; we make it a documented contract in `tools/agent-worktree.sh`).

These three together cap the on-disk footprint at the single-tree size, not the worktree-count multiple.

---

## 2. The architectural ownership lessons

Three things they got right (or had to learn) that we should not re-learn:

### 2.1 Rust owns playback policy; native owns OS handles only

Their `FINAL_PLAN.md §4` is explicit: AVPlayer/AVAudioSession/MPNowPlayingInfoCenter execution lives in Swift, but the **state machine** (queue, rate, skip, sleep-timer, mark-played, ad-skip) lives in Rust. Their parity work flagged repeated drift where Swift acquired policy ("if we're already playing this episode, don't reload"). We codify this in [`capabilities.md`](capabilities.md) §A — the `AudioPlaybackCapability` bridge holds the `AVPlayer` and the `AVAudioSession` only; **every** decision is Rust-side, including idempotent-load detection.

### 2.2 The god-module avoidance rule

Their RMP bible appendix (and `FINAL_PLAN.md §8`) says:

> When the core actor file exceeds ~1,000 lines, split by domain into submodules with `pub(super)` visibility.

The Pika reference (their bible's reference app) has a 4,600-line `core/mod.rs`. We avoid the same by splitting `podcast-core` from the start into per-noun submodules ([`podcast-core.md`](podcast-core.md) §B–D show the exact split: `domain/<noun>.rs`, `views/<view>.rs`, `actions/<verb>.rs`). AGENTS.md's 500-LOC hard cap enforces this automatically.

### 2.3 Capability bridges are bridges, not policies

Repeated failure mode in podcast-rmp: bridges accumulating state ("a download retry counter on the iOS side") that should be in Rust. The cure is the bounded-state proof per capability (every entry in [`capabilities.md`](capabilities.md) has one).

### 2.4 SwiftData parity work happens early

Their `FINAL_PLAN.md §11` ("Migration Strategy") was deferred to M12 (penultimate milestone). They later learned that **schema-level domain modeling needs to be right before view wiring begins**, because the view payloads are direct projections of the domain shape. We make the inverse choice: [`podcast-core.md`](podcast-core.md) §B is the **first** thing built; views come after.

---

## 3. What we explicitly do *not* take from podcast-rmp

### 3.1 Their scope

Their target app has Wiki, Briefings, CarPlay, Watch, NIP-46, NIP-44, Cashu, peer-agent tool gates, App Intents/Siri, Live Activities — none of which exist in `/Users/pablofernandez/src/podcast`. Including them in M11 would defeat the kernel-boundary proof (the kernel-boundary check is only meaningful when the app stays small enough that the kernel could conceivably grow podcast nouns to make it easier).

### 3.2 Their crate names

`podcastr-domain` / `podcastr-storage` / `podcastr-feed` / etc. follow their style guide. Our crate names follow ours: `podcast-core` / `podcast-llm` / `podcast-rag` / `podcast-feeds`. No `podcastr-*` namespace.

### 3.3 Their 14-milestone roadmap

That's their roadmap for their bigger app. Ours is M11 — one milestone — because we're proving a single architectural claim (the kernel boundary), not shipping a v1 product.

### 3.4 Their xtask layer

Their `cargo xtask` machinery is a fine tool for their team, but NMP already has `justfile` + `nmp-codegen` covering the same surface. We don't introduce a parallel orchestrator.

### 3.5 Their UI fidelity restoration plan

Their `docs/plans/iphone-view-fidelity-restoration.md` (287 LOC) documents a painful weeks-long effort to recover UI fidelity *after* drift had set in. We do the opposite — Step 0 ([`copy.md`](copy.md)) is the first commit in M11, and the screenshot diff gates from line 1.

---

## 4. What we share

- The actor pattern. (Already adopted via ADR-0009.)
- TEA shape (`AppState`/`AppAction`/`handle_message`). (Adopted.)
- "No business logic in native." (Doctrine D0 + AGENTS.md guardrails.)
- Capability bridges idempotent + bounded. (Codified per [`capabilities.md`](capabilities.md).)
- sqlite-vec for the vector store. (Adopted per [`podcast-rag.md`](podcast-rag.md) §A.)

---

## 5. Concrete artifacts copied (none) vs adapted (these)

- `~/.cargo-shared-target` + `-derivedDataPath` pattern — adopted (this doc §1.2).
- The sqlite-vec iOS bundling spike — adopted as risk register (see [`risks.md`](risks.md)).
- The "Rust owns policy; native owns handles" review checklist — adopted (per-capability bounded-state proof).
- The god-module-avoidance rule — adopted (file-LOC ceiling in AGENTS.md is the enforcement).

No source code from `podcast-rmp` is copied into NMP. The lessons are imported; the implementation is fresh.
