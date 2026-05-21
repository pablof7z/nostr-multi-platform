# Opus Direction Review #48

**Date:** 2026-05-21
**Scope:** Strategic review — what NMP is building, what's missing, what
should be cut. Grounded in direct reads of `docs/aim.md`,
`apps/chirp/nmp-app-chirp/src/ffi.rs`, `apps/fixture/nmp-app-fixture/src/`,
`crates/nmp-repl/`, `crates/nmp-marmot/`, and the live action registry.

---

## Q1 — The thesis test

**Proven end-to-end:** `chirp.react` / `chirp.follow` / `chirp.unfollow`. A
user taps the heart → `KernelBridge.swift:254` calls
`nmp_app_dispatch_action("chirp.react", …)` → host executor at
`apps/chirp/nmp-app-chirp/src/ffi.rs:472` builds `ActorCommand::React` →
actor signs, publishes, timeline re-renders from a snapshot projection.
Zero Swift business logic. This is the thesis as shipped.

**Most at risk:** NIP-17 DM receive. `DmInboxProjection` exists,
`nip17.dm_inbox` is registered (`ffi.rs:351`), `DmInboxStore.swift:143`
consumes it — but no integration test demonstrates a real kind:1059
flowing the full loop on a live relay (review #47 made this point too;
it has not been answered). If this stays "structurally wired,
empirically unverified" through M10, the most architecturally
interesting feature NMP ships will not have proof it works.

---

## Q2 — The second-app problem

A second-app developer reaches for `nmp init` (`nmp-cli`, 152 LOC), then
hits **580 LOC of hand-rolled C-ABI** in `apps/chirp/nmp-app-chirp/src/
ffi.rs`: a `wire_action!` macro, three `register_<nip>_actions` helpers,
an `unsafe impl Send for ChirpHandle` with three paragraphs of soundness
rationale (lines 99–131), bespoke `*const c_char` plumbing per entry
point. `apps/fixture/nmp-app-fixture/src/ffi.rs` shows the same shape —
hand-written, no `@generated` header. `nmp-codegen` exists but neither
app uses it as source of truth.

Day-1 blockers for the marketplace / RSS developer:

1. **FFI is hand-rolled.** They write 500+ LOC of `extern "C"` + their
   own `unsafe impl Send/Sync` rationale.
2. **iOS scaffold doesn't exist.** Chirp's `*Bridge.swift` files are
   bespoke per noun; `nmp init` doesn't emit them.
3. **Swift consumer knows the namespaces.** `KernelBridge.swift:727`
   enumerates `dmInbox` / `groupChat` cases — Chirp-shaped at the
   binding boundary.
4. **UniFFI is M14, post-everything-else** — until then no typed binding
   generator.

The architecture is reusable in principle (action registry + projection
keys are generic). In practice they fork Chirp's `ffi.rs` and rename
strings.

---

## Q3 — The missing protocol

**NIP-65 (relay-list metadata) on the user-facing publish side.** The
gossip-style filter routing is in `nmp-core/src/planner`, but Chirp has
no UI to set, edit, or publish a kind:10002 for the active user — every
NMP user's outbox reflects whatever default the framework guessed.

Why this over zaps / finishing NIP-17: NIP-65 is the doctrine keystone
(D3 — "outbox routing automatic"). Every other Nostr client a user
migrates to/from will respect their kind:10002; NMP users will be
silently mis-routed because they cannot deliberately curate it. This is
the gap between "the framework does the right thing" and "the user can
verify the framework did the right thing", and it is the cheaper, more
load-bearing v1 fix than zaps.

---

## Q4 — The single-actor tradeoff

The actor breaks at two predictable points:

(a) Synchronous network inside the actor loop (LNURL HTTP, NIP-44 ECDH
against a NIP-46 bunker for gift-wrap encryption).
(b) `nmp-marmot` MLS large-group key operations (3,990 LOC of state +
ops; a 100-member welcome rotation will not finish in one tick).

The escape hatch is there and named:
`apps/chirp/nmp-app-chirp/src/zap.rs:33` spawns a thread, does blocking
LNURL work, re-enters via `dispatch_action`. The seam that does **not**
exist: a typed `ActorWorker` capability for "do this async, then
re-dispatch", ADR-blocking the spawn-thread-and-pray pattern from
proliferating one-off in every NIP crate.

Not a ceiling if the project gets ahead of (b) before NIP-17-bunker and
Marmot-at-scale need it simultaneously. Build the worker capability
**before** the third call site appears.

---

## Q5 — The REPL/CLI value

**`nmp-repl` is not earning its weight as a Nostr CLI; it is a Marmot
testbed wearing one's badge.** 5,786 LOC, twelve of twenty-one command
files prefixed `mls_*` (create/invite/messages/send/status/init/accept/
fetch_kp/util). The non-MLS commands (`req`, `show`, `refresh`,
`create_account`) are reachable with `nak` in two flags. Marmot lives
here because no Marmot UI exists in Chirp beyond a message row.

To make it essential: either (a) rename it `nmp-marmot-repl` and stop
pretending it is a general developer tool, or (b) pivot the surface to
`dispatch_action` against an in-process actor with a mock relay + live
snapshot introspection — the only thing that would help the second-app
developer from Q2. `nmp-cli` (280 LOC) is right-sized but unused;
until `apps/fixture/ffi.rs` is regenerated from codegen with a zero
diff, the codegen path is unproven.

---

## Q6 — The v1 definition problem

**Must ship before calling it v1:**

1. **NIP-17 DM full round-trip verified** by an `nmp-testing`
   integration test (two accounts, mock relay, B's `DmInboxProjection`
   contains the decrypted message). Until then, the most interesting
   feature is unfalsifiable.
2. **A second app that is NOT Chirp**, exercising `dispatch_action` +
   `register_snapshot_projection` with at least one non-social kind.
   `fixture-todo-core` doesn't count — todos aren't Nostr.
3. **`nmp gen modules` as source of truth for `apps/chirp/ffi.rs`**, CI
   check on zero diff. Until then the FFI is hand-rolled forever.

**Cut or defer (currently in-flight):**

1. **NIP-57 LNURL executor (PR #164).** Review #47: two features in
   one branch, D8 threading risk, no UI. Cut the Rust half; ship the
   cosmetic fade-in. Zaps wait for NIP-60 wallet scoping.
2. **`nmp-nip77::RunSync` `ActionModule`** (`run_sync.rs:46`) — zero
   registration sites in `apps/` or `ios/Chirp/` per grep. M4 "LANDED"
   is true at trait-impl level, dormant in the live seam. Wire it as a
   Chirp setting or delete it before it joins the shipped-but-inert list.
3. **`ios/NmpHighlighter/` experimentation** (the dirty files in
   `git status`). Per the "NMP-only, 2 agents" doctrine these apps are
   deferred. Commit + freeze read-only, or delete — dirty files in a
   deferred app create review noise.

---

## Q7 — The observable gap

**`crates/nmp-marmot` (3,990 LOC) is the largest piece of scope creep in
the workspace.** Two `ActionModule`s, a 689-LOC `projection/ops.rs`,
exists under the ADR-0025 "Marmot MLS exception". Per
`docs/plan/marmot-mls.md` it is post-v1. Yet it is the second-largest
NIP-crate-shaped thing after `nmp-core` itself — larger than
`nmp-nip01` + `nmp-nip17` + `nmp-nip29` combined. Its only public
consumers are twelve `mls_*` REPL commands plus one Chirp message row.

This is the wrong abstraction level for v1: MLS-over-Nostr is a
research protocol with active spec churn. Shipping 3,990 LOC of Rust
under an evolving spec, while NIP-65 publish UI, NIP-77 `RunSync`, and
a second-app proof are unbuilt, is the exact "rot and create
maintenance debt without delivering user value" failure mode.

Right call: feature-gate `nmp-marmot` behind `--features marmot` off by
default, freeze its public surface, stop optimizing for it until v1
ships. The `nmp-repl` MLS commands move into a separate
`nmp-marmot-repl` crate so they stop dragging the Nostr CLI's identity.

---

*Outside view: NMP is closer to v1 than the milestone ladder suggests
on Chirp social features, and further from v1 than the ladder suggests
on "another developer could build a Nostr app with this". The gap is
empirical proof and reusability, not architecture.*
