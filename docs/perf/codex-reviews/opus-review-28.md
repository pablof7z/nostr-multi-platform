# Opus Direction Review #28 — NMP architecture

Date: 2026-05-21
Scope: PRs #89 (correlation_id through PublishNote) and #90 (pending_mls_autopublish → AtomicBool)
since review #27.

## TL;DR

PR #89 closes **half** of the correlation_id gap: the *id-mapping* half (host
dispatch id == reported `last_action_result.correlation_id`) is now correct and
end-to-end test-covered for `PublishNote`. The *scalar-vs-vector* half flagged
in review #25 remains open — `last_terminal` is still a single overwritten
slot, so two actions settling between snapshot emits lose the earlier verdict.
PR #90 is a clean, correct right-sizing. No new D0–D8 violations.

The headline finding this review is **not** in the PRs: Chirp's iOS
`KernelBridge` dispatches `chirp.react` / `chirp.follow` / `chirp.unfollow` —
namespaces that **no crate registers anywhere**. Those three user actions are
silently dead: `dispatch_action` returns `{"error":"unknown action
namespace"}` and the bridge frees-and-ignores it. Only `nmp.publish` (the
built-in) actually works from the iOS app.

---

## Q1 — Does PR #89 close the correlation_id round-trip for PublishNote?

**Trace (verified, all hops):**

1. `nmp_app_dispatch_action` → `ActionRegistry::start` validates
   `PublishAction::PublishNote`. `PublishModule::preferred_action_id` returns
   `None` for the unsigned-note variant (the event id is unknown — the actor
   signs later), so `start` mints a fresh 32-hex `correlation_id`. The host
   receives this id as the dispatch return value.
2. `execute_action` → `ActionRegistry::execute("nmp.publish", …, correlation_id,
   send)`. The `default_registry()` executor closure decodes `PublishNote` and
   sends `ActorCommand::PublishNote { content, reply_to_id, correlation_id:
   Some(correlation_id.to_string()) }` (action_registry.rs:441–448).
3. Actor `dispatch.rs:278` destructures all three fields and forwards to
   `commands::publish_note(.., correlation_id, ..)`.
4. `publish_note` (commands/publish.rs:246): on a **local key** it calls
   `kernel.publish_signed_with_correlation(&signed, &[], correlation_id)`. On a
   **remote (NIP-46) signer** it parks a `PendingSign::with_correlation_id(op,
   …, correlation_id)`; the actor idle-poll later publishes via
   `kernel.publish_signed_to_with_correlation(.., ps.correlation_id_override)`.
   Both branches preserve the id — the bunker path is not a regression.
5. `run_publish_engine_at` → `PublishEngine::start_publish(action, now_ms,
   correlation_id_override)` stores it on the in-flight row as
   `correlation_id_override`.
6. On terminal settlement (`on_ack`, `tick`, no-targets, cancel — engine.rs
   357/392/494/705) the engine builds `LastTerminal` with `correlation_id =
   correlation_id_override.unwrap_or(handle)`.
7. `Kernel::last_action_result_projection` reads `publish_engine.last_terminal()`
   and emits it under `projections["last_action_result"]` (update.rs:261).

**Verdict: the id survives every hop.** A dispatched `PublishNote` now settles
under the host-visible dispatch id, not the signed event id. Test coverage is
real: `publish_note_executor_threads_correlation_id_onto_actor_command`
(action_registry.rs) plus
`correlation_id_override_is_reported_in_last_terminal_not_the_handle` and
`no_correlation_id_override_falls_back_to_handle_in_last_terminal`
(publish/engine/tests.rs). This is the right fix and PR #86 + #89 together
close the namespace-split gap review #26 named.

**The remaining half (review #25's open item).** Every `last_terminal`
assignment is an **overwrite** (`self.last_terminal = Some(...)`), not an
append. `recently_completed` *does* accumulate, but it is NOT in the snapshot —
only the scalar `last_terminal` feeds `last_action_result_projection`. If
dispatch A and dispatch B both reach a terminal verdict between two snapshot
emits, the host sees only B; A's verdict is lost. PR #89 fixed *which id* is
reported; it did not make the channel per-tick-drainable.

Whether this is a live bug depends on a host constraint: **does iOS ever have
two actions in flight at once?** Today `nmp.publish` is the only working
namespace, and a user rarely fires two notes inside one ~16ms emit window — so
in practice the scalar is currently adequate. But the moment a second action
namespace goes live (react/follow), concurrent settlement becomes plausible.
Recommend: make `last_action_result` a drained `Vec<LastTerminal>` per snapshot
tick before wiring any second action, or document a hard host-side
serialization contract.

## Q2 — Is `ActionModule::reduce()` called in the production runtime?

**No — and the dormancy is structural, not merely "unwired."**

The registry stores modules as `Box<dyn ErasedActionModule>`. The
`ErasedActionModule` trait **declares only `start()`** (action_registry.rs:56–70).
There is no `reduce` method on the dyn-safe facade at all. `ActionModule::reduce`
therefore *cannot be invoked through the registry* — not "no caller is wired"
but "no callable surface exists." Every `reduce` call site in the tree is a
test (`run_sync.rs` tests, `nip29_lifecycle.rs`, `publish/action.rs` tests) or
the unrelated `Kernel::reduce` (the `KernelAction` reducer, a different method).

What the `ActionTransition` step-machine provides **today**: nothing at
runtime. It is a fully specified, fully tested, fully dormant FSM. `ActionPlan`
(the `start()` output) *is* consumed — `initial_status` flows into the
dispatch-action JSON return. But `ActionInput` / `ActionTransition` /
`Step` / `Output` are exercised only by unit tests. The 15+ `ActionModule`
impls each carry a `reduce` body that is dead weight. This is the same finding
as reviews #18–#27; PR #89 does not change it. The honest framing: the
project has a typed multi-step action FSM designed but a single-shot
"validate → fire ActorCommand → report terminal" runtime. The FSM is
speculative until a multi-step action (`AwaitCapability` / `AwaitUserApproval`)
actually needs to suspend and resume.

## Q3 — Most direct path to a second real (non-publish, non-built-in) action

`ActorCommand::React`, `Follow`, `Unfollow` **already exist** (actor/mod.rs:289–302)
and are fully wired through `dispatch.rs` to live `commands::react` /
`commands::follow`. The smallest-blast-radius second action is therefore
`nmp.react`, needing **zero seam work**:

```rust
// in default_registry() or a host register call:
registry.register::<ReactModule>();              // a trivial ActionModule
registry.register_executor("nmp.react", |json, _id, send| {
    let a: ReactAction = serde_json::from_str(json)?;
    send(ActorCommand::React { target_event_id: a.target, reaction: a.reaction });
    Ok(())
});
```

This is strictly smaller than the NIP-29 `JoinRequestAction`, which would need
`ActorCommand::PublishUnsignedEventToRelays` (currently `#[allow(dead_code)]` —
no live caller) to be invoked from an executor, *and* a relay-pin resolution
path. `nmp.react` reuses an already-live ActorCommand and an already-live
`commands::react` handler.

**But the actual bug to fix first:** the iOS bridge already *names*
`chirp.react` / `chirp.follow` / `chirp.unfollow`. Those should either (a) be
registered in `default_registry` as `nmp.react` / `nmp.follow` / `nmp.unfollow`
and the Swift namespaces corrected, or (b) be registered by a real
`nmp-app-chirp` crate at `register` time. Right now they are dispatched into a
void. Fixing this both removes a silent bug and lands the "second real action"
proof in one move.

## Q4 — New D0–D8 violations in #89 / #90?

**None.**

- PR #89: `correlation_id: Option<String>` is plain data threaded on an
  existing `ActorCommand` variant; the single-writer (D4) and no-Result-across-FFI
  (D6) properties are untouched. The override is `None` for every legacy caller,
  so back-compat holds.
- PR #90: `Arc<Mutex<bool>>` → `AtomicBool` with `swap(false, AcqRel)` for the
  take and `store(_, Release)` for the set. This is a strict simplification — a
  bool behind a mutex never needed the mutex. It also chips at the "108 lock
  sites / Mutex proliferation" finding from review #19. Correct and welcome.

## Standing concerns (unchanged, not regressions)

- `ViewModule`: 14+ impls, still zero `ViewRegistry`. Dead.
- `IdentityModule`: 1 impl (`TodoIdentityModule`), never instantiated. Dead.
- `ActionTransition` FSM: designed, tested, never driven.
- `fixture-todo-core`: `TodoViewModule` / `TodoIdentityModule` still dead;
  `TodoActionModule::reduce` returns `Complete` but is never called.

## Recommendation (priority order)

1. **Fix the Chirp dead-namespace bug.** `chirp.react/follow/unfollow` resolve
   to nothing. Register `nmp.react` / `nmp.follow` / `nmp.unfollow` in
   `default_registry` (zero seam work — the ActorCommands exist) and correct the
   Swift namespace strings. This is a real shipped bug AND the second-action
   proof.
2. **Drain `last_action_result`.** Make it a `Vec<LastTerminal>` drained per
   snapshot tick before action #2 goes live, or the moment two actions can
   settle in one window a verdict is lost.
3. **Decide `ActionTransition`'s fate.** Either build the first multi-step
   action that genuinely suspends (`AwaitUserApproval`), or delete the FSM and
   the 15 dead `reduce` bodies. It has been dormant for 10+ reviews.
