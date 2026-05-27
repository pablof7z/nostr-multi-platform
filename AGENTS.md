# Repository Guidance

## Agent workflow

- All implementation work must happen in a git worktree owned by the agent doing the work.
- Each agent is responsible for its own branch/worktree lifecycle. Do not edit from the shared root checkout for feature, fix, or refactor work.
- Before starting work, every agent must read `WIP.md` from the project base directory to understand what other agents are currently doing.
- When an agent starts work, it must add an entry to `WIP.md` in the project base directory with a timestamp, a one-line description of the work, and the git worktree path it is using.
- When an agent finishes work, it must remove its own entry from `WIP.md`.
- When the work is complete, open a pull request before reporting completion. The PR description must include:
  - a short TLDR summary of what changed;
  - a detailed overview of the work performed;
  - any subjective decisions made, including tradeoffs or assumptions.
- Do not open completed work as a draft pull request. If the work is ready and validated, open it as ready for review. Use draft PRs only when explicitly asked or when the work is intentionally incomplete.
- After opening the PR, clean up the agent-owned worktree before completing the task.

### Test scope — local vs. CI vs. merge

When validating your own work locally, **scope `cargo test` to the crates
you touched** — not the whole workspace. Examples:

- Touched only `crates/nmp-nip17/`? Run `cargo test -p nmp-nip17` and
  the obvious downstream consumers (`cargo test -p nmp-app-chirp`,
  `cargo test -p nmp-core --lib nip17` for substrate-side coverage).
- Touched a substrate seam in `crates/nmp-core/src/substrate/`? Run
  `cargo test -p nmp-core --lib substrate` plus every Layer-4 crate
  that imports the seam (`grep -l 'use nmp_core::substrate' crates/`).
- Touched the kernel? Run `cargo test -p nmp-core --lib kernel` and
  the binding crates (`cargo test -p nmp-wasm`, `cargo test -p nmp-ffi`
  if either exists).

`cargo test --workspace` is reserved for the merging agent (the
supervisor running this conversation) and CI. A workspace-wide run can
take 10+ minutes; with multiple agents sharing a cargo target
directory it serializes the build queue and starves every other
worktree. The supervisor enforces full-suite + cross-target coverage
at merge time (see `docs/architecture/crate-boundaries.md` discussion
of soundness gates).

**Always-on local gates** — these are fast and catch the silent
classes of failure that scoped tests miss:

- `cargo test -p nmp-testing --test doctrine_lint_smoke` — the D-rule
  gates (D0 substrate purity, D15 host-closure guards, D11 one-door,
  file-size, etc.) trip silently in plain `cargo test -p <your-crate>`
  runs.
- `cargo build --workspace` (compile-only, fast) if you renamed a
  public symbol, moved a module, changed a Cargo.toml dep path, or
  added a workspace member.

If you cannot tell whether your change is scope-localized — for
example, you removed a public API from `nmp-core` and don't know
every importer — run `cargo build --workspace` and search the
compile errors for the touched symbol. The compile pass is much
faster than the test pass and surfaces the same blast radius.

## Planning discipline — three canonical files, no duplicates

This repository has exactly **three canonical planning/status files**. Every plan, todo, roadmap, milestone, violation, decision, and in-flight task lives in one of them. Scattered notes, ad-hoc `TODO.md` / `NOTES.md` / `ROADMAP.md` / `PLAN-foo.md` files, parallel planning docs, and inline `// TODO:` annotations used as a substitute for tracking are **forbidden**.

| File | Role | Update cadence |
|---|---|---|
| [`docs/plan.md`](docs/plan.md) | Overarching plan — milestones, doctrine, where we are, v1 exit criteria. | Only when a milestone changes status or the v1 exit criteria move. |
| [`docs/BACKLOG.md`](docs/BACKLOG.md) | Tactical queue — active violations, pending user decisions, ordered v1 feature backlog, post-v1 list. | Every PR that touches an item listed here. |
| [`WIP.md`](WIP.md) | Live in-flight tracker — branches currently on a worktree. | When an agent starts work (add entry) and when work merges (remove entry). |

Rules — enforced strictly:

- **Do not create new top-level planning files.** No `PLAN.md`, `TODO.md`, `ROADMAP.md`, `NEXT.md`, `STATUS.md`, or per-feature plan files at the repo root or directly under `docs/`. New detail belongs in one of: `docs/plan/m*.md` (per-milestone), `docs/architecture-audit/<plan>.md` (migration plans gating a specific violation), or `docs/decisions/00NN-*.md` (ADRs). Never as a parallel overview.
- **Do not duplicate state across files.** A violation tracked in `BACKLOG.md §1` is not also a row in `WIP.md`. If an agent is actively fixing it on a branch, only the branch reference lives in `WIP.md`; the violation entry stays in `BACKLOG.md`.
- **Plan files have authority over scattered notes.** A `// TODO:` comment in code is not a plan. If it represents work to be done, it belongs in `BACKLOG.md`. If it represents a known limitation, it belongs in an ADR or doctrine clarification.
- **Single source of truth per fact** — this is D4 (single writer per fact) applied to docs. Memory files, codex review notes (`docs/perf/codex-reviews/`), and audit reports may *reference* the canonical files; they must not become parallel authorities. Post-merge codex review findings are not actionable until they are promoted into `BACKLOG.md`.
- **Edit existing entries; do not append parallel ones.** If a `BACKLOG.md` item changes, edit it in place. If a milestone status in `docs/plan.md` changes, edit the table. Append-only history files (`docs/perf/pending-user-decisions.md`) are explicitly opt-in and named — do not invent new ones.
- **When in doubt, fewer files.** The cost of a duplicate plan is divergence: within one sprint two files will describe different states of the same world, and neither will be trustworthy. If a new file feels necessary, justify why it cannot be a section of one of the three canonical files (or a per-milestone / per-ADR / per-audit sub-page) before creating it.

This discipline is non-negotiable. A PR that introduces a duplicate planning file, a scattered todo list, or a parallel roadmap is rejected and the entries are folded back into the canonical files.

## File Size

- Keep hand-authored source and documentation files under 300 lines of code where practical.
- Treat 500 lines of code as a hard ceiling for hand-authored files.
- Split files by cohesive ownership when they approach the soft limit. Prefer feature modules, sibling views, or linked docs over large catch-all files.
- Generated, vendored, lockfile, binary, and benchmark-output artifacts are exempt from the LOC ceiling, but keep their producers small and documented.

## TEA organization: co-locate by owner, not by role

- Do not create top-level `model/`, `update/`, `view/`, `state/`, or `actions/` buckets whose only purpose is technical role separation.
- Prefer one cohesive module per feature, page, view module, protocol module, or central domain type. Keep the state shape, input messages/actions, reducer/update path, projection/view payload, and tests near that owner.
- The LOC rule still wins. When a cohesive owner approaches the limit, split under the same owner namespace by concrete sub-type or sub-protocol, not by recreating global Model/Update/View layers.
- Keep the top-level actor/router flat until a screen or module has genuinely self-contained state. Compose nested messages deliberately; do not introduce native/local component state to avoid plumbing.

## Architecture: Rust owns all logic; native is rendering + capabilities only

Per `docs/aim.md` §2 (architectural north star, commandment #4):

> **No native business logic.** If you would write an `if` statement in Swift, Kotlin, or any native language that decides what the app should *do* (not how it should *look*), that logic belongs in Rust. Native is rendering plus capability execution. Nothing else.

Native code (Swift, Kotlin, TypeScript, etc.) is allowed to do exactly two things:
1. **Render** — translate Rust-produced state snapshots into UI.
2. **Execute capabilities** — call OS APIs (Keychain, AVPlayer, push, location) and report raw results back to Rust. Never decide policy; never retry; never cache.

Everything else — state, business rules, derived data, routing decisions, error recovery, protocol logic — lives in Rust.

## Effects, replay, and snapshot discipline

- Every external effect is represented as typed data crossing the Rust/native boundary: Rust requests a capability, native reports a raw result, Rust decides the next state.
- New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams. Reducers must remain replayable from message history.
- Debug/history surfaces must use log-safe action tags and correlation ids; never record secrets, raw nsecs, plaintext DMs, or bearer tokens.
- Keep `FullState`/full snapshot as the correctness path. Add granular `ViewBatch` or other delta variants only when profiling proves the snapshot path is the bottleneck and the delta is lossless.

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

## Zero-tolerance on hacks, debt, and fragmentation

**No temporary hacks. Ever.** This is a strict, non-negotiable rule enforced pedantically:

- No "for now" workarounds, stubs that stay, or `// TODO: fix this properly` comments left
  in production code. A staged fix is allowed *only* when a written plan in `BACKLOG.md`
  documents every stage with a completion deadline. An unplanned, undocumented "temporary"
  measure is categorically forbidden — there is no such thing as acceptable technical debt.
- No fragmentation: every concept has exactly one canonical representation and one code path.
  If two paths exist for the same concern, one must be deleted before the PR merges.
- Every change must be done by the book, seeking the long-term correct architecture — not the
  shortest path to a green CI. If the correct fix requires touching 10 files, touch 10 files.
  If it requires a new crate, create the crate. Never paper over a structural problem with a
  local patch.
- "It works" is not an acceptance criterion. "It works and is architecturally correct" is.

The spirit: future maintainers must be able to read any line of this codebase and see a
deliberate, reasoned decision — not an expedient shortcut that was never revisited.
