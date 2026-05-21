# Opus Direction Review #35 — The four "recent PRs" are not on master

Date: 2026-05-21
Reviewer: Opus (architectural direction audit)
Scope: NMP kernel + `dispatch_action` seam + Chirp FFI. Triggered by a request
to advise on async-capability, NIP-29, and the smallest user-visible win.

---

## TL;DR — read this first

**The premise of this review was wrong, and that is the finding.**

The task brief described "what shipped in the last 5 cycles": PRs #99–#105. I went
to read that shipped code. Four of the five do not exist on `master`. They are
stranded in orphan `worktree-agent-*` branches that were never merged:

| PR | What it claims | Commit | Orphan branch | On master? |
|----|----------------|--------|---------------|------------|
| #99  | Wire 15 NIP-29 ActionModules | `71c558a9` | (merged) | **YES** |
| #100 | `HttpCapability` + iOS URLSession | `aae33956` | `worktree-agent-a1f4846403928d82d` | **NO** |
| #101 | Delete `last_action_result` scalar | `180cd335` | `worktree-agent-adb9c446941f71728` | **NO** |
| #103 | NIP-29 admin namespaces → snake_case | `bbc176c3` | `worktree-agent-aae4bc11908508756` | **NO** |
| #104 | ADR-0024 async-capability protocol | `1bc0cd6c` | `worktree-agent-a9f39b544a9c3096a` | **NO** |
| #105 | `actor_queue_depth` straddle counter | `b6825f4b` | `worktree-agent-a3cbf6987d10403b4` | **NO** |

`master` HEAD is `faaa9305`. Only PR #99 landed. The other five "recent cycles"
of work produced commits that compile in a worktree, pass CI in a worktree, and
have never been seen by `master`.

The prior reviews coined "shipped-but-inert": code that is green and merged but
unreachable by users. **This is worse than shipped-but-inert. It is not shipped
at all.** Green CI on an unmerged worktree branch is not a landed PR. The
changelog/heartbeat narrative recorded these as done; they are not.

This violates the project's own [agent push protocol] memory: *"heartbeat sweeps
for orphan worktree-agent-* branches with commits not on master and cherry-picks
them."* That sweep has not been running, or has been failing silently, for at
least 5 cycles.

The single highest-leverage action for this project is not async-capability,
not NIP-29 UI, not picking a user win. **It is a merge sweep + a fix to the
heartbeat sweep that was supposed to prevent this.** Everything else in this
report is downstream of that.

---

## 1. The headline: a broken merge pipe, not a broken architecture

The architecture is fine. The `dispatch_action` seam works. The single-actor
boundary is clean. The kernel is genuinely good at what prior reviews credited
it for. None of that is the problem.

The problem is delivery. Six cycles of agent work, one merge. The orchestration
is spawning parallel agents into isolated worktrees, the agents are doing real
work, CI is going green — and then the work evaporates because nothing lands it
on `master`. The heartbeat's job (per the project's own memory) is to sweep
those orphans. It is not doing it.

This produces a specific and dangerous failure mode: **the user makes direction
decisions against a fiction.** Direction review #34 in the brief discusses
"PR #103 made shipped-but-inert WORSE" and "fix NIP-29 namespaces NOW" as if
#103 happened. It didn't. The CamelCase namespace bug #103 was supposed to fix
is *still live on master today*. Review #33 "DECIDED: LNURL HTTP = Option A" and
review #34 treats ADR-0024 as an artifact you can build against. ADR-0024 does
not exist on master.

Every direction review since ~#30 has been partially reasoning about code that
isn't there.

### Concrete live consequence: the NIP-29 namespace bug is still in master

`crates/nmp-nip29/src/action/admin.rs:36` on master today:

```rust
const NAMESPACE: &'static str = concat!("nip29.", stringify!($Module));
```

`stringify!($Module)` yields the Rust identifier — `CreateGroupAction`,
`EditMetadataAction`, etc. So the seven admin namespaces registered on master
right now are `nip29.CreateGroupAction`, `nip29.EditMetadataAction`,
`nip29.PutUserAction`, … — CamelCase, mixed with the eight snake_case
membership/content namespaces (`nip29.join_request`, `nip29.post_chat_message`).

This is a wire-format inconsistency. The fix (`bbc176c3`) converts them to
`nip29.create_group` etc. It exists, it is correct, and it is stranded in
`worktree-agent-aae4bc11908508756`. Until that branch lands, **any Chirp UI
built against a NIP-29 admin action on master will dispatch a CamelCase
namespace string** — and when #103 finally lands it becomes a breaking change
*with* a caller, which is exactly the situation review #34 thought had been
avoided.

---

## 2. Stop / Start / Continue

### STOP

- **Stop spawning parallel agents while the merge pipe is broken.** Ten agents
  in ten worktrees producing ten green-CI branches that never merge is *worse*
  than one agent that merges, because it manufactures the illusion of progress.
  The "always keep exactly 2 agents in parallel" memory is actively harmful
  right now — it doubles the orphan rate. Drop to one agent (or zero) until a
  PR can be shown to land on `master` reliably.
- **Stop treating worktree CI green as "shipped."** The honest changelog verb
  for an unmerged worktree branch is not "feat" and not even "scaffold" — it is
  "drafted, unmerged." A PR is shipped when `git merge-base --is-ancestor
  <sha> master` returns true. Nothing else counts.
- **Stop writing direction reviews against the heartbeat narrative.** Reviews
  #30–#34 reasoned about #100/#103/#104/#105 as landed. Future reviews must
  start from `git log master`, not from the orchestration log.

### START

- **Start a one-shot merge sweep** of the five stranded branches (Section 4).
- **Start verifying the heartbeat sweep.** The memory says it cherry-picks
  orphan `worktree-agent-*` branches. Find out why it hasn't for 5 cycles. The
  most likely cause: branches were pushed to `origin` (they are all visible as
  `remotes/origin/worktree-agent-*`) but the sweep looks at local refs, or the
  sweep ran in a tree that was dirty and silently no-op'd, or the sweep treats
  "branch exists" as "branch merged." Whatever it is, it is the actual bug.
- **Start gating each cycle on a merge.** A cycle that ends with commits only
  on a worktree branch is a failed cycle. Make that explicit.

### CONTINUE

- The single-actor kernel boundary. It is clean and it is the project's real
  asset.
- The `dispatch_action` seam for the 4 live namespaces (`nmp.publish`,
  `chirp.react`, `chirp.follow`, `chirp.unfollow`). This is the one path that
  works end-to-end and reaches a user. It is the model; do not regress it.
- Doctrine-lint (D0/D6/D7/D8 — 0 findings). Keep it. But note doctrine-lint
  passing on a worktree branch tells you nothing about `master`.

---

## 3. Answers to the six questions

### Q3 — Architecture debt vs. new features: which to freeze?

Neither, yet. **Freeze new infrastructure AND freeze new features. Spend the
next cycle entirely on the merge sweep and the heartbeat fix.** The 4 live
Swift dispatches "haven't grown" across 35 reviews not because the architecture
is wrong but because the work that would grow them keeps not landing. You cannot
diagnose "is something fundamental missing" while half the recent work is
invisible to `master`. Land the backlog first, *then* re-measure.

### Q4 — The async-capability question (ADR-0024)

Moot as posed. You cannot "implement ADR-0024 next" because ADR-0024 is not on
master — it is in `worktree-agent-a9f39b544a9c3096a`. The next step is to
**land `1bc0cd6c`** (a docs-only commit, zero-risk, no code) so the decision
record exists. Only then is "implement it" a coherent next step.

When it *is* time to implement: yes, the async pattern is the right call, and
it should come before any further ZapModule work. A capability that blocks the
single actor thread for a multi-second LNURL round-trip is a D8/D3 violation by
construction. But it is not the highest-leverage thing this week — landing the
backlog is.

### Q5 — The NIP-29 question: 15 executors, 0 callers

Third option, and the discovery sharpens it: **do not delete, do not build UI —
land `bbc176c3` (snake_case fix) first.** Right now master has 15 NIP-29
namespaces, 7 of them with the wrong (CamelCase) wire string. Building a Chirp
NIP-29 screen against master today would hard-code CamelCase namespace strings
and then break when #103 lands.

After #103 is on master, the answer is: build exactly **one** NIP-29 screen —
group chat read + `nip29.post_chat_message` dispatch — and keep the other 14
executors. One real caller proves the seam for the protocol-crate case; the
other 14 are cheap (macro-generated, tested) and become live the moment a
screen needs them. Deleting them buys nothing and re-deriving them later costs
real work. The 15-executors-0-callers state is only a problem because it was
*reported as shipped value*. It is fine as a staged capability — provided the
changelog stops calling it "feat."

### Q2 — Smallest user-visible win

The honest answer changed once I saw master. The smallest user-visible win is
not a new feature — it is **landing PR #100 (`aae33956`, HttpCapability)**.
That branch carries the iOS `HttpCapability.swift` URLSession implementation and
the `nmp-core` `substrate/http.rs` seam. With it on master, the kernel can make
an HTTP call at all — the precondition for *every* user-facing feature that
isn't pure Nostr-relay traffic (zaps, LNURL, Blossom, link previews, NIP-05
verification). It is one commit, already written, already CI-green in its
worktree. Today a user gets zero HTTP-backed features; after the merge the
platform can grow them. That is the smallest delta that unblocks real user
value, and it costs a merge, not a sprint.

If the question insists on a *feature* a user can tap: the smallest is a
NIP-29 group-chat **read** view (no posting) — `nmp_app_open_*` style
subscription to a group's `h`-tagged events rendered in a list. But that should
come after the merge sweep, not before.

### Q1 — Stop/start/continue

Covered in Section 2.

### Q6 — What NMP should NOT do

- **Should not run a parallel-agent fan-out while the merge pipe leaks.**
  Parallelism multiplies orphan branches. Until merges are reliable, parallel
  agents are a liability, not throughput.
- **Should not add a 16th NIP-29 action, a 2nd CapabilityModule, or any new
  registered-but-uncalled surface** until an existing one has a Swift caller.
  The project has a structural habit of building the registration side of a
  seam and calling it done. Every new namespace without a caller widens the gap
  between "what CI says" and "what a user can do."
- **Should not write ADRs for protocols it hasn't built a consumer for.**
  ADR-0024 is sound, but it is the 24th decision record on a project whose
  users can do 4 things. Decisions are cheap; merged consumers are the
  scarce resource.
- **Should not trust the orchestration log / changelog as the source of truth
  for project state.** `git log master` is the only source of truth. The
  README's milestone table and the heartbeat narrative have drifted from it.

---

## 4. The merge sweep — concrete plan

Five branches to land. They have file overlaps, so order matters.

| Order | PR | Branch | Risk | Notes |
|-------|----|--------|----|-------|
| 1 | #104 | `worktree-agent-a9f39b544a9c3096a` | none | docs-only (`0024-...md`). Land first, free. |
| 2 | #103 | `worktree-agent-aae4bc11908508756` | low | touches `nip29/admin.rs` + `chirp/ffi.rs`. Land before #100/#101 (both also touch those files). Fixes a live wire bug. |
| 3 | #105 | `worktree-agent-a3cbf6987d10403b4` | low | touches `kernel/action_registry.rs`, `kernel/update.rs`, `ffi/action.rs`, `publish/action.rs`. |
| 4 | #101 | `worktree-agent-adb9c446941f71728` | medium | largest diff (15 files); overlaps #105 on `action_registry.rs`/`update.rs`/`ffi/action.rs`/`publish/action.rs` and #103 on `nip29/admin.rs`. Rebase on top of #103+#105, resolve, re-run tests. |
| 5 | #100 | `worktree-agent-a1f4846403928d82d` | medium | overlaps #103/#101 on `chirp/ffi.rs` + `nip57/action.rs`. Rebase last. |

Each branch is only 1–2 commits ahead of master and 7 behind — they will need a
rebase regardless. For each: `git rebase master`, resolve, run the **scoped**
test suite for the touched crates (not full-workspace — see the agent
test-scoping memory), confirm doctrine-lint, then `git push origin HEAD:master`.

Do this serially, one branch at a time, verifying `git merge-base
--is-ancestor <sha> master` after each push. Do not fan this out to 5 agents —
the overlaps guarantee conflicts and you want a single serializing hand on it.

After the sweep: re-derive the "what reaches a user" inventory from `master`,
not from the changelog. Then review #36 can finally answer the real direction
questions against real code.

---

## 5. The single highest-leverage action

**Land the five stranded branches in the order above, then fix the heartbeat
orphan-sweep so this cannot recur.**

Not async-capability. Not NIP-29 UI. Not a new feature. The project's bottleneck
is not design and not even execution-of-code — it is *execution-of-merge*. Six
cycles of correct, CI-green work is sitting one `git push` away from users and
nobody pushed it. Fix the pipe before pouring anything else into it.

---

## Appendix — verification commands

```
# Confirm a "shipped" PR is actually on master:
git merge-base --is-ancestor <sha> master && echo ON || echo ORPHAN

# Results at review time (master = faaa9305):
#   71c558a9 (PR#99  nip29 wiring)        ON
#   aae33956 (PR#100 HttpCapability)      ORPHAN
#   180cd335 (PR#101 del last_action)     ORPHAN
#   bbc176c3 (PR#103 snake_case ns)       ORPHAN
#   1bc0cd6c (PR#104 ADR-0024)            ORPHAN
#   b6825f4b (PR#105 actor_queue_depth)   ORPHAN
```
