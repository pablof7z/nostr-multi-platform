# Opus Direction Review #34 — NMP Architecture

Date: 2026-05-21
Reviewer: Opus (architect advisor)
`master` HEAD: `ca9f6d02` (`docs(readme): qualify WASM delivery path as aspirational`)
Scope: post-PR-#99 NIP-29 wiring correctness, cancel/retry two-doors,
shipped-but-inert verification gap, `actor_queue_depth`, the NIP-57
`HttpCapability` sequencing problem, highest-ROI next step.

Baseline corrected from: review #33. Review #33's still-valid verdicts
(`action_results` dormant-but-correct, Option A for LNURL, WASM aspirational,
NIP-17 greenfield, `last_action_result` deletable, `decode.rs` hash-verify
precondition) are NOT relitigated here.

---

## 0. Headline

PR #99 is **mechanically correct and architecturally inert.** All 15 NIP-29
namespaces are now reachable through `dispatch_action` — and not one of them is
called by any host. The scope of review #33 §4.1's #1 risk
("shipped-but-inert features camouflaged by green CI") did not shrink; it grew
by 14 namespaces. Separately, the review #33 §6 decision (`HttpCapability` as
"a second `CapabilityModule`") rests on a **false premise**: the capability
seam is **synchronous**, and a second impl that does HTTP would stall the
actor thread. That finding (Q5) is the most important in this review.

---

## 1. Q1 — NIP-29 wiring correctness: the 15 executors ARE reachable

**Verdict: wiring is correct. There is one real wire-format inconsistency.**

### (a) All 15 namespaces are registered

`apps/chirp/nmp-app-chirp/src/ffi.rs:349-391` `register_nip29_actions` wires
exactly 15 actions via the local `wire!` macro (`ffi.rs:353-370`):

- membership (2): `JoinRequestAction`, `LeaveRequestAction`
- content (3): `PostChatMessageAction`, `PostDiscussionAction`, `PostArtifactAction`
- composed (3): `ShareEventIntoGroupAction`, `ReactInGroupAction`, `CommentInGroupAction`
- admin (7): `CreateGroupAction`, `EditMetadataAction`, `PutUserAction`,
  `RemoveUserAction`, `CreateInviteAction`, `DeleteEventAction`, `DeleteGroupAction`

Count = 15. The doc comment's "all 15" is accurate.

### (b) Executor closures call the right command functions

The `wire!` macro (`ffi.rs:353-370`) expands each entry to:

- `register_action_module($Action::NAMESPACE, …)` — a validator that decodes
  `$Input`, builds an `ActionContext`, and delegates to `$Action::start`;
- `register_action_executor($Action::NAMESPACE, …)` — a closure that calls
  `$command(action_json)?` and `send(cmd)`.

Each `wire!` invocation (`ffi.rs:373-390`) pairs the correct
`(Action, Input, command)` triple — verified against the imports at
`ffi.rs:33-45`, which list 15 `*_command` functions, 15 `*Action` types and
15 `*Input` types. No triple is mismatched.

### (c) `wire!` macro is correctly expanded

The macro is hygienic and used 15 times with no name collisions. The
validator-and-executor pair both key off `$Action::NAMESPACE`, so a namespace
can never have a validator without an executor (or vice versa). This is the
correct shape — `dispatch_action` requires both halves.

### The real problem: namespace naming is inconsistent across the catalog

`crates/nmp-nip29/src/action/admin.rs:36` — the `admin_action!` macro
generates the namespace as:

```rust
const NAMESPACE: &'static str = concat!("nip29.", stringify!($Module));
```

`$Module` is the **CamelCase Rust identifier** (`CreateGroupAction`), so the
7 admin namespaces are `nip29.CreateGroupAction`, `nip29.EditMetadataAction`,
`nip29.PutUserAction`, etc.

The 8 non-admin namespaces (membership / content / composed) are **snake_case**
— `ffi.rs:581` dispatches `"nip29.join_request"` directly in a test, and the
crate's own `JoinRequestAction::NAMESPACE` is `nip29.join_request`.

So the NIP-29 namespace catalog is **half snake_case, half CamelCase**:

| submodule  | example namespace               | convention |
|------------|---------------------------------|------------|
| membership | `nip29.join_request`            | snake_case |
| content    | `nip29.post_chat_message`       | snake_case |
| composed   | `nip29.react_in_group`          | snake_case |
| admin      | `nip29.CreateGroupAction`       | CamelCase  |

**Why this is a real problem, not cosmetic.** Both register *and* dispatch
sides use the `Action::NAMESPACE` constant, so in-tree the strings always
agree — that is why CI is green and `nip29_all_namespaces_dispatch_through_action_registry`
(`ffi.rs:694`) passes. But `nmp_app_dispatch_action`'s first argument is a
**raw string supplied by an external caller** (Swift, or a future SDK
consumer). That caller cannot import `CreateGroupAction::NAMESPACE`; it types
a string literal. A developer who has wired `nip29.join_request` will
reasonably type `nip29.create_group` and get
`ActionRejection::Invalid("unknown action namespace")` from
`action_registry.rs:275`. The wire format is a public API; an inconsistent
one is a latent integration bug.

The `register_nip29_actions` doc comment (`ffi.rs:346-348`) even *documents*
the inconsistency as a known quirk ("the admin namespaces are
macro-generated as `nip29.<ModuleIdent>`"). Documenting a wart is not fixing
it.

**Recommendation:** snake_case is the established precedent (8 of 15, plus
`nmp.publish`, `nmp.zap`, `chirp.react`). Make `admin_action!` emit it. The
mechanical fix: pass an explicit namespace literal as a macro argument, or
lower-case-with-underscores `stringify!($Module)` at const-eval time (a small
`const fn`). One PR, 7 namespaces, zero behaviour change — and it must land
**before** any host dispatches a NIP-29 admin verb, because changing a
public namespace string after a caller ships is a breaking change.

---

## 2. Q2 — Cancel/Retry publish: the two-doors problem

**Verdict: the bespoke C symbols are the RIGHT door for cancel/retry. The
actual defect is the dead `PublishAction::Cancel` arm — delete it.**

### The two doors, precisely

Door 1 — bespoke C symbols (live, correct):
- `nmp_app_cancel_publish` / `nmp_app_retry_publish`
  (`crates/nmp-core/src/ffi/identity.rs:306,317`) each send
  `ActorCommand::CancelPublish` / `RetryPublish`.
- Those commands have **real handlers**: `dispatch.rs:334`
  `ctx.kernel.retry_publish_now(&handle)` and `dispatch.rs:339`
  `ctx.kernel.cancel_publish(&handle)`. Both emit an update afterward.
- Swift calls them: `KernelBridge.swift:229-234` →
  `NotificationsView.swift:24-25` (the outbox row's retry/cancel buttons).

This path is **fully wired and exercised by real UI.**

Door 2 — `dispatch_action`'s `nmp.publish` `Cancel` variant (dead):
- `action_registry.rs:461`: `PublishAction::Cancel { .. } => Ok(())`.
- The comment admits it: "No publish-engine cancel command yet; the registry
  already marked the action `Cancelled`." It is a **no-op stub**.
- Nothing in Swift dispatches `{"Cancel":{…}}` to `nmp.publish`. Its only
  exercise is `start_cancel_action_returns_correlation_id`
  (`action_registry.rs:529`), which proves the *validator* runs — never the
  executor's effect.

### Is the two-doors pattern a problem here?

**No — and this is the correct call.** Cancel and retry are **control-plane
verbs**: the host already holds the publish `handle` (it came from the outbox
projection). There is no payload to validate, no `correlation_id` to mint, no
multi-step plan. The whole value of the `dispatch_action` round-trip — typed
validation + correlation-id assignment + a pluggable executor table — buys
nothing for "cancel handle X." Routing cancel/retry through the action
registry would add a JSON encode/decode and a registry lookup to deliver the
exact same `ActorCommand` the bespoke symbol already sends. That is ceremony,
not architecture.

This is **not** the `nmp_app_publish_note` anti-pattern reviews #13–#19
unwound. That anti-pattern was a per-app host owning a **data-plane**
substrate step (building and routing a Nostr event). Cancel/retry own no
protocol logic — they are pure intent signals over an opaque handle. D0 is
not implicated; `nmp-core` already owns the publish engine and its `handle`
type.

### The actual defect

`PublishAction::Cancel { .. } => Ok(())` is **dead code that looks alive.**
A reader of `action_registry.rs` sees a `Cancel` variant in the `nmp.publish`
executor and reasonably believes cancellation flows through `dispatch_action`.
It does not. This is the *same* "shipped-but-inert" pattern as Q3 — a green,
tested arm with no real effect.

**Recommendation:** delete the `Cancel` variant from `PublishAction` and the
`Cancel` arm from the `nmp.publish` executor (`action_registry.rs:461`).
Update `start_cancel_action_returns_correlation_id` to exercise a live
variant instead. Cancel/retry stay on the bespoke C symbols — document, in
one line on those symbols, that they are the *intentional* control-plane door
and `dispatch_action` deliberately does not carry them. A deliberate
exception, stated, is architecture. An undocumented dead arm is debt.

---

## 3. Q3 — The "shipped but inert" gap: WORSE after PR #99, not better

**Verdict: review #33 §4.1's #1 risk is unchanged in kind and larger in
scope. PR #99 moved 14 namespaces from "unregistered" to "registered but
uncalled." That is not progress against the risk — it is the risk,
reproduced 14 times.**

### Evidence — what Swift actually dispatches

`grep -rn 'dispatch_action\|namespace:' ios/Chirp/Chirp/` — every
`dispatchAction(...)` / `nmp_app_dispatch_action` call site:

- `KernelBridge.swift:191-192` — `namespace: "nmp.publish"` (publishNote)
- `KernelBridge.swift:221` — `"nmp.publish"` (publish signed)
- `KernelBridge.swift:259-260` — `"chirp.react"`
- `KernelBridge.swift:265` — `"chirp.follow"`
- `KernelBridge.swift:269` — `"chirp.unfollow"`

**That is the complete list.** Four namespaces: `nmp.publish`, `chirp.react`,
`chirp.follow`, `chirp.unfollow`.

- NIP-29: **0 of 15** namespaces dispatched from Swift.
- NIP-57 `nmp.zap`: **0** dispatch sites.

### The honest assessment

PR #99's commit message — `feat(nip29): wire 14 dormant ActionModule impls
through dispatch_action` — is accurate about the library and silent about the
consumer. After PR #99, the 15 NIP-29 executors are *registered, validated,
typed, and unit-tested* — and a Chirp user cannot create a group, post a
chat message, or moderate a member, because **no screen calls them.** The
state transition PR #99 delivered is precisely:

> 14 namespaces: `unregistered` → `registered-but-uncalled`

The `nip29_all_namespaces_dispatch_through_action_registry` test
(`ffi.rs:694`) is real and valuable — it proves the *seam* carries the
namespaces. But it dispatches them from a Rust test harness, not from
Chirp's UI. It verifies the library half. It cannot verify the
consumed-by-a-real-app half, because that half does not exist.

This is the camouflage review #33 §4.1 named: green CI, a convincing
changelog, and an inert feature. PR #99 did not reduce the inert surface —
it is the largest single addition to it. NIP-29 group functionality in Chirp
is **0% reachable by a user** despite 15 green executors.

**Recommendation (discipline rule, restated and now overdue):** stop wiring
NIP-crate `ActionModule`s ahead of a UI consumer. A namespace is not "wired"
until one Chirp screen dispatches it and that path is exercised. Until then
the honest changelog verb is "scaffold," not "feat … wire." The next NIP-29
work should be **subtractive or consuming**: either delete the admin
executors Chirp has no screen for, or build the one screen (see §6).

---

## 4. Q4 — `actor_queue_depth` is still hardcoded to 0

**Verdict: still true. The unbounded channel is unobservable. There is a
clean minimum-viable fix.**

### Confirmed still true

- `crates/nmp-core/src/kernel/types.rs:473` — `pub(super) actor_queue_depth: u32`.
- `crates/nmp-core/src/kernel/update.rs:104` — `actor_queue_depth: 0,`
  (a literal, every tick).
- The command channel is `mpsc::channel()` (`lib.rs:178`,
  `actor/mod.rs:543` for the relay channel; the command channel is the
  std unbounded `mpsc`). `run_actor(command_rx: Receiver<ActorCommand>, …)`
  (`actor/mod.rs:440`).
- The stress harness already documents this as a known no-op:
  `crates/nmp-testing/bin/ffi-stress/s4_reconciler_backpressure.rs:240`
  ("The kernel hardcodes actor_queue_depth=0 … always passes") and `:421`
  ("follow-up: wire mpsc channel length to Metrics::actor_queue_depth").
  Gate G-S4 row 2 (`:297`) asserts `actor_queue_depth_peak <= 50` against a
  value that is structurally always 0 — **the gate is currently vacuous.**

### Is there ANY mechanism to detect queue growth?

**No.** `std::sync::mpsc::Receiver` exposes no `len()`. There is no counter,
no high-water mark, no shed-load path. A fast producer (an FFI thread looping
`dispatch_action`, or relay-event feedback enqueuing publishes) grows the
queue with zero visibility. The G-S4 gate that should catch it cannot, by
construction.

### Minimum viable change — observability, NOT a bounded channel

Do **not** convert to a bounded `SyncSender`. `actor/mod.rs:6-12` records the
deliberate decision to stay unbounded (a bounded channel "could fill", no
forwarder threads, no drops). Bounding is a separate, larger design call.
The MVC is to make the depth *measurable*:

**Option 1 (smallest diff) — an `Arc<AtomicU64>` straddle counter.**
- Add `queue_depth: Arc<AtomicU64>` shared between `NmpApp` and the actor.
- `NmpApp::send_cmd` does `fetch_add(1, Relaxed)` before the channel `send`.
- The actor's command-drain loop (`actor/mod.rs:669`, the `try_recv` site)
  does `fetch_sub(1, Relaxed)` per successfully drained command.
- `make_update` reads the counter into `actor_queue_depth` at `update.rs:104`.
- Cost: ~10 lines, one atomic per enqueue/dequeue (negligible).

**Option 2 — migrate the command channel to `crossbeam_channel`.**
`crossbeam::channel::Receiver` has `.len()`. This is cleaner long-term and is
also the prerequisite if a bounded channel is ever wanted. Larger blast
radius (it touches every `command_rx` signature) but removes the manual
counter. Reasonable as a follow-up; Option 1 is the freeze-friendly choice.

Either way, once `actor_queue_depth` carries a real number, the G-S4 gate
(`s4_reconciler_backpressure.rs:297`) stops being vacuous and starts
catching the OOM-precursor it was written to catch. That alone justifies the
~10-line PR.

---

## 5. Q5 — What must happen AFTER HttpCapability: the seam is SYNCHRONOUS

**Verdict: `HttpCapability` CANNOT land as "a second `CapabilityModule`" as
review #33 §6 scoped it. The capability seam is synchronous and blocking;
adding HTTP to it stalls the actor thread (D8 violation). The full zap flow
requires a new async-capability design. This is the most important finding
in review #34.**

*Note: PR #100 added the `HttpCapability` type definition and iOS URLSession
implementation. Since the ZapModule executor was correctly NOT wired to call
it (the capability slot is inaccessible from executor closures), there is no
live D8 violation — the type exists but nothing calls `dispatch_capability`
for HTTP. The concern is forward-looking: before any executor is wired to
use it, the async-capability ADR must be written.*

### The seam is synchronous — evidence

`crates/nmp-core/src/capability_socket.rs:32` —
`dispatch_capability(slot, request_json) -> String`. It:

1. locks the callback slot,
2. **calls the native `extern "C"` callback inline**
   (`capability_socket.rs:40-42`),
3. takes the returned C string and returns it as an owned `String`.

There is no channel, no correlation-id round-trip, no continuation. The
caller **blocks on the native callback's return value.**

The only caller today, `run_keyring`
(`actor/session_persistence.rs:241`), proves the call site is the **actor
thread**: `session_persistence` runs inside actor command dispatch, and it
consumes `dispatch_capability`'s return synchronously. For keyring this is
fine — a Keychain read is microseconds.

### Why this breaks for HTTP

An LNURL-pay round-trip is two network requests (`GET` the lnurl-pay
endpoint, `POST` the signed kind:9734 to its callback). On a phone that is
hundreds of milliseconds to seconds, and unbounded on a bad network. If an
executor calls `dispatch_capability` for HTTP, the **actor thread blocks for
the entire HTTP round-trip.** During that block:

- every `dispatch_action`, every cancel, every `Start` waits;
- relay events back up;
- `emit_hz` cadence is missed.

That is a direct D8 violation ("the actor never blocks").

### Does the executor model support multi-step async actions? No.

An `ActionModule` executor is `Fn(&str, &str, &dyn Fn(ActorCommand)) -> Result<(), String>`
(`action_registry.rs:108`). It runs **once, synchronously**, and its only
output is "send zero or more `ActorCommand`s now, then return." It has no
way to await an HTTP response, resume when the bolt11 invoice arrives, or
chain `build kind:9734 → lnurl GET → lnurl POST → hand bolt11 to wallet`.

The full zap is inherently a **multi-step async saga**. The current executor
model is single-shot. So completing the zap flow is **not** "wire one more
capability" — it requires a continuation mechanism.

### The required design

The minimal correct design: **async capability via correlation-id round-trip.**
Instead of `dispatch_capability` returning inline, it should:
- enqueue the `CapabilityRequest` to the host off the actor thread;
- the host delivers the `CapabilityEnvelope` back **as a new `ActorCommand`**
  (e.g. `ActorCommand::CapabilityEnvelopeReady { envelope }`), re-entering
  the actor through the normal command lane;
- the actor matches the `correlation_id` to a pending zap-saga state and
  advances it one step.

This makes the zap a state machine driven by the existing command lane — no
blocking, D8 preserved.

**Recommendation:** before any executor is wired to use `HttpCapability`,
write an ADR for the **async capability model**. The keyring path can keep a
synchronous fast-path (it's microseconds); HTTP *must* be async.

---

## 6. Q6 — Highest-ROI next step

**If only one thing: wire ONE existing typed action from Swift UI, end to end.**

Pick a NIP-29 verb whose executor already works (e.g. `nip29.post_chat_message`
or `nip29.create_group`) and build the single Chirp screen that dispatches it,
with the dispatch path in CI.

Why this wins:
1. Directly attacks the #1 risk (§3). Today *zero* of 15 NIP-29 executors are
   reachable by a user. One end-to-end wired action turns the verification story
   from "library half tested, app half unknown" into "one full path proven."
2. Zero new substrate. The executor, validator, command, and actor handler all
   exist and are tested. New code is only Swift UI + one bridge call.
3. Freeze-safe. Small, additive, no architectural decision pending.

**Necessary parallel track:** write the **async-capability ADR** (§5). It is
the gate on the entire zap feature. The ADR is cheap (no code) and unblocks
correctly-scoped zap work.

**Prioritized runner-ups:**
- Fix the admin namespace CamelCase inconsistency (§1) — **must land before**
  any host dispatches an admin verb.
- `actor_queue_depth` observability (~10 lines, un-vacuums G-S4 gate).
- Delete `PublishAction::Cancel` dead arm (§2).

---

## 7. Summary

PR #99 is mechanically correct — all 15 NIP-29 namespaces register and
validate cleanly through `dispatch_action` — and architecturally inert: Swift
dispatches exactly four namespaces (`nmp.publish`, `chirp.react/follow/unfollow`);
**zero** NIP-29 actions and **zero** `nmp.zap`. Review #33 §4.1's #1 risk did
not shrink — it grew by 14 namespaces. The most consequential new finding:
`dispatch_capability` (`capability_socket.rs:32`) is **synchronous and
blocking**; wiring any executor to it for HTTP stalls the actor (D8). The
async-capability ADR must precede all ZapModule HTTP wiring. Admin NIP-29
namespaces use CamelCase (`nip29.CreateGroupAction`) while 8 others use
snake_case (`nip29.join_request`) — a latent integration bug that must be
fixed before any host dispatches an admin verb. `actor_queue_depth` is still
hardcoded `0` (`update.rs:104`), making G-S4 vacuous; a ~10-line AtomicU64
straddle counter fixes it. The `PublishAction::Cancel` arm is a dead no-op
masquerading as wired — delete it; cancel/retry correctly stay on bespoke
control-plane C symbols. Highest-ROI next step: wire ONE NIP-29 action from a
Chirp screen end-to-end in CI, and write the async-capability ADR in parallel.
