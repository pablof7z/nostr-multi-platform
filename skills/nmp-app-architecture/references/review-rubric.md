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

Redesign-spine blocking findings (2026 clean-break — see the deep-dive references):

- Production app root calls `register_defaults()` or recreates a defaults bundle, or product
  policy (relay defaults, seed follows, onboarding) lives in a shared NMP crate.
- Product screen or app-core crate opens a raw `open_interest` / assembles
  `ObservedProjection`/`ReducedSource` directly instead of a typed read session.
- Shell treats dispatch acceptance (`correlation_id` non-null) as terminal publish success.
- `PublishTarget::Explicit` is used without a typed `PublishRouteClass`, or app/native code
  composes, signs, or routes outside the actor pipeline.
- A new `pub extern "C"` appears in a framework crate (`crates/nmp-*`) without an ADR-0030
  exception; or a native UniFFI facade copies runtime-bridge policy instead of delegating to
  `nmp-uniffi-support` / `nmp-native-runtime`.
- A render/display/app-noun/aggregation concern leaks into an L0–L4 crate: display fields
  outside `ProfileProjection`, engagement aggregation in storage, a render-card/feed-item type
  in a NIP crate, or a typed NIP-NN codec in `nmp-core`.
- A `nmp-nip*` transport crate owns a kind-named action, a foreign-NIP kind literal, or imports
  another `nmp-nip*` crate.
- A headless/OS surface (AppIntent, widget, CarPlay, Live Activity, share extension) owns
  product-queue state, signer state, relay policy, or publish-result models.
- Browser durable mode is claimed without a real Worker/OPFS proof.
- A wrong/stale doc is "fixed" by adding a superseding ADR instead of editing the owner in
  place; or an architecture slice lands new surface without a deletion ledger / flat-or-down
  ratchet.

## Design Questions To Answer

For every new feature:

- What fact is being added, and who is its single writer?
- What app action or internal event introduces each state transition?
- Which nondeterministic inputs exist, and how do they enter replayable Rust state?
- What typed read session owns this read demand, what is its bounded typed output, and what is the teardown path when the view closes?
- For a write: what is the typed intent, who finalizes/signs it (the actor, not the shell), and what route provenance does it carry?
- What native code is present, and is it only rendering or capability execution?
- At which layer (L0–L6) does each new type live, and does any display/render/app-noun/aggregation concern sit below L5?
- Which doctrines (D0–D27) are touched, and is each enforced by doctrine-lint, the scanner, or a new gate?
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
- UniFFI interface change: `bash ci/check-uniffi-bindings-drift.sh`; app-facade change: `uniffi-bindgen generate --library` for Swift and Kotlin against the app cdylib (Rust compile alone does not prove the generated native namespace).
- UniFFI byte-transport change: `ffi-transport-bench --standard --fail-on-gate`.
- Reactivity/hot-path/snapshot/view update change: run the project reactivity/performance bench with fail-on-gate.
- Native shell rendering: build and visually verify the platform path; use screenshots or browser/simulator checks where available.
- Capability bridge: test idempotent start/stop/restart, raw result reporting, failure reporting, teardown, and no native policy.
- Privacy/routing/signing: test fail-closed behavior and absence of hardcoded relay or recipient fallback paths.
- Triage scan (any NMP/RMP app, including external consumers): `python3 scripts/nmp_architecture_scan.py <root>` — investigate every hit; it complements doctrine-lint, it does not replace it.

## Common False Comforts

- "It is just UI code." UI code can still smuggle product policy.
- "It is only a cache." A second writer is a D4 violation even when it is called a cache.
- "It only runs every second." A timer-based state check is polling.
- "We need this for performance." Performance fixes still need one source of truth and bounded FFI.
- "The shell already knows this." If the shell knows product semantics, the boundary is wrong.
- "A later PR will clean it up." Later cleanup is not an architectural gate.
