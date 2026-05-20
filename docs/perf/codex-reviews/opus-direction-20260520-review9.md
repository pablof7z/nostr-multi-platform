# NMP Direction Review #9 — Stop Reviewing. Start Deleting.

**Date:** 2026-05-20
**Reviewer:** Opus (principal-engineer / product-strategy direction review)
**Predecessors:** reviews #1–#8, all dated 2026-05-20. Read #6 and #7 first.
**Mandate:** be blunt. Prior reviews were diplomatic. This one is not.

---

## Headline

`ios/NmpHighlighter` is **39,097 lines of Swift**. `crates/nmp-highlighter-core`
is **49 lines** — `lib.rs` plus a `placeholders.rs`. That is the entire NMP
"highlighter app." The thin-shell thesis — "write protocol logic once in Rust,
the native app is a <300-line shell" — has not slipped. **It has inverted.** The
ratio of app logic to shared core, for the project's largest reference app, is
797:1 in the wrong direction.

A second number, equally damning. This is the **ninth direction review in one
day**. Reviews #6 and #7 named the exact fix — wire `ActionRegistry::reduce`,
build a `ViewRegistry`, add one generic `projections` slot to `KernelUpdate`.
Review #7 measured what shipped in the 2.5 hours after #6 landed: four PRs of
Chirp polish and zero of the named fix. It is now hours later. I re-ran the
greps. `grep -rn ViewRegistry crates --include='*.rs'` is still **zero**.
`ActionRegistry::reduce` is still `#[allow(dead_code)]`. `execute_action` still
has its `_ => Ok(())` arm at `ffi/action.rs:175`.

The project does not have a design problem. The design is in eight memos. The
project has an **execution-discipline problem**: it generates reviews faster
than it acts on them, and every working hour routes to the social client instead
of the load-bearing seam. Review #9 will therefore not propose a ninth design.
It proposes a forcing function that makes the design impossible to keep ignoring.

### Correcting the brief's own numbers

The task brief mis-states two figures, and that is itself signal — if the
project cannot keep its own headline metrics straight, the reviews are arguing
over a phantom.

- Brief says "Chirp is 39K Swift LoC." **Chirp is 8,282.** The 39K app is
  `NmpHighlighter`. Chirp is actually the *best-behaved* iOS app in the tree.
- Brief says "76-field `KernelUpdate`." The struct at `kernel/types.rs:501` has
  **30 fields** (3 cfg-gated). 76 is the count with nested struct fields
  flattened. Use 30. The god-struct critique survives at 30 — but precision
  matters when you are asking someone to make hard cuts.
- iOS total across all three apps (`Chirp` 8.3K + `NmpHighlighter` 39.1K +
  `NmpPodcast` 19.0K) is **66,409 Swift LoC**. The brief's MEMORY.md says
  podcast was "skipped." `NmpPodcast` is 19K lines of not-skipped.

---

## What the prior eight reviews said, and what actually changed

| Review | Named finding | Shipped? |
|---|---|---|
| #6 | `ActionRegistry::reduce` dead; no `ViewRegistry`; `KernelUpdate` has no generic slot | **No.** Still dead, still absent, still no slot. |
| #7 | "Good design followed by empty registry theater" — pattern, not accident | **Confirmed again today.** Pattern intact. |
| #8 | CRITICAL: publish `InFlight` pins forever, no timeout sweep | **Yes.** `sweep_inflight_timeouts` landed. The one tactical bug got fixed because it was tactical. |

Read that table honestly. The **one** finding that shipped was the one that was
small, local, mechanical, and didn't threaten the architecture. Every finding
that required *retiring* something — a dead trait, a shadow registry, a god
struct — is exactly as unaddressed as the day it was written. That is not a
coincidence. The team can fix bugs. It cannot, so far, **delete its own
scaffolding**. Reviews #6–#8 kept saying "build the registry." They were
correct and they did not work. Review #9 changes the verb from *build* to
*delete*, because addition is what got the project into a 49-vs-39,097 hole.

---

## 1. The #1 strategic bet to validate or abandon in the next 30 days

**The bet: "a non-social app can be built on NMP with the core doing the
protocol work." Settle it with a destructive experiment, not a constructive
one.**

Every prior review proposed *building* the proof (the bookmark app, the second
app, fixture-todo). None got built as a real app, because building competes
with Chirp for hours and loses. So invert it. The 30-day experiment is a
**deletion deadline**, and it is binary:

> **By 2026-06-20, `nmp_app_publish_note`, `nmp_app_react`, `nmp_app_follow`,
> and `nmp_app_unfollow` are deleted from the FFI surface. iOS calls
> `nmp_app_dispatch_action` for all four, or the build is red.**

Be honest about the shape of this work: only `nmp.publish` has an executor in
`execute_action` today (`ffi/action.rs:154`). `react`/`follow`/`unfollow` each
need a *new* executor branch built — that is most of the 30 days, not a free
re-route. The recommended cadence: migrate `publish_note` in week 1 (the 2-day
change in §5 — it rides the existing executor), and if that recipe holds, build
the three new executors and retire the other verbs across the remaining three
weeks. All four are confirmed live Chirp callers (`KernelBridge.swift:210`,
`:213`, `:229`, `:235`), so each deletion genuinely forces the migration. If
you cannot retire **four** verbs into `dispatch_action` in 30 days, the
`dispatch_action` thesis is empirically dead and you should delete
`dispatch_action` instead and admit NMP is a verb-per-noun FFI. Either outcome
is fine. **The current state — both systems alive, neither winning — is the
only unacceptable one.** `dispatch_action` arriving as FFI symbol #48
(confirmed: 49 symbols total, listed below) while 30+ verbs persist is not a
migration. It is a second system wearing a migration's clothes.

Pass criterion: `grep -c "fn nmp_app_publish_note" crates/nmp-core/src/ffi` → 0,
and Chirp ships a build that posts a note through `dispatch_action`. Fail
criterion: the deadline passes with the verb still exported. There is no third
state. **Pick one system in 30 days.**

Why this and not "build the second app": building a second app is the *right*
strategic move but it is a 2–3 month effort and it has been proposed in four
reviews without happening. A deletion deadline is enforceable, is measurable by
a grep, costs zero new code, and produces an unambiguous verdict. It is the
experiment that *settles* the bet rather than deferring it a ninth time.

---

## 2. What NMP should support that it currently doesn't

The FFI thesis fails at scale for one mechanical reason: **every type that
crosses the C ABI as JSON is hand-decoded twice** — once in Rust, once in Swift —
and the Swift half is uncodegenerated. That is why `NmpHighlighter` is 39K lines.
Most of those lines are not "app logic"; they are hand-written `Codable` structs
and decoders shadowing Rust types, plus the SwiftUI needed because the kernel
emits no view contract. To make the thesis work at scale, NMP must support:

1. **Generated Swift/Kotlin from the FFI types — non-negotiable.** Review #8
   §2.1 already found `nmp-codegen` (980 LoC) generates a Rust module enum and
   *nothing else* — zero Swift, zero headers. The single highest-LoC-elimination
   change available to this project is `nmp-codegen emit-swift`: for every
   `#[derive(Serialize)]` type that crosses the ABI, emit the `Codable` struct.
   This is pure mechanical translation — exactly what codegen exists for. Until
   it ships, every new field on `KernelUpdate` costs a hand-written Swift
   decoder, and the 39K number only grows. **This is the actual lever on the
   thin-shell thesis.** Not the registry — the *decoder generation*.

2. **A generic projection slot on `KernelUpdate`.** Reviews #6/#7 named it:
   `projections: BTreeMap<String, serde_json::Value>`. I will not re-argue it.
   I will only add the blunt version: every one of the 30 fields on
   `KernelUpdate` is a decision that a non-social app inherits and pays for. A
   bookmark app gets `bunker_handshake`, `wallet_status`, `thread_view`,
   `author_view` whether it wants them or not. The struct is a social-client
   API masquerading as a kernel API. One generic map fixes it. It has been
   un-fixed across three reviews.

3. **Backpressure as a stated contract, not a hope.** The command channel
   (`ffi/mod.rs:243`) is an unbounded `mpsc`. Review #8 §1.2 correctly called
   this benign *today* (no host floods it). But the moment `dispatch_action`
   becomes the primary write path — which §1 above mandates within 30 days — a
   host that dispatches in a tight loop grows that queue without bound, on the
   actor's heap, with no signal. NMP must support a **bounded command channel
   with an explicit `try_send`-and-report-rejection** path before `dispatch_action`
   carries real traffic. Not a redesign; a `sync_channel` with a documented
   depth and a `ChannelFull` error variant. Ship it *with* the `dispatch_action`
   migration, not after.

---

## 3. What NMP should stop doing

Blunt, ranked, and every item is a *deletion*, because addition is the disease.

1. **Stop writing direction reviews.** This is the ninth in one day. Eight
   reviews of converging findings is not diligence; it is a substitute for
   acting on them. The marginal value of review #10 is negative — it will cost
   an hour and find what #6 found. **Moratorium: no review #10 until
   `ActionRegistry::reduce` is wired or deleted.** Make the next review *report
   a code change*, not propose one.

2. **Stop expanding `ViewModule`. Delete the dead impls or wire the trait —
   today, not "soon."** 20 impls, **zero** wired to the kernel (confirmed: no
   `view_registry` / `ViewRegistry` symbol exists in `kernel/`). Review #8 said
   "stop expanding." It is now worse: `nmp-reactions` got new `ViewModule` work
   in the last two weeks per the brief. Every new impl is code that compiles,
   passes a static-dispatch test, and is never called at runtime. A new
   contributor implements the trait, wires nothing, and is misled — review #8
   §2 documented exactly this. **Decision forced: either a `ViewRegistry` lands
   this week, or all 20 `ViewModule` impls and the trait itself are deleted and
   the project commits to `KernelEventObserver` as the only read path.** A dead
   trait family with 20 members is not "design ahead." It is a 20-file lie about
   how the system works.

3. **Delete the `_ => Ok(())` arm in `execute_action` (`ffi/action.rs:175`).**
   Right now `nmp_app_dispatch_action` returns a correlation id — a *success
   token* — for any namespace that has no executor. The test
   `execute_action_unknown_namespace_is_noop_ok` (`ffi/action.rs:407`) *asserts
   this lie is intended behavior*. A host dispatches a NIP-29 group action, gets
   a correlation id back, and nothing happened. This is a D6-class defect
   wearing a passing test. The fix is one line: `_ => Err(format!("no executor
   for namespace {namespace}"))`. It will turn the silent failures into loud
   ones — which is the *point*. You cannot migrate to `dispatch_action` while
   `dispatch_action` lies about what it executed.

4. **Delete `nmp-highlighter-core`.** 49 lines of placeholder for an app
   (`NmpHighlighter`, 39K Swift) that does not consume it. Review #8 already
   said delete it. It is still there. Deleting a 49-line dead crate should not
   require a third review to authorize. The fact that it does is the execution
   problem in miniature.

5. **Stop treating `fixture-todo-core` as evidence the substrate thesis works.**
   It appears only in `nmp-cli` codegen templates (`gen.rs`, `init.rs`,
   `main.rs`) — it is a *scaffolding fixture for the code generator*, not an app
   wired through a running kernel actor loop. No smoke test drives a
   `TodoDomainModule` round-trip through `dispatch_action` and a real actor.
   Citing it as "the second app" (as prior planning has) is citing a template.
   Either wire it through an actor in an integration test this week, or stop
   referencing it as proof of anything.

---

## 4. The most dangerous assumption baked into the architecture

**The assumption: "the C-ABI JSON snapshot is a stable enough contract that a
30-field god struct can keep absorbing every app's needs without a generic
seam."**

This is the load-bearing assumption and it is wrong, and here is *what breaks
first when it fails*: **not the kernel — the iOS build, silently, at decode
time.**

`KernelUpdate` carries `schema_version` precisely so a shell can detect a
kernel/shell mismatch (`kernel/types.rs:506`). But the actual decode is 66,409
lines of *hand-written* Swift `Codable`. When field #31 lands on `KernelUpdate`
for the next social feature, three things happen in sequence:

1. The Rust side compiles — adding a field is free in Rust.
2. The Swift side *also* compiles — `Codable` tolerates unknown keys, missing
   keys decode to `nil` for `Optional`.
3. The new data is silently absent in every app that didn't hand-update its
   decoder. No error. No `schema_version` mismatch (the version bumped, but
   nothing *reads* it as a hard gate). A feature that "shipped" in Rust is
   invisible in two of three apps and nobody knows until a user reports it.

The dangerous assumption is not "single-actor" and it is not "hand-rolled
transport" — review #8 already showed the actor and the transport are sounder
than feared. The dangerous assumption is that **a hand-decoded JSON god struct
scales as a multi-app contract.** It does not. It scales as a *single*-app
contract — which is why Chirp (8K, closely co-developed) is fine and
`NmpHighlighter` (39K, diverged) is a monument to the decoders going stale.
What breaks first is cross-app consistency, and it breaks *silently*, which is
the worst possible failure mode. The generic `projections` slot plus generated
Swift decoders (§2.1, §2.2) is not a nice-to-have. It is the only thing that
converts this from a silent runtime divergence into a compile-time error.

---

## 5. One concrete, ≤2-day change with disproportionate leverage

**Delete `nmp_app_publish_note` from the FFI. Make Chirp post a note through
`nmp_app_dispatch_action` / `nmp.publish`.**

Not "deprecate." **Delete.** `#[deprecated]` was review #8's second-priority
suggestion; it didn't happen, and even if it had, a deprecation warning is a
note-to-self that compiles fine forever. Deletion is a forcing function that
*cannot* be ignored — the build is red until iOS migrates.

Why this specific verb, why ≤2 days, why disproportionate:

- **It is the one path that already works.** `execute_action` already handles
  `nmp.publish` → `ActorCommand::PublishSignedEvent` (`ffi/action.rs:154-167`).
  The Rust executor exists. This is not building a new path; it is forcing the
  caller onto the path that's already paved. That is what makes it a 2-day job
  and not a 2-week one.
- **It produces an unambiguous verdict on the §1 bet, in miniature, this week.**
  If migrating *one* verb that already has an executor takes more than two days,
  you have learned — cheaply — that the 30-day four-verb deadline is unrealistic
  and `dispatch_action` should be abandoned. If it takes two days, you have a
  proven, repeatable migration recipe for the other three. Either way you exit
  the week with a real answer instead of a ninth opinion.
- **It is the first actual subtraction from the FFI surface.** The surface has
  only ever grown — 49 symbols, `dispatch_action` added *on top of* the verbs.
  Deleting one symbol and watching the project survive it breaks the
  psychological pattern that addition is the only safe move. That is the
  disproportionate part: the leverage is not the one verb, it is proving the
  team can retire surface area at all.

Concrete steps: (1) delete the `nmp_app_publish_note` `#[no_mangle]` fn and its
`ActorCommand::PublishNote` variant; (2) update Chirp's `KernelBridge.swift`
note-post call site to build a `PublishAction::Publish` and call
`nmp_app_dispatch_action`; (3) fix `execute_action`'s `_ => Ok(())` to
`_ => Err(...)` while you are in that file (§3 item 3 — it is the same 2-day
window and the migration is unsafe without it). One verb, one Swift call site,
one honesty fix. Red build until done. Ship it before review #10 is even
considered.

---

## Summary

- The thin-shell thesis is **inverted**, not delayed: `nmp-highlighter-core` 49
  lines vs. `NmpHighlighter` 39,097. The brief's "Chirp 39K" is wrong — Chirp is
  8.3K and is the *healthiest* app. `KernelUpdate` is 30 fields, not 76.
- Eight reviews; the only finding that shipped (`sweep_inflight_timeouts`) was
  the only one that didn't require *deleting* anything. The project can fix
  bugs and cannot retire scaffolding. **That is the actual problem.**
- **#1 bet:** delete `nmp_app_publish_note/react/follow/unfollow` within 30 days
  or delete `dispatch_action` instead. Both-alive is the only failure state.
- **Support:** generated Swift `Codable` from FFI types (the real LoC lever); a
  generic `projections` map on `KernelUpdate`; a bounded command channel before
  `dispatch_action` carries load.
- **Stop:** writing reviews (moratorium until `reduce` is wired or deleted);
  expanding `ViewModule` (wire the registry this week or delete all 20 impls);
  the `_ => Ok(())` lie; `nmp-highlighter-core`; citing `fixture-todo-core` as a
  built app.
- **Most dangerous assumption:** a hand-decoded JSON god struct scales as a
  multi-app contract. It fails *silently* at iOS decode time — the worst
  failure mode there is.
- **2-day change:** delete `nmp_app_publish_note`, route Chirp through
  `dispatch_action`, fix `execute_action` to return errors. First subtraction
  from the FFI surface. Red build until done.

The one-sentence version: **NMP's problem is not that the design is wrong — it
is that the team keeps designing instead of deleting. Stop reviewing. Delete a
verb. Make the build red. Ship the subtraction.**
