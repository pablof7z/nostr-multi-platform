# Repository Guidance

Authoritative contributor guide — agents and humans equally. Everything here is enforced. `CLAUDE.md` defers to this file.

## Agent workflow

- All implementation work happens in an agent-owned git worktree. Never edit feature/fix/refactor work from the shared root checkout.
- Open a PR before reporting completion. Description must include: what changed (TLDR), detailed overview, any tradeoffs or assumptions.
- Open as ready for review, not draft, unless the work is intentionally incomplete.
- Clean up the worktree after the PR is open.

## Test scope

- Scope `cargo test` to the crates you touched. `cargo test --workspace` is for CI and the merging supervisor only.
- Always run `cargo test -p nmp-testing --test doctrine_lint_smoke` — D-rule gates don't trip in plain per-crate runs.
- Run `cargo build --workspace` (compile-only) if you renamed a public symbol, moved a module, or changed `Cargo.toml` deps.

## Planning discipline

**One canonical tactical queue: GitHub Issues.** No `PLAN.md`, `TODO.md`, `ROADMAP.md`, scattered notes, or `// TODO:` substitutes.

- **Priority order:** `priority:p0` → `p4`; within a bucket: `violation` → `feature` → `test` → `decision`.
- **Durable knowledge goes in durable docs:** `docs/aim.md`, `docs/product-spec/`, `docs/decisions/`, `docs/architecture/`, `wiki/`. Close issues when done; move lessons to the doc that owns the concept.
- **Correct docs in place** — never add a superseding ADR to contradict a wrong doc; edit it directly.
- **Never commit code reviews** — promote findings to issues or durable docs; discard the review.
- **Edit existing issues in place** — don't open a parallel issue for the same work.

## File size

Source files: 300 LOC soft limit, 500 hard. Split by cohesive ownership. Generated/vendored/lockfiles exempt. Docs (`.md`, ADRs, wiki) have no limit.

## TEA organization: co-locate by owner, not role

- No top-level `model/`, `update/`, `view/`, `state/`, `actions/` buckets — one cohesive module per feature/page/protocol.
- When an owner hits the LOC limit, split by concrete sub-type under the same namespace, not by recreating Model/Update/View layers.
- Keep the top-level actor/router flat until a module has genuinely self-contained state. Compose nested messages deliberately.

## Architecture: Rust owns domain logic; native is rendering + capabilities only

Native has exactly three responsibilities: render, execute capabilities, hold ephemeral presentation state. Test: *"would a second platform have to reimplement this to stay correct?"* — yes → Rust; only-how-it-looks → shell. See `docs/aim.md` §2.

## Effects, replay, and snapshot discipline

- External effects are typed data crossing the Rust/native boundary: Rust requests, native reports a raw result, Rust decides next state.
- Nondeterministic inputs (time, randomness, network, OS callbacks) enter as explicit actions/events or injected seams. Reducers must be replayable.
- Log surfaces: log-safe action tags + correlation ids only. Never record secrets, nsecs, plaintext DMs, or bearer tokens.
- `FullState` is the correctness path. Add delta variants only when profiling proves the snapshot is the bottleneck.

## NMP crates vs. app-specific crates

`crates/` = reusable Nostr infrastructure any Nostr app could use. Test: *"useful to a completely different Nostr app?"* → NMP crate. `apps/<app>/` = app-proprietary domain logic that wouldn't generalize.

The line is not protocol vs. product (NIP-29 group chat belongs in NMP). It's generic Nostr mechanism vs. this app's proprietary domain. One app's need is evidence, not permission to specialize the framework.

## No polling

No `sleep` + check loops, no `try_recv` + `sleep` spins, no `Timer.scheduledTimer` querying state. Use `recv()` / `recv_timeout()` in Rust, push-driven ViewBatch snapshots on iOS, OS callbacks for system events. Rationale: D8 + `docs/builder-guide/06-reactivity-contract.md`.

## Zero-tolerance on hacks and debt

- No temporary workarounds. Staged fixes require a `status:staged` GitHub issue with completion deadline.
- One canonical path per concept — if two exist, delete one before merging.
- Do the architecturally correct thing, even if it means touching 10 files or creating a new crate.
- "It works and is architecturally correct" is the acceptance criterion, not just "it works."
