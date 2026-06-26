# Repository Guidance

This is the authoritative contributor guide for the repository — for agents and humans equally. It covers worktree discipline, PR workflow, test scope, planning rules, and the doctrine corollaries that govern every implementation decision. Everything here is enforced, not suggested. `CLAUDE.md` defers to this file.

## Agent workflow

- All implementation work must happen in a git worktree owned by the agent doing the work.
- Each agent is responsible for its own branch/worktree lifecycle. Do not edit from the shared root checkout for feature, fix, or refactor work.
- When the work is complete, open a pull request before reporting completion. The PR description must include:
  - a short TLDR summary of what changed;
  - a detailed overview of the work performed;
  - any subjective decisions made, including tradeoffs or assumptions.
- Do not open completed work as a draft pull request. If the work is ready and validated, open it as ready for review. Use draft PRs only when explicitly asked or when the work is intentionally incomplete.
- After opening the PR, clean up the agent-owned worktree before completing the task.

## User product corrections

When the user gives any correction or instruction about how the NMP product
should work, treat it as a possible product-authority update, not just an
implementation request. Before making code changes, delegate a separate agent
to research whether the correction should be represented in product specs,
doctrine, canonical docs, GitHub Issues, ADRs under
`docs/decisions/`, or another existing authoritative document.

The delegated research must report where the instruction already lives, where
it should live if it is missing, and whether the code change should be blocked
until the documentation/source-of-truth update is made. If documentation needs
to change, make that update in the same PR as the implementation unless the
user explicitly scopes the work to docs only.

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

## Planning discipline — GitHub queue, temporal files, no duplicate plans

This repository has exactly **one canonical tactical queue: GitHub Issues**. Every active violation, pending user decision, queued feature, post-v1 item, staged fix, and follow-up belongs in an open GitHub issue with labels. Scattered notes, ad-hoc `TODO.md` / `NOTES.md` / `ROADMAP.md` / `PLAN-foo.md` files, parallel planning docs, and inline `// TODO:` annotations used as a substitute for tracking are **forbidden**.

Plans are not durable understanding and must not survive as reference documentation after they have been implemented, executed, or invalidated. When a plan completes, close its issue or collapse it to the smallest live follow-up. Any lasting knowledge learned from the work belongs in durable documentation instead: `docs/aim.md` for the north star, `docs/product-spec/` for product doctrine, `docs/architecture/` and `docs/design/` for architecture, `docs/decisions/` for ADRs, the builder guide for maintained how-to material, and `wiki/` for source-backed synthesis.

| Surface | Role | Update cadence |
|---|---|---|
| GitHub Issues | The single tactical and release tracker — active violations, pending user decisions, milestone/release state, ordered v1 feature work, post-v1 list, staged fixes, and follow-ups. | Every PR that touches a queued item; update/close the issue and move lasting conclusions to durable docs. |

Rules — enforced strictly:

- **Do not create new top-level planning files.** No `PLAN.md`, `TODO.md`, `ROADMAP.md`, `NEXT.md`, `STATUS.md`, `plan.md`, a `docs/plan/` tree, or per-feature plan files at the repo root or directly under `docs/`. New tactical detail belongs in a GitHub issue. Short-lived migration plans may live in `docs/architecture-audit/<plan>.md` only when they gate a specific active milestone or violation and link back to the owning issue. Durable decisions belong in `docs/decisions/00NN-*.md` ADRs, not in plans. Never create a parallel overview.
- **Do not duplicate state across files.** A violation or feature tracked in GitHub Issues is not also restated as a queue row in a parallel planning file; the issue remains the queue authority.
- **Issue labels define priority order.** Work in this order: `priority:p0`, `priority:p1`, `priority:p2`, `priority:p3`, then `priority:p4`. Within a priority bucket, prefer `category:violation` before `category:feature`, then `category:test`, then `category:decision` unless the user explicitly directs otherwise. Use `phase:*`, `area:*`, `doctrine:*`, and `status:*` labels to filter scope.
- **GitHub search is the backlog view.** Use `gh issue list --state open --label priority:p0 --limit 50`, then repeat for `priority:p1` through `priority:p4`.
- **Plan files have authority over scattered notes only for active release state.** A `// TODO:` comment in code is not a plan. If it represents work to be done, it belongs in a GitHub issue. If it represents a known limitation or durable decision, it belongs in an ADR, doctrine clarification, architecture/design doc, builder-guide page, or wiki article as appropriate.
- **Single source of truth per fact** — this is D4 (single writer per fact) applied to docs. Tactical and release-plan state belong in GitHub Issues. Durable facts belong in durable docs or code. Audit reports may reference the canonical issue; they must not become parallel authorities.
- **Correct docs in place; do not layer superseding corrections.** When any repository documentation is wrong, edit the authoritative document so it says the correct thing. This applies to ADRs, product specs, architecture/design docs, builder-guide pages, wiki articles, temporal plan docs, and every other doc surface. If a doc has incorrect current guidance, edit that file directly; do not add a "superseding ADR", appendix, wiki page, or "revision" section whose job is to contradict it while leaving the wrong text intact. Historical notes are allowed only when they describe past events without preserving incorrect current guidance. The current rule must be readable in the document that owns it, not in a correction elsewhere.
- **Never commit code reviews.** AI code review output, direction reviews, codex review dumps, and post-merge review notes must not be committed to the repository. If a review surfaces an actionable finding, promote it into a GitHub issue (active) or a durable doc (lasting understanding) — then discard the review itself.
- **Edit existing issues; do not append parallel ones.** If queued work changes, update the existing issue body/labels/title in place. Append-only history files (`docs/perf/pending-user-decisions.md`) are explicitly historical — do not invent new ones.
- **Retire executed plans.** A plan that has been implemented is no longer a source of truth. Close the issue, delete the temporal detail, or replace it with the smallest remaining live follow-up issue. Preserve durable lessons in the durable doc that owns that concept.
- **When in doubt, fewer planning files.** The cost of a duplicate plan is divergence: within one sprint two sources will describe different states of the same world, and neither will be trustworthy. If a new planning file feels necessary, justify why it cannot be a GitHub issue plus a durable doc.

This discipline is non-negotiable. A PR that introduces a duplicate planning file, a scattered todo list, or a parallel roadmap is rejected and the entries are folded back into GitHub Issues or durable docs.

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

## Architecture: Rust owns all domain logic; native is rendering + capabilities only

See `docs/aim.md` §2 commandment #4 for the canonical rule. Summary: native has exactly **three** responsibilities (render, execute capabilities, hold ephemeral presentation state). The discriminating test is *"would a second platform have to reimplement this to stay correct?"* — yes → Rust (domain); only-how-it-looks → shell (presentation). Do not let domain logic leak into the shell, and do not push pure presentation concerns into the core.

## Effects, replay, and snapshot discipline

- Every external effect is represented as typed data crossing the Rust/native boundary: Rust requests a capability, native reports a raw result, Rust decides the next state.
- New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams. Reducers must remain replayable from message history.
- Debug/history surfaces must use log-safe action tags and correlation ids; never record secrets, raw nsecs, plaintext DMs, or bearer tokens.
- Keep `FullState`/full snapshot as the correctness path. Add granular `ViewBatch` or other delta variants only when profiling proves the snapshot path is the bottleneck and the delta is lossless.

## What belongs in NMP crates vs. app-specific Rust crates

**NMP crates (`crates/`)** provide reusable Nostr infrastructure. A feature belongs in an NMP crate when it is a general building block that any Nostr application — or a meaningful subset of Nostr applications — could use directly. Examples: relay management, signing, NIP implementations, event storage, timeline projection, encrypted messaging, identity. The test: *"would this crate be useful to a completely different Nostr app?"* If yes, it is an NMP crate.

**App Rust crates (`apps/<app>/`)** hold the Rust side of features that are specific to that application's domain and would not generalize to other Nostr apps. Example: a podcast app's audio playback engine, chapter parsing, or feed-subscription state. These belong in the app's own Rust crates, not in NMP. NMP does not accumulate app-specific logic.

The line is not protocol vs. product — a product-level feature (e.g., NIP-29 group chat, Marmot MLS encrypted groups) belongs in an NMP crate if other Nostr apps would use it. The line is **generic Nostr building block vs. this app's proprietary domain**.

A request from one app is evidence, not permission to specialize the framework.
Do not add app-named helpers, bespoke publish/read commands, hard-coded product
defaults, operator policy, compatibility shims, or "quick" shared-crate
workarounds just because a consuming app needs them. First ask whether the
missing piece is a reusable Nostr mechanism. If yes, add the generic mechanism
once. If no, implement it in the app's Rust core. If the answer is unclear,
update the owning issue or ADR before writing code.

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
  in production code. A staged fix is allowed *only* when a GitHub issue labeled
  `status:staged` documents every stage with a completion deadline. An unplanned, undocumented "temporary"
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
