# Opus Direction Review #10 — 2026-05-20

Review #9 (~2 hours ago) ended with an explicit recommendation: **moratorium on
reviews until code is deleted.** I am being asked for #10 anyway. I acknowledge
that. The honest move is to keep this short, verify the facts, and not pad.

## 1. Is this review justified?

Mostly no. Review #9 was right. Generating a 10th analysis document in one day
while the strategic backlog is untouched *is* the execution pathology, not a
remedy for it. Reviews 6–9 already said the same thing in escalating tones; a
4th restatement adds zero new information to a reader who has #9.

There is exactly **one** sliver of value in doing #10, and it is not the
analysis — it is sections 2 and 4. Section 2 is a factual ledger that proves,
with file:line evidence, that the 30-day clock from #9 is real and unstarted.
That ledger is cheap accountability. Section 4 hands the next agent a single
concrete 2-hour task so the response to this review is a *commit*, not another
`.md`.

So the verdict: **this is the last review.** Reviews #11+ are forbidden until at
least one item in Section 2 flips from "unaddressed" to "deleted/wired." If the
next heartbeat wants to spend a turn on NMP direction, it spends it doing
Section 4, not writing prose. Anything else is compounding the problem.

## 2. Accountability audit

All five verified by reading, not assuming.

**(1) 4 bespoke FFI verbs — UNADDRESSED.**
`crates/nmp-core/src/ffi/identity.rs` still exports `nmp_app_publish_note`
(L109), `nmp_app_react` (L347), `nmp_app_follow` (L370), `nmp_app_unfollow`
(L384). Zero progress. The 30-day delete-or-decide clock from #9 has not
started. **Recommendation still valid — and now overdue by 2 hours of the 30
days.**

**(2) `ActionRegistry::reduce` — STILL DEAD, and the comment now admits it.**
`crates/nmp-core/src/kernel/action_registry.rs:232` carries a fresh doc comment:
"No code drives `reduce` today (the publish engine drives transitions
in-process), so this is `#[allow(dead_code)]`." The grep hits in
`kernel_reducer.rs` are a *different* `reduce` (the `KernelReducer`, the real
one). So the registry's `reduce` is confirmed dead. Progress: a more honest
comment — which is the same pattern as Section 3 (label the lie, keep the
corpse). **Recommendation valid, but sharpen it: don't "wire" `reduce`. Delete
it.** The publish engine already drives transitions in-process; `reduce` is a
speculative hook for an "M6 ledger" that does not exist. Dead code defending a
hypothetical milestone is debt. Cut it.

**(3) No ViewRegistry — CONFIRMED, and worse than #9 thought.**
`grep ViewRegistry` returns nothing. The `ViewModule` trait exists
(`substrate/view.rs:89`) with 18 `impl`s, none driven by any registry. New
finding: `actor/commands/event_observer.rs:10–14` now tells new code to
**register a `KernelEventObserver` instead of implementing `ViewModule`.** The
substrate trait is being abandoned in place — 18 impls becoming fossils while a
parallel mechanism quietly replaces it. **Recommendation changes:** stop saying
"build a ViewRegistry." Decide. Either `ViewModule` is the substrate contract
(then wire a registry) or `KernelEventObserver` is (then delete `ViewModule` and
its 18 impls). Two mechanisms for one job is the substrate thesis failing twice.

**(4) `nmp-highlighter-core` — CONFIRMED placeholder, 49 lines total.**
`lib.rs` is 25 lines, `placeholders.rs` is 24 — 49 together, matching #9. It
re-exports `nmp_nip29::GroupId` to "prove the crate boundary holds" and ships
module placeholders. The 39K-line Swift app still has no Rust core doing
protocol work. Zero progress. **Recommendation valid.**

**(5) `KernelUpdate` has no generic `projections` slot — CONFIRMED.**
The grep "hit" in `kernel/types.rs:535` is a false positive: it matched the word
"projections" inside the comment `// ── T66a identity / publish / relay-edit
projections ──`. The actual snapshot struct below it is ~25 hand-named social
fields (`accounts`, `publish_queue`, `bunker_handshake`, `wallet_status`, …) —
the monolithic struct #9 flagged. `KernelUpdate` itself (`app.rs:39`) is still a
fixed enum. No generic slot. Zero progress. **Recommendation valid.**

**Audit summary: 0 of 5 addressed. 5/5 recommendations still valid; (2) and (3)
should be re-pointed at deletion/decision rather than "wiring."**

## 3. What the 3 changes that DID land tell you

The three shipped fixes — `execute_action`'s `_ => Err(...)` arm, the removed
`expect()` in `fanout::launch()`, the removed redundant `.unwrap()` in
`tick()` — share one shape: **they make existing code more honest or less
fragile. None of them change the architecture.** Not one byte of FFI surface
removed, not one registry wired, not one `ViewModule` either driven or deleted.

This is a precise, diagnostic pattern: **the project does correctness work
reflexively and structural work never.** Correctness fixes are safe — they are
local, they pass the existing tests with a one-line assertion change, they
require no decision, and they feel productive. Deletion is none of those: it
forces a *decision* (which API is the real one?), it breaks callers, it admits
prior work was wrong.

The strategic bet — "one Rust substrate, generic dispatch, thin app shells" — is
not blocked by bugs. It is blocked by an unwillingness to choose. `dispatch_action`
vs. 4 bespoke verbs: both still shipped, because keeping both requires no
decision. `ViewModule` vs. `KernelEventObserver`: both still shipped. The
velocity on the strategic bet is therefore not "slow" — it is **zero, and
structurally so.** The team will fix lies all day because fixing a lie is easier
than killing a feature. Until a review forces a *deletion* with a *deadline that
is enforced*, #11 will report 0/5 again.

The `execute_action` fix is the tell. It made the substrate path *correctly
report* "no executor registered for namespace" — i.e. it correctly reports that
the substrate path does not work, while the 4 bespoke verbs that *do* work sit
untouched 200 lines away. The project hardened the road it isn't driving on.

## 4. One thing to do in the next 2 hours

**Route `nmp_app_publish_note` through `dispatch_action` and delete the bespoke
verb.** This is the one bespoke FFI verb that has a *real substrate target
today* — verified, not hypothetical:

- `PublishModule` is already registered in `default_registry()`
  (`action_registry.rs:277`) under namespace `"nmp.publish"`, with
  `Action = PublishAction` (`publish/action.rs:92–94`).
- The generic FFI entrypoint already exists:
  `nmp_app_dispatch_action(app, namespace, action_json)`
  (`ffi/action.rs:83`).
- Today `nmp_app_publish_note` (`ffi/identity.rs:109`) instead sends the
  *legacy* `ActorCommand::PublishNote` — a third, separate publish path that
  bypasses the registered module entirely.

The change:
1. In `ffi/identity.rs`, make `nmp_app_publish_note` build a
   `PublishAction::Publish { event, .. }` from `content`/`reply_to_id`, serialize
   it, and call `dispatch_action_json(app, "nmp.publish", &json)` — the same code
   path `nmp_app_dispatch_action` uses.
2. Once the Swift caller is confirmed unaffected (signature unchanged), this is a
   pure internal re-route; the bespoke *export* can then be deleted in the
   follow-up once iOS migrates to calling `dispatch_action` directly.
3. Run `cargo build -p nmp-core && cargo test -p nmp-core`. Green = done.

Why this and not deleting `ActionRegistry::reduce`: deleting `reduce` is correct
(item 2) but it is a substrate *retreat* — trimming dead machinery. This task is
a substrate *advance*: it drives the registered `PublishModule` end-to-end for
the first time from real FFI traffic, which is exactly the road this morning's
`execute_action` fix hardened but left undriven. It is the only 2-hour task that
both subtracts toward item (1)'s 30-day clock *and* proves the strategic bet
works for one real verb. `follow`/`unfollow` cannot be done in 2 hours honestly —
there is no `FollowModule` registered at all, so they would need a new module
written first; do not attempt them in this window.

If the next heartbeat does only this and writes no prose, review #10 will have
paid for itself. Do not write review #11. Do Section 4.
