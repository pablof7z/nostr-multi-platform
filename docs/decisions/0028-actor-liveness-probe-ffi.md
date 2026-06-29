# ADR-0028 — Actor-liveness probe FFI

Date: 2026-05-22  
Status: Superseded by the UniFFI/native-runtime lifecycle API
Deciders: NMP team

## Supersession note

This ADR originally approved a public C-ABI liveness probe as a temporary
pull-side sibling for the update-channel panic frame. The native runtime now
owns lifecycle and liveness APIs directly, with UniFFI bindings for host
shells. The old public C export was retired by the M14 lifecycle clean-break
slice, so this ADR remains only as historical context for the liveness
semantics.

## Context

The actor thread in `nmp-core` owns the kernel loop. When it panics, the
runtime supervisor wraps the actor in `std::panic::catch_unwind` and emits exactly one
`UpdateEnvelope::Panic` frame (`{"t":"panic","v":{"msg":...}}`) on the update
channel before the channel closes (`docs/architecture/d7-actor-death-contract.md`
context; see also `crates/nmp-core/src/update_envelope.rs`).

The push-side panic frame is sufficient when the host is actively draining
the update callback when the actor dies. But on iOS the host can miss the
frame entirely:

- The app is backgrounded. The Swift listener thread is still wired but
  `KernelBridge.swift`'s `nmpUpdateCallback` is a no-op for the duration of
  the system's background suspension.
- The actor panics during a NIP-77 reconcile or a background fetch.
- The supervisor closure emits the panic frame, the update channel closes,
  the listener thread exits.
- The user foregrounds the app. The Swift listener has already exited; the
  push signal is gone.

From the user's perspective the timeline freezes, "Send" taps fail silently,
and the only diagnostic is the OS log line `NMP_ACTOR_PANIC detected` —
which never reaches the user.

The push-side frame remains the primary signal; this ADR adds a pull-side
sibling for the case above.

## Decision

Add a pull-side liveness probe. Semantics:

- `app == NULL` → `0`
- `actor` mutex poisoned → `0` (kernel state irrecoverable)
- `actor` slot is `None` (already joined by `Drop`) → `0`
- `JoinHandle::is_finished()` returns `true` → `0`
- otherwise → `1`

The host treats every negative response as "kernel dead — surface a fatal
error to the user". The probe is called on the existing active scene-phase
transition alongside the foreground lifecycle report.

## Rationale: why not a per-verb `dispatch_action` namespace

App verbs route through the generic action doorway instead of gaining one C/JNI
symbol per verb. That is correct for publish, react, follow, zap, and DM send:
per-verb native symbols double maintenance and promise ABI stability to App
Store binaries.

A liveness probe is **not** an app verb:

- It does not produce events, mutate state, or enqueue an `ActorCommand`.
- It has no `correlation_id`, no `action_result`, no snapshot projection.
- It cannot route through `dispatch_action` because `dispatch_action` itself
  requires a live actor — the dispatch goes through `send_cmd`, which is the
  exact path this probe diagnoses as broken.
- It is observability of the FFI plumbing itself, not of the protocol state
  that plumbing carries.

The freeze's failure message documents this exception explicitly (lines
98–101 of `ci/check-ffi-surface-freeze.sh`):

> If you believe a new nmp_app_* export is genuinely required (e.g. a
> lifecycle hook with no dispatch analogue), write an ADR and reference
> it in your commit message as 'ADR-XXXX: <title>'.

The original C-ABI probe was exactly that case. The matching ADR override in
the freeze gate was removed when the public C export was retired.

## Constraints (hard limits on this exception)

- The override applied only to the original liveness probe. The freeze gate's
  default-reject behaviour for every other new native symbol is unchanged.
- Future lifecycle / observability probes (e.g. `nmp_app_actor_queue_depth`
  as a C-ABI counterpart to the `actor_queue_depth` snapshot metric) MUST
  file a new ADR with its own override entry. The override is per-symbol,
  not a per-category bypass.
- The probe is read-only. A future request to "add a `nmp_app_restart_actor`
  symbol" must be rejected — restart is an app verb that goes through
  `dispatch_action` (or, more honestly, through process restart — see the
  Swift banner's "Relaunch" button which calls `exit(0)`).
- The Swift host MUST NOT poll the probe (no `Timer` / `DispatchSourceTimer`
  loop). The two legitimate consumers are (a) the scenePhase active arm,
  fired once per foreground transition, and (b) future ad-hoc debug
  diagnostics. Polling would re-introduce the very anti-pattern the no-polling
  doctrine (`docs/perf/feedback_no_polling.md`) forbids.

## Consequences

- The historical implementation added one C-ABI symbol. The current
  implementation keeps the semantics on the native runtime/UniFFI path and
  rejects reintroduction of that public C export.
- The Swift host gains a deterministic answer to "is the kernel still
  there?" without re-attempting a dispatch and inferring from silence.
- The user gets a visible, actionable error (the red banner in
  `RootShell.swift`) instead of a frozen UI. The banner's "Relaunch" button
  is the only recovery path: a panicked actor cannot be restarted in-process
  because the kernel state (event store, MLS DB, NIP-77 watermarks) is in an
  unknown state — restart is the safe disposition.
