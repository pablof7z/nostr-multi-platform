# NMP/RMP Architecture Review Rubric

Use this rubric for PR review, app scaffold review, migration review, or pre-implementation design review.

## Output Format

Lead with findings. Order by severity.

```text
Findings
- BLOCKING path:line - Rule violated. Why it is wrong. Required architectural fix.
- HIGH path:line - Risk and fix.
- MEDIUM path:line - Risk and fix.

Open Questions
- Only questions that materially affect architecture or verification.

Verification
- Gates run and results.
- Gates not run and why.
```

If there are no findings, say so directly and list remaining test or performance risk.

## Blocking Findings

Block the change for any of these:

- Native shell decides product behavior, protocol behavior, retry, routing, privacy, cache invalidation, time, signing, or state transitions.
- Any polling loop or timer-based state query is introduced.
- A second source of truth is introduced for a fact already owned by Rust.
- App nouns or product-specific behavior leak into shared framework substrate.
- Event store, history, signer internals, relay watermarks, or unbounded data cross FFI.
- Errors cross FFI as exceptions, typed results, panics, or native recovery policy.
- Capability bridge stores policy state or is not idempotent.
- Relay URL appears in app-facing send/publish/view-open surface.
- Private/gift-wrap publish can fall back to public or recipient-unknown relay sets.
- Reducer/replay path reads raw wall clock instead of injected clock.
- Hot path allocates or wakes broadly where a bounded D8 path is required.
- Temporary hack, TODO debt, stub, duplicate path, or "fix later" workaround remains.
- Performance is unmeasured for a change that affects snapshots, projections, queues, FFI cadence, scrolling, media, or event firehose behavior.

## Design Questions To Answer

For every new feature:

- What fact is being added, and who is its single writer?
- What app action or internal event introduces each state transition?
- Which nondeterministic inputs exist, and how do they enter replayable Rust state?
- What projection crosses FFI, and why is it bounded by open views or app chrome?
- What native code is present, and is it only rendering or capability execution?
- Which D0-D10 doctrines are touched?
- What tests or benches prove the rule is enforced?

For each platform shell:

- Does the shell render from Rust state without deriving product policy?
- Does back navigation dispatch state intent back to Rust instead of mutating hidden native state?
- Are native caches limited to UI framework mechanics, not app facts?
- Are OS callbacks converted into typed actions/capability results?
- Are accessibility and platform-native interactions preserved?

For each Rust module:

- Is the module owned by a cohesive domain/protocol/app feature rather than a technical bucket?
- Does the crate/module root have an accurate doctrine map when it touches doctrine?
- Are public APIs typed so misuse is hard or impossible?
- Are escape hatches named, gated, instrumented, and tested?

## Verification Checklist

Minimum verification depends on the touched surface:

- Rust app/core logic: scoped `cargo test -p <crate>` plus relevant downstream consumers.
- NMP doctrine-sensitive change: `cargo test -p nmp-testing --test doctrine_lint_smoke`.
- Public symbol, module move, or dependency path change: `cargo build --workspace`.
- Reactivity/hot-path/snapshot/view update change: run the project reactivity/performance bench with fail-on-gate.
- Native shell rendering: build and visually verify the platform path; use screenshots or browser/simulator checks where available.
- Capability bridge: test idempotent start/stop/restart, raw result reporting, failure reporting, teardown, and no native policy.
- Privacy/routing/signing: test fail-closed behavior and absence of hardcoded relay or recipient fallback paths.

## Common False Comforts

- "It is just UI code." UI code can still smuggle product policy.
- "It is only a cache." A second writer is a D4 violation even when it is called a cache.
- "It only runs every second." A timer-based state check is polling.
- "We need this for performance." Performance fixes still need one source of truth and bounded FFI.
- "The shell already knows this." If the shell knows product semantics, the boundary is wrong.
- "A later PR will clean it up." Later cleanup is not an architectural gate.
