# RMP/NMP Hard Rules

Use this reference as the architectural source for NMP/RMP app work. These rules are intentionally strict. The point of the architecture is to make bad Nostr apps and bad multiplatform apps structurally hard to build.

## RMP Baseline

RMP starts from one idea: one Rust core owns behavior and each platform renders native UI.

- Rust owns state machines, policy decisions, business logic, validation, protocol behavior, transport, cryptography, persistence, networking, long-lived state, cross-platform invariants, routing decisions, and error semantics.
- Native owns rendering, native UX affordances, accessibility semantics, and bounded execution of OS capabilities such as Keychain, file pickers, camera, push, location, audio sessions, and secure storage.
- Data flow is TEA/Elm: `AppAction` enters Rust, one actor processes it, state changes, a snapshot/update is emitted, native applies it on the UI thread and renders.
- `dispatch()` is fire-and-forget. It must not block and must not return operation success. The acceptance outcome (a `correlation_id`) is not publish/operation success — terminal status arrives later as projected state. See `write-intents-and-publishing.md`.
- The actor is the single writer. Async work reports back through explicit internal events. Reducers do not await.
- Incremental, typed-projection emission is the steady-state default (ADR-0055): the transport omits unchanged projections and emits a `Changed` row carrying that projection's full current value only when its encoded bytes differ. Full snapshots are the cold-start / resync baseline, not every tick. The generic `payload:Value` JSON lane is deleted — all projections are typed FlatBuffers sidecars. See `projections-and-emission.md`.
- Native navigation may use native widgets, gestures, and transitions, but Rust owns navigation state.
- Apps must feel fully native: 60fps scrolling, instant touch response, platform-native navigation, platform-native accessibility, and no visible "Rust is involved" tax.

## NMP Evolutions Over RMP

NMP keeps the RMP skeleton and adds stricter doctrine and Nostr-specific correctness gates.

- RMP says "Rust owns business logic"; NMP makes this enforceable through D0–D27 (the doctrine-lint binary enforces A6, D0, D6–D27, action_namespace, no_raw_tap, product_raw_read — the older "D0–D10" framing is incomplete), the architecture scanner, bounded snapshots, and extension seams. See `doctrine-governance-and-enforcement.md`.
- RMP's early examples sometimes lower display strings into Rust. NMP refines this: Rust owns semantic state and invariant-bearing derived facts; platform rendering may own pure visual formatting such as truncation, typography, local date presentation, and layout labels when those choices do not affect behavior or protocol meaning. If a string affects policy, routing, sorting, identity, replay, tests, or cross-platform semantic parity, compute it in Rust.
- RMP permits full snapshots as a simple baseline. NMP requires snapshots to be bounded by open views and app chrome. The event store never crosses FFI.
- RMP allows native capability bridges. NMP requires them to report raw results only, never policy, retry, routing, cipher choice, or recoverability.
- RMP values performance. NMP treats performance as correctness: no polling, bounded working set, <=60 Hz per view, no hot-path allocation after warmup where D8 applies, no unbounded queues, no native jank.
- NMP adds Nostr-specific rules: outbox routing is automatic, negentropy-first history sync, injected kernel time, provenance, and private-event fail-closed behavior.

## The 2026 Redesign Spine (ADR-0069–0073) and Deep-Dive References

The clean-break app-architecture migration (EPIC-NS-001 / #2340) reshaped the app-facing
contract around three "doors" plus one native surface. These are now durable architecture, not
in-flight work. Each has a deep-dive reference in this directory — read the relevant one before
designing or reviewing in that area:

- **One native surface** — UniFFI is the sole public native ABI (C-ABI and `nmp-ffi` deleted);
  browser is wasm-bindgen; FlatBuffers ride *through* UniFFI. Apps own a single UniFFI facade
  for app-specific verbs → `ffi-and-native-surface.md`.
- **Explicit composition** — `register_defaults()` and `nmp-defaults` are deleted; the app root
  installs `nmp_substrate::install(...)` plus named owner-crate protocol/runtime installers
  explicitly → `composition-and-product-policy.md`.
- **The read door** — typed read sessions own the read lifecycle; `open_interest` /
  `ObservedProjection` / `ReducedSource` are private substrate → `read-sessions.md` and
  `projections-and-emission.md`.
- **The write door** — typed publish intents, composable unsigned drafts, mandatory typed route
  provenance, dispatch ≠ success → `write-intents-and-publishing.md`.
- **Runtime / capability / shell boundary** — the three-tier runtime stack, the capability port
  contract, and what a native shell may and may not own → `runtime-capability-shell-boundary.md`.
- **Crate layers** — the L0–L6 layer model and the layer-inversion rule (no display/render/
  app-noun/aggregation in L0–L4) → `crate-layers-and-inversion.md`.
- **Protocol-crate purity** — D0 is `nmp-core`-scoped; protocol crates own one mechanism;
  NIP-29 is a kind-blind transport with one generic publish door →
  `protocol-crates-and-kind-blind-transport.md`.
- **Governance** — the ADR ledger (Current/Amended/Folded/Retired), rolling ratchets, the
  two-tier waiver model, and how doctrine-lint and the scanner divide labor →
  `doctrine-governance-and-enforcement.md`.

## D0-D10 In One Page

Resolve conflicts in order: D0 outranks D1, D1 outranks D2, and so on. D0–D10 are the
publicly enumerated baseline; doctrine-lint enforces through D27 (see
`doctrine-governance-and-enforcement.md` for the full table).

- D0: The framework core knows nothing about any app domain. No app nouns in `nmp-core`. The doctrine-lint banned-token gate is scoped to `nmp-core`, but the **layer-inversion rule extends the spirit of D0 to every sub-L5 crate**: no render/display/app-noun/aggregation concern may leak into L0–L4 (storage, transport, planner, kernel, protocol crates). Protocol crates may name their own protocol nouns; they may not carry render-cards, display strings, or foreign-NIP semantics. App and protocol modules contribute typed variants through seams. Business logic does not move to Swift/Kotlin/TS to avoid Rust boundaries. See `crate-layers-and-inversion.md`.
- D1: Render now, refine in place. Do not hide renderable content behind loading gates. View payloads carry values or typed placeholders, not "wait for profile" status.
- D2: History syncs by diff, not re-download. Historical backfill uses negentropy/NIP-77 coverage gates where supported; raw REQ scans are not the default.
- D3: Relay routing is automatic. App-facing send, publish, and view-open surfaces do not accept relay URLs. Manual relay selection is an audited opt-out.
- D4: One source of truth. Exactly one writer owns each fact; downstream caches and views derive mechanically. No app-side cache mirrors the Rust state.
- D5: Only what is on screen crosses FFI. Snapshots are small, screen-shaped, and scoped to open views. The event store, history, watermarks, signer state, and gossip cache stay inside Rust.
- D6: Errors show up in state, not exceptions. No `Result<T,E>`, thrown exception, panic, or typed per-op error crosses FFI. Failures clear busy flags and surface as toast, diagnostic, or action-stage state.
- D7: Native bridges execute; the kernel decides. Native never decides retry, recoverability, relay, cipher, routing, next state, or fallback policy. Capability lifecycles are idempotent.
- D8: Reactivity is bounded. No polling. No false wake loops. No per-event hot-path allocation after warmup. No view emits above 60 Hz. Memory scales with active views, not event history.
- D9: The kernel owns time. Reducers and replay paths use an injected clock. Replaceable resolution, expiration, and publish timestamps are kernel decisions, not relay claims or native wall-clock reads.
- D10: Private messages stay private. Gift-wrap/private events target verified recipient inbox relays only; unknown inbox fails closed. Received private events are never laundered to public relays.

## Clean Architecture Absolutes

- Do the architecture-correct fix. Do not paper over a boundary failure with a local shortcut.
- Do not add temporary hacks, stubs, planned TODO debt, parallel paths, or duplicate representations.
- Do not leave two code paths for the same concept. Delete or migrate the old one in the same change unless a canonical staged plan already exists.
- Do not add a native cache, native state machine, native retry loop, or native data derivation because plumbing Rust state feels tedious.
- Do not introduce broad "manager" or "service" objects that own facts already owned by the actor.
- Do not split TEA by technical role into global `model/`, `update/`, `view/`, `state/`, or `actions/` buckets. Co-locate by feature/domain owner and split by cohesive subdomain when files grow.
- Do not create unbounded queues, unbounded snapshots, unbounded in-memory history, or unbounded cross-FFI payloads.
- Do not let tests pass by weakening doctrine, widening an escape hatch, or adding a special test-only production path.

## Performance Absolutes

Pristine, performant applications are mandatory. Performance is not a later polish phase.

- Native UI must remain responsive and platform-correct: 60fps scroll and transitions, instant taps, no avoidable main-thread blocking.
- Rust hot paths must be budgeted before implementation. Name the steady-state allocation, wakeup, and memory bounds.
- FFI update cadence must be bounded and coalesced. High-frequency data must not serialize one update per event/frame unless intentionally designed and measured.
- Use blocking/event-driven primitives, not polling. `sleep` plus checking state is a violation in Rust, Swift, Kotlin, TypeScript, tests, and background jobs.
- If a feature can firehose events, profiles, reactions, messages, or media frames, design the reverse index, dependency declaration, coalescing, and eviction path before writing UI.
- A performance concern is not a license for native caching. Optimize the Rust projection/update contract or use a lossless delta after profiling.

## Capability Bridge Rules

- Rust requests the capability.
- Native executes the OS API.
- Native reports raw data or raw failure.
- Rust decides what it means.
- Rust owns retries, fallbacks, state transitions, user-visible messages, and teardown.
- Native holds only transient OS handles or buffers.
- Start, stop, and restart must be safe when called repeatedly.
- Secrets may cross only through explicit secret-bearing side-effect updates or secure capability channels; never embed them in normal snapshots, logs, action tags, or debug history.

## Scanner Precision And Canonical Allowances

The architecture scanner (`scripts/nmp_architecture_scan.py`) is triage, not proof. These canonical allowances tell it — and reviewers — what is a real violation versus a legitimate boundary. Apps that hit these cases stay thin showcase clients; they do not add local suppression hacks or duplicated config.

- App/operator relay policy is the audited D3 opt-out, not a routing bypass. An app declares its bootstrap/default relay set once, in an app/operator config surface (a `*-config` crate, a `config`/`defaults`/`relay_policy` module). That declaration is operator policy and is allowed. D3 stays a hard error only where a relay URL appears at an ordinary send/publish/view-open call site, because that bypasses automatic outbox routing. A relay URL in a native placeholder/prompt display string is presentation, not routing. The scanner reports the config-surface and display-string cases as warnings (verify they are declarations/strings, not routing), and the call-site case as an error.
- Generated files are derived, never canonical. A file marked generated (`*.generated.*`, or a header such as `@generated` / `DO NOT EDIT` / `Code generated by`) is a mechanical projection of a canonical source. Review and fix the canonical source; the generated artifact is skipped. Editing generated output instead of its source is itself a D4 violation.
- One-shot presentation timers are not polling. A single-fire `Task.sleep`, `DispatchQueue.asyncAfter`, `setTimeout`, or `Timer.scheduledTimer(repeats: false)` used by the native shell for toast dismissal, copy feedback, focus, or initial scroll is bounded presentation behavior and is allowed. Polling — and a D8 hard error — is a repeating timer (`setInterval`, `Timer.scheduledTimer(repeats: true)`), a sleep inside a loop, a `sleep`+`try_recv` recheck, or any `thread::sleep` on a reactive/production path. One-shot timers must never drive domain state or re-arm a loop; that turns them into polling.
- FFI-boundary error handling versus native OS error handling. D6 forbids errors crossing the FFI boundary as exceptions, panics, or typed `Result`/`throws`. Ordinary native `try`/`catch`/`throws` around OS and capability APIs (Keychain, file pickers, networking) is normal and not a D6 hit. The scanner flags D6 only at the FFI boundary (near `#[uniffi::export]`, `extern "C"`, the UniFFI bindings, or an `ffi`/`bindings` path); pure native OS error handling is left alone.

## Documentation And Source Of Truth

- Product corrections may require source-of-truth updates, not just code changes.
- Durable rules belong in product specs, architecture/design docs, ADRs, or builder guides.
- Temporal work belongs only in canonical planning/status files used by the repo.
- If an implementation discovers a new invariant, document the invariant in the durable owner in the same PR.
- If a doctrine needs an exception, write an ADR. No ADR means no waiver.
