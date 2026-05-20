# NMP direction review #8 — runtime correctness, new-NIP DX, stop-doing

Date: 2026-05-20
Reviewer: Opus (principal-engineer direction review)
Scope: runtime safety, developer experience, crate-graph pruning, 30-day bet.
Deliberately NOT covered: ViewRegistry / projections slot (rehashed in reviews
#6 and #7 — it remains a known gap, this memo does not re-litigate it).

---

## 1. Runtime correctness risks

### 1.1 CRITICAL — a publish can stay `InFlight` forever. There is no timeout sweep.

This is the headline finding. The publish retry FSM is sound *as a pure function*
but the engine that drives it has a hole: **nothing ever transitions a relay out
of `InFlight` on the basis of elapsed time.**

Trace:

- `publish/state.rs:156-159` — `PerRelayState::InFlight { sent_at_ms, attempt }`
  records the send time. `TimedOut { attempt, last_at_ms }` exists as a state
  (`state.rs:168-171`).
- `publish/state.rs:5` — the documented state graph shows `InFlight --Timeout-->`
  as a real edge. But that edge is only ever taken when an *ack* with
  `code == "timeout"` is fed into `apply_ack`. `apply_ack` only consumes acks for
  `InFlight` states (`state.rs:266`) — it never fires on the passage of time.
- `publish/traits.rs:202-214` — in production the dispatcher is `QueueDispatcher`,
  whose `dispatch()` **returns `Vec::new()`**. No synchronous ack. The comment is
  explicit: "every relay stays InFlight until the kernel feeds the real OK frame
  in via `on_ack`."
- `publish/engine.rs:237-243` — `PublishEngine::tick()` calls only
  `dispatch_pending` for every handle, then `flush_view()`. `dispatch_pending`
  → `dispatch_due` (`engine/helpers.rs:19-63`).
- `publish/engine/helpers.rs:37-49` — `dispatch_due`'s readiness check matches
  exactly three states: `Pending`, `RelayError`, `TimedOut`. **`InFlight` is in
  the `_ => false` arm.** A relay sitting in `InFlight` is never re-examined by
  `tick`. `sent_at_ms` is written (`helpers.rs:54-55`) and never read.

So the live failure modes:

1. **Relay accepts the TCP connection, swallows the `EVENT`, never sends `OK`.**
   This is common in the wild — overloaded relays, relays that silently drop
   events failing an undisclosed policy, NIP-42 relays that expect AUTH but send
   nothing. The socket stays healthy, so no `RelayEvent::Failed`/`Closed` fires.
   The publish is `InFlight` permanently. The user sees a spinner that never
   resolves; `FsPublishStore` keeps the row across restarts, so it is *durably*
   stuck.

2. **Relay closes mid-sequence.** This case *is* covered — but only by luck of a
   different mechanism. `actor/dispatch.rs:531` and `:542` call
   `kernel.mark_publish_relay_unavailable` on `RelayEvent::Failed` /
   `RelayEvent::Closed`. `engine.rs:248-272` (`mark_relay_unavailable`) moves any
   `InFlight` row for that relay back to `Pending`. Good — but this fires *only*
   on an explicit socket-level event. The relay must actively close the socket.
   The silent-drop case in (1) produces neither event and is not rescued.

The escape valve is therefore "the relay must either OK the event or kill the
socket." Any relay that does neither pins the publish forever. There is no
defense in depth.

`RetryPolicy` (`state.rs:213-230`) has `backoff_base_ms`, `backoff_factor`,
`transient_max_retries` — but **no `inflight_deadline_ms`**. The policy literally
cannot express "give up waiting for an OK after N seconds." The 30-day bet (§4)
fixes exactly this.

### 1.2 LOW — the 60 Hz emit path cannot starve the command mailbox. Confirmed sound.

The dual-channel design in `actor/mod.rs:511-581` is correct. Commands are drained
in a `loop { try_recv }` at the top of every iteration (`mod.rs:517-581`) *before*
any relay event is touched; relay events use a separate `relay_rx` read with
`recv_timeout(compute_wait(...))` (`mod.rs:585-586`). A relay-event flood cannot
queue ahead of a command — they are not in the same channel. The old 4096-slot
merged `SyncSender` that could silently drop `CreateAccount` is gone. This is
genuinely fixed; no action needed.

One residual: the command channel (`ffi/mod.rs:243`, `mpsc::channel()`) is
**unbounded**. A host that calls `dispatch_action` in a tight loop faster than the
actor drains will grow that queue without bound. Not a 30-day concern (no host
does this), but it is the one place "no backpressure" is literally true. Worth a
one-line note in `substrate/mod.rs` so it is a known property, not a surprise.

### 1.3 LOW — observer fan-out IS bounded. Confirmed sound, credit where due.

The prompt asks whether `Arc<dyn KernelEventObserver>` fan-out is bounded. It is.
`kernel/event_observer.rs:77` defines `C_FANOUT_CHANNEL_BOUND = 1024`; the C-ABI
fan-out uses `try_send` and drops on overflow (`event_observer.rs:311`), with the
foreign callbacks invoked on a dedicated drain thread, never the actor thread
(`event_observer.rs:150-157`, `:268-312`). The doctrine comment at
`event_observer.rs:42-64` names this exact starvation concern and shows it was
designed against. Rust trait observers fire synchronously on the actor thread by
deliberate contract ("must be cheap and must not panic"). This is the correct
trade. No change needed — and the design here should be the *template* for the
publish-timeout fix.

### 1.4 Heartbeat / `SystemTime::now()` — NOT a real risk. Prompt premise corrected.

The prompt asks what happens to the liveness heartbeat on NTP jumps. Answer: the
heartbeat does not use wall-clock time, so nothing happens.

- `actor/mod.rs:501` — `last_emit` is seeded from `Instant::now()` (monotonic).
- `actor/tick.rs:compute_wait` / `flush_due` — all staleness/emit-cadence math is
  `last_emit.elapsed()` and `Instant` arithmetic. Monotonic; immune to NTP.
- The publish FSM takes `now_ms` injected (`state.rs:259`, `engine.rs:237`).
- The only `SystemTime::now()` in `nmp-core` is `ffi/action.rs:231` (`now_ms()`),
  used to stamp `ActionContext::now_ms` on dispatched actions — a metadata
  timestamp, not a liveness clock. A backwards NTP jump there only mis-stamps an
  action; it does not stall or falsely-expire anything.

There is one real wall-clock dependency worth noting: `kernel/publish_engine.rs`
uses `now_epoch_ms()` (via `SystemTime`) to drive publish *retry backoff*
deadlines (`pending_retries` stores epoch-ms due times — `engine.rs:60`). A large
backwards NTP step would delay a scheduled retry by the step size. This is benign
(retries are best-effort, the user is not blocked) but should be documented. It is
not a heartbeat bug.

**Net for §1:** one critical bug (1.1). Everything else the prompt fished for is
either already correctly handled (1.2, 1.3) or a non-issue (1.4). Do not let the
fishing-expedition framing dilute the memo — the publish timeout is the finding.

---

## 2. New-NIP DX walkthrough — where it breaks down

The question: can a new engineer add a NIP in a day, kernel + iOS Chirp surface?
**No. Realistically 3-5 days, and the codegen does not help.** Evidence is the
actual NIP-23 (long-form articles) crate, added in one commit `0e320243`:

```
crates/nmp-nip23/  — 19 files, 1,968 LoC in a single commit
  Cargo.toml, src/{build,decode,domain,kinds,lib}.rs,
  src/view/{accumulator,detail,list,mod}.rs, + 13 test files
```

The steps to add a NIP today, in order, and where each one bites:

1. **Create `crates/nmp-nipNN/`.** Hand-author `kinds.rs`, `build.rs` (event
   builder), `decode.rs` (tag → typed struct), `domain.rs` (the `DomainModule`
   impl), and `view/` (the `ViewModule` impls). NIP-23 spent ~1,200 non-test LoC
   here. There is no scaffold — `cargo new` plus copy-paste from `nmp-nip22`.
   *Breaks down: no template, no `nmp-codegen new-nip` subcommand.*

2. **Implement the substrate traits.** `decode.rs`/`view/` implement `ViewModule`,
   `domain.rs` implements `DomainModule` (`substrate/mod.rs:43-58`). This is real,
   load-bearing work — the traits are exercised by tests via **static dispatch**
   (`<RepliesView as ViewModule>::open(...)`).
   *Breaks down hard: `substrate/mod.rs:9-13` admits the kernel-side dispatch
   runtime "does not exist yet." So you implement `ViewModule`, and then the
   kernel never calls it. To actually get your NIP's events to the app you must
   ALSO register a `KernelEventObserver` (the real v1 path,
   `substrate/mod.rs:21-33`). A new engineer reads the trait, implements it,
   wires nothing, and is surprised. Two parallel mental models, one of them dead
   at runtime.*

3. **Register the crate in `nmp-codegen` manifest.** Add the crate name to
   `[modules].protocol` in the app's `.nmp` manifest (`manifest.rs:49`,
   `ordered_modules` at `:67-72`).
   *Breaks down: this is all the codegen does for a NIP — see §2.1 below.*

4. **Wire it into the kernel.** Because there is no `ViewRegistry`, surfacing the
   NIP means either (a) adding fields to the 76-field `KernelUpdate` struct, or
   (b) registering a `KernelEventObserver` in the per-app crate
   (`apps/chirp/nmp-app-chirp/src/ffi.rs`) that composes the typed view from raw
   `KernelEvent`s. Path (b) is the documented one, but it lives in the *app*
   crate, not the NIP crate — so "add a NIP" actually means "edit two crates plus
   the manifest."

5. **Surface to iOS Chirp.** The `KernelEventObserver` JSON, or new
   `KernelUpdate` fields, cross the C ABI as JSON. The Swift side hand-writes the
   decode. There is **no generated Swift** (see §2.1). For a new typed view this
   is a fresh hand-written Swift struct + decoder + a SwiftUI surface.

Honest estimate: a disciplined engineer who has done it once can do a *small*
NIP (one kind, no cross-references) in ~2 days. NIP-23-scale (addressable events,
naddr resolution, domain index) is a week. "A NIP in a day" is not true today and
the gap is structural, not effort.

### 2.1 Is `nmp-codegen` pulling its weight? Barely.

`nmp-codegen` (980 LoC across 5 files) reads a `.nmp` manifest and generates
**Rust module-wiring glue** — `rust_crate_name`, `variant_name`, `app_crate_name`
(`lib.rs:30-50`), an enum of modules, a `check_modules` drift gate (`lib.rs:10-28`).
That is it. Grep for Swift/Kotlin/header output in `generate.rs` and `ffi_gen.rs`:
**zero**. The codegen does not generate the FFI surface, does not generate Swift
decoders, does not scaffold a NIP crate, does not generate the `KernelUpdate`
projection wiring.

So the single most painful, most error-prone parts of adding a NIP — the C ABI
shape and the Swift decode — are 100% hand-written. The codegen automates the one
part that was already cheap (a module enum). It earns its keep as a drift gate
and not much else.

**What would make it valuable:** a `nmp-codegen new-nip NN` that scaffolds the
crate skeleton (the 5-file `kinds/build/decode/domain/view` layout), and — far
more important — generating the Swift `Codable` structs for every type that
crosses the FFI as JSON. The hand-written Swift decoders are pure mechanical
translation of Rust structs; that is exactly what codegen is for. Until it does
that, the codegen is not pulling its weight relative to its 980 LoC.

### 2.2 `substrate/mod.rs` module doc — what is missing

The doc (`substrate/mod.rs:1-66`) is honest about v1-vs-v2 and that is good. What
it is missing, for a new contributor:

- A **step list**: "to add a NIP, do these N things in this order." The doc
  describes traits; it does not describe the *workflow*. §2 above is what should
  be in that doc.
- An explicit "**do not implement `ViewModule` expecting the kernel to call it**"
  warning at the top, not buried at line 9. The current ordering puts the dead v2
  design first and the live v1 mechanism second.
- The two-crate reality: a NIP touches `crates/nmp-nipNN/` *and*
  `apps/<app>/nmp-app-<app>/src/ffi.rs`. New contributors will not guess this.
- The unbounded command channel property (§1.2).

---

## 3. Stop-doing list

Verified against the crate graph (28 crates) and dependency grep, not assumed.

| Crate / pattern | Verdict | Reason |
|---|---|---|
| **`substrate::ViewModule` (20 impls, 0 wired)** | **Stop expanding** | `substrate/mod.rs:9-13` admits no kernel dispatch runtime exists. Every new `ViewModule` impl is code that compiles, tests via static dispatch, and is never invoked by the kernel. It is the single biggest source of "this looks wired but isn't" confusion (§2 step 2). Either build the registry or stop adding impls — do not keep growing a dead trait family. Pick one. |
| **Old `ActorCommand` verbs vs `dispatch_action`** | **Deprecate the verbs** | `actor/mod.rs:120-340` — `ActorCommand` still carries `PublishNote`, `React`, `Follow`, `Unfollow`, `AddRelay`, etc. as first-class variants *alongside* `Kernel(KernelAction)` and the new `dispatch_action` path. Two dispatch systems, neither marked deprecated. Every new write feature now has an ambiguous home. Pick `dispatch_action`, mark the kind-specific verbs `#[deprecated]`, and stop adding new ones. The doc comment on `PublishUnsignedEvent` (`mod.rs:217`) even says it is a "stepping stone... deprecates kind-by-kind" — finish that sentence in code. |
| **`nmp-highlighter-core`** | **Delete** | 49 LoC total (`lib.rs` 25, `placeholders.rs` 24). Its own doc calls it the M11.5 highlighter placeholder. The `nmp-highlighter` iOS app is 39K Swift LoC with zero NMP usage (per the brief). A 49-LoC placeholder crate for an app that does not consume NMP is pure graph noise. Delete it; recreate if/when highlighter actually integrates. |
| **`nmp-desktop`** | **Keep, but cap it** | A ~200-line egui shell. It is the *only* desktop consumer exercising the FFI from non-iOS, which has real value as a cross-platform smoke test. Do not delete — but enforce the 200-line ceiling. The moment it grows app logic it becomes a second un-budgeted client. |
| **`nmp-repl` + `fanout.rs`** | **Keep — it is a dev tool, not architectural debt** | The brief implies `fanout.rs` is a legacy seam. It is not. It is a bounded worker pool (`FANOUT_MAX_WORKERS = 64`, `fanout.rs:33`) used by the `req` REPL command for relay debugging — `run_discovery` / `launch` dial relays for interactive investigation. It is well-tested (network-free unit tests, `fanout.rs:513-623`) and self-contained. It does not feed the kernel. Classify it as a developer tool and leave it alone. The uncommitted edit in the working tree should just be committed or reverted — it is not a design question. |
| **Per-NIP crates merged into `nmp-core`?** | **No — keep them separate** | The per-NIP crate boundary is doing real D0 work: it keeps protocol-kind knowledge out of `nmp-core` (the kernel never names a kind). Merging `nmp-nip22`/`nip23`/`nip57` into core would re-introduce exactly the coupling the architecture is built to avoid. The crate count (28) is not the problem; the *dead trait family* (`ViewModule`) is. Leave the crate split; fix the wiring. |

The through-line: the complexity worth cutting is not crates, it is the **two
parallel half-built dispatch designs** (`ViewModule` vs `KernelEventObserver`,
and `ActorCommand` verbs vs `dispatch_action`). Four mechanisms where there
should be two. That is the cognitive tax, not the crate count.

---

## 4. The 30-day bet — one specific change

**Add an in-flight publish timeout sweep to `PublishEngine::tick`.**

This is the §1.1 critical bug. It is safety-critical (a stuck publish is durable
data loss from the user's perspective — the note never went out and the UI lies),
the prompt explicitly excludes the ViewRegistry rehash, and the patch is small and
self-contained.

Concrete commit:

1. **`crates/nmp-core/src/publish/state.rs:213-230`** — add a field to
   `RetryPolicy`:
   ```rust
   pub inflight_deadline_ms: u64,   // default 30_000
   ```
   Update `Default` (`state.rs:221-230`) to set `30_000`.

2. **`crates/nmp-core/src/publish/engine/helpers.rs`** — add a new pure helper
   alongside `dispatch_due`:
   ```rust
   /// Transition any InFlight relay whose send predates `now_ms - deadline_ms`
   /// into TimedOut, so the existing retry ladder can pick it up. Returns true
   /// if any row changed (caller persists + flushes).
   pub(super) fn sweep_inflight_timeouts(
       in_flight: &mut InFlight, now_ms: u64, deadline_ms: u64,
   ) -> bool
   ```
   For each `PerRelayState::InFlight { sent_at_ms, attempt }` where
   `now_ms.saturating_sub(*sent_at_ms) >= deadline_ms`, set the state to
   `PerRelayState::TimedOut { attempt, last_at_ms: now_ms }` and mark
   `in_flight.dirty = true`.

3. **`crates/nmp-core/src/publish/engine.rs:237-243`** — in `PublishEngine::tick`,
   call `helpers::sweep_inflight_timeouts(row, now_ms, self.policy.inflight_deadline_ms)`
   for every in-flight row *before* the existing `dispatch_pending` loop. Because
   `dispatch_due` (`helpers.rs:39`) already readies `TimedOut` rows, the swept
   relay is automatically re-dispatched on the same tick, walks the existing
   transient-retry backoff ladder, and after `transient_max_retries` settles to
   `FailedAfterRetries` — which already surfaces a `RecentFailure` on the snapshot
   and a terminal `TerminalOutcome` to the iOS queue projection. No new
   surfacing code needed; the swept state plugs into machinery that already
   exists end to end.

4. The actor already ticks the engine: `actor/mod.rs:718-729` calls
   `kernel.tick_publish_engine_for_now()` every idle iteration, and the 250 ms
   idle poll (`tick.rs::compute_wait`) paces it. So once `tick()` does the sweep,
   a stuck publish is detected within ~250 ms of its deadline with **no new
   wakeup, no new thread, no polling loop** — it rides the existing tick. This
   respects the project's "no polling ever" doctrine because the tick already
   exists for retry backoff; the sweep is a free rider on it.

5. Test: `crates/nmp-core/src/publish/engine/tests` — script a `QueueDispatcher`
   publish (no ack), advance injected `now_ms` past `inflight_deadline_ms`, call
   `tick`, assert the relay is `TimedOut` then re-dispatched, and after the retry
   budget assert `FailedAfterRetries` plus a `RecentFailure` row. This is the
   regression gate proving a silent-drop relay can no longer pin a publish.

Why this and not something bigger: it closes a real, durable, user-visible data
loss path; it is ~80 LoC plus tests; it touches three files all inside
`publish/`; it requires zero FFI or Swift changes; and it cannot regress the
happy path (a real `OK` still settles the relay before the deadline). It is the
highest correctness-per-line change available in the next 30 days.

Second priority, if time remains: mark the kind-specific `ActorCommand` verbs
`#[deprecated]` (§3) — zero behavior change, pure signal, stops the two-dispatch-
systems drift from getting worse while the larger consolidation waits.

---

## Summary

- **One critical bug:** publish `InFlight` has no timeout; a relay that accepts
  the socket but never `OK`s pins the publish forever, durably. Fix in §4.
- The dual-channel actor and the observer fan-out are **already correct** — the
  prompt's load/starvation concerns there are unfounded; do not let the framing
  pad the memo.
- The heartbeat is `Instant`-based and **immune to NTP jumps** — non-issue.
- New-NIP DX is **3-5 days, not one**, and the blocker is structural: a dead
  `ViewModule` trait family and a codegen that automates the cheap part while the
  C ABI + Swift decode stay hand-written.
- Stop-doing: stop growing `ViewModule` and the old `ActorCommand` verbs (two
  half-built dispatch designs are the real tax); delete `nmp-highlighter-core`;
  keep `nmp-repl`/`nmp-desktop`/the per-NIP crates — they are not the problem.
- 30-day bet: `sweep_inflight_timeouts` in `PublishEngine::tick`.
