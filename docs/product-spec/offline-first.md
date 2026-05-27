# Offline-First and Flaky-Relay Resilience

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

An NMP app renders immediately — with whatever is in the local store. No waiting for relay responses. No spinner while profiles load. No deferred first frame. This is D1 operationalized: the precise behaviors required for kernel bootstrap, subscription lifecycle, and degraded-relay conditions. Not a new doctrine; the product contract that makes D1 exact.

## 1. The core rule

NMP apps **MUST** render immediately with whatever data is locally available. Blocking the UI on network activity is a framework defect, not an app concern. There is no "loading" gate between a launched app and its first rendered frame; the kernel emits an initial snapshot from the local store as soon as the actor is ready, and the platform draws it.

This rules out, by construction:

- A spinner displayed while the kernel waits for any relay to respond.
- A view that defers its first paint until a subscription reaches EOSE.
- An app screen whose existence is gated on "online" connectivity state.

## 2. What "offline-first" means for NMP

The **kernel's local event store is the source of truth for rendering.** Relay traffic refines that source over time; it never gates access to it. Views project from the working set the kernel holds in memory, which is populated from the durable store at boot. A relay round-trip is a refinement signal, not a precondition.

This is the substrate consequence of D1 + D4 (single writer per fact; caches derive): the store is the writer, the projections are the derived caches, and the relay layer is one of several inputs that mutate the store asynchronously.

## 3. Bootstrap behavior

Kernel startup **MUST NOT** wait on a subscription response, an EOSE, or any relay handshake before emitting its first snapshot. The bootstrap sequence is fixed:

1. Open the event store; load the working set for currently-registered views.
2. Emit an initial `AppState` snapshot — **even if the working set is empty**.
3. Then (and only then) begin opening relay connections and dispatching subscriptions.

The first snapshot is unconditional. An app that observes the kernel will receive at least one snapshot before any network I/O completes. Subsequent snapshots refine that initial state as relay data arrives and reductions run.

This rules out, by construction:

- A bootstrap path that returns `Result<Kernel, TimedOut>` because a relay never answered.
- An `await first_snapshot` call that can hang on connectivity.
- A `pre-warm` step that the platform must complete before a view is allowed to render.

## 4. Flaky-relay policy

Relays are assumed adversarial in availability: any individual relay may be slow, disconnected, rate-limiting, policy-denying, or returning partial data at any moment. The framework degrades each failure mode silently to the next-best behavior:

- **Disconnected relay.** The view renders cached data from the store. Connectivity surfaces as an observable diagnostic field (per ADR-0007), never as a render gate.
- **Partial EOSE.** Views render whatever events arrived so far. EOSE is a coverage hint that updates a watermark; it is not a "now you may paint" signal.
- **Relay returns `policy_denied` / `auth_required` / `rate_limited`.** The kernel records the outcome for that `(relay, filter)` pair, skips that relay for the current subscription, and continues with the rest. No toast, no error dialog, no UI state change beyond the connectivity diagnostic.
- **Every configured relay fails.** The view still renders from the local store. The connectivity indicator goes to "offline"; rendering does not stop.
- **Slow relay.** Treated identically to "disconnected" until it responds. The framework never blocks waiting for a tardy relay to catch up.

This is the relay half of D1 + D7 (capabilities report; never decide policy): the relay-transport capability reports outcomes; the kernel decides what to do with them; the app code is never asked to handle "connectivity" as a first-class state.

## 5. The banned anti-pattern

The following string is a verbatim instance of a framework defect:

```
failed to bootstrap NmpGallery kernel: timed out waiting for live thread view
```

**Any** wait, timeout, or block on a view that depends on a relay response is forbidden. The class of error this string represents is banned by construction:

- No bootstrap path may include a timeout whose expiry indicates "the relays did not respond fast enough."
- No view may have a "ready" state distinct from "registered" — registration is sufficient to render.
- No app entry point may report `Err(TimedOut)` because of relay behavior.

A framework that emits this error class has confused refinement with precondition. Fix the kernel; do not raise the timeout.

## 6. App developer contract

App code **never** handles "waiting for connectivity" as a first-class state. The platform shell:

- Does not branch on `online` / `offline` before rendering content.
- Does not implement retry, reconnect, or backoff logic — those are framework substrate concerns.
- Does not consume a `Loading` variant from any view payload; view payload fields are non-`Option` and carry placeholders per D1.
- Does not implement timeouts around `dispatch`, subscription open, or snapshot subscription.

Connectivity health, if surfaced at all, is an **observable diagnostic** the shell may choose to display (e.g., a small banner). It is never an input to rendering decisions. If a future module needs "online" semantics for a specific feature (e.g., disabling a publish button while every relay is unreachable), that is expressed as a view payload field the module owns — not as a global app state.

This is a substrate burden: the framework MUST make "offline-first" the path of least resistance, and "blocks on network" impossible to type.

## 7. Test requirement

Every viewer-class app (gallery, reader, timeline shell) **MUST** have a smoke test that boots the kernel with **zero relay connectivity** and verifies that the first rendered frame is produced from local-store content alone.

The test must:

- Construct the kernel with an empty relay set (or a relay set whose endpoints reject connection).
- Drive the bootstrap path end-to-end.
- Assert that an initial `AppState` snapshot is emitted within a bounded budget that does not depend on network I/O.
- Assert that the first snapshot is consistent with the local store contents (empty store → empty-but-rendered views; populated store → cached content rendered).

For the gallery TUI, this is the existing `--smoke` headless mode plus a local-only fixture; any bootstrap path that fails this test is a framework defect, not a test-environment problem.

## 8. Relationship to existing doctrines

| Doctrine | Relationship |
|----------|--------------|
| D1 | This document is the operational elaboration of D1 for bootstrap and relay lifecycle. |
| D4 | The local event store is the single writer; views derive from it; relays are one of several inputs to the writer. |
| D6 | Connectivity outcomes surface as state fields and diagnostic records, never as exceptions across FFI. |
| D7 | Relay transport is a capability; it reports outcomes and never decides whether rendering may proceed. |
| D8 | Working-set projection is bounded by registered views, not by relay completion. |

## 9. Enforcement

- **Code review.** Any PR that introduces a wait, timeout, or block on relay state in a bootstrap, view-open, or snapshot-emit path is rejected on offline-first grounds.
- **Smoke gate.** The local-only smoke test (§7) is a required CI lane for every viewer-class app.
- **Diagnostic.** Per ADR-0007, the framework records each relay outcome; a viewer that fails to render with zero connectivity will show up as an empty render budget rather than as a user-visible error — that is the desired behavior.
