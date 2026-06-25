# Chirp Web — wasm runtime EXECUTION plan

> Status: plan (2026-06-12). Companion to
> [`wasm-mvp-build-plan.md`](wasm-mvp-build-plan.md) (PR-1…PR-7 feature
> sequence). That plan exposes capabilities; **this plan proves the runtime
> actually executes in a browser**, because today it never has. Every claim
> below was verified against the tree on 2026-06-12; §5 lists what could not
> be verified.

---

## 0. The verified problem

1. **CI never executes the wasm.** `.github/workflows/chirp-web.yml` runs
   `cargo test -p nmp-wasm` (native host) and `cargo check -p nmp-wasm
   --target wasm32-unknown-unknown` (with `CC_wasm32_unknown_unknown: clang`)
   — a type-check, not an execution. `npm run test` is vitest with
   `environment: "node"` (`web/chirp/vite.config.ts`) and exercises only the
   `DegradedRuntime` + frame decoders against synthetic Tier-3 FlatBuffers
   frames built in the test. No
   Playwright config exists anywhere in the repo (only prose mentions under
   `web/registry/docs/wiki/`).

2. **The only wasm binary ever loaded in a browser is a 3-week-old stub.**
   The worker loads `/nmp-wasm/nmp_wasm.js` (`web/chirp/src/nmp/wasmBridge.ts:3`)
   from a **checked-in** artifact at `web/chirp/public/nmp-wasm/` whose last
   commit is `48f63ba4` (PR #263, 2026-05-24, `nmp_wasm_bg.wasm` = 217 KB) —
   built **before** the Stage-3 relay transport, the Stage-3b/3c signer +
   publish paths, and the PR-1 kernel-authored snapshot. `web/chirp/README.md:69`
   documents a *manual* `wasm-pack build` refresh; neither CI nor Vercel
   (`web/chirp/vercel.json` → `npm run build`) ever rebuilds it.

3. **A current build would panic at `Start`.** On `wasm32-unknown-unknown`,
   `std::time::Instant::now()` **and** `std::time::SystemTime::now()` abort
   (upstream `std` behavior: "time not implemented on this platform"). The
   kernel calls both on every hot-path leg (§1). The very first panic fires
   inside `WasmRuntime::start` → `reduce(KernelAction::Start)` →
   `Kernel::start()` → `Instant::now()` at
   `crates/nmp-core/src/kernel/mod.rs:2712`. Boot dies before the first
   snapshot.

4. **Silent-green trap.** When the wasm module is missing/broken, the worker
   silently degrades (`web/chirp/src/nmp/worker.ts:48-66` →
   `DegradedRuntime`), which still answers `start` with
   `{degraded: …}` status (`web/chirp/src/nmp/degradedRuntime.ts:17-25`). A
   naive smoke that only checks "page rendered" would pass against the stub.
   Every smoke below must assert the **real** runtime: `runtime_status:
   "running"` (only the real kernel emits it — `crates/nmp-wasm/src/runtime.rs:274-280`)
   and the absence of the `wasm_bridge_unavailable` startup error.

Conclusion: PR-1 (snapshot, landed) and PR-2 (tick, #1140 in review) are
stacking on a runtime that has provably never run. #1140's own PR body
already deferred `poll_claim_expansion` parity because "`std::time::Instant`
(wasm32-incompatible without a polyfill)".

---

## 1. Panic-surface inventory (verified, non-test code only)

### `Instant::now()` on the wasm hot path

| Leg | Site | When it fires |
|---|---|---|
| **Boot/Start** | `kernel/mod.rs:2712` (`start()` → `timing.started_at`) | first `KernelAction::Start` — **the boot killer** |
| Boot/Start | `kernel/requests/startup.rs:34` (`startup_requests` → `contacts_deadline`) | account-set / startup REQs |
| **Snapshot** | `kernel/update.rs:213` + `:238` (`make_update` emit/serialize timing) | every snapshot frame (`:321`/`:365` are `#[cfg(test)]`) |
| Snapshot | `kernel/status.rs:377` (`elapsed_ms`) | consumes stored `Instant`s while building status rows |
| REQ open | `kernel/mod.rs:2701` (`insert_wire_sub` → `opened_at`) | every wire subscription |
| Relay connected | `kernel/relay_transport.rs:169`; `kernel/requests/relay_lifecycle.rs:50` | every `handle_relay_connected` |
| Inbound frame | `kernel/ingest/mod.rs:371` (EVENT counters/timing), `:184` (EOSE) | every relay EVENT/EOSE |
| Event refs | `kernel/requests/event.rs` (`register_claim_expansion`) | every event `resolve_ref` dispatch through the wasm ref router |
| Diagnostics | `kernel/relay_diagnostics.rs:240` | `nmp.kernel.diagnostics` dispatch |
| Contact feed | `kernel/ingest/timeline.rs:227`, `:312`, `:336` | the exact code paths PR-3/PR-4 light up |

### `SystemTime::now()` (panics identically on wasm32-unknown-unknown)

- `kernel/clock.rs:31-33` — `SystemClock`, the **default** kernel clock
  (`kernel/mod.rs:2090`), feeding `now_ms()` (`kernel/mod.rs:2245`) →
  `make_update`'s `last_tick_ms` and ingest `received_at_ms`. The `Clock`
  trait exists but is `pub(crate)`, deliberately scoped to
  business-logic reads only (`clock.rs:9-13`: "`Instant::now()` reads …
  stay as direct calls"), and **not injectable** through `KernelReducer`
  (no `set_clock` in its public surface, `crates/nmp-core/src/kernel_reducer.rs`).
- `kernel/routing_trace.rs:197` — stamped on routing decisions (claim REQs).
- `kernel/publish_engine_wire.rs:89` — `now_epoch_ms()` →
  `tick_publish_engine_for_now()` → `KernelReducer::tick()`
  (`kernel_reducer.rs:186`). **#1140's 1 Hz timer panics every second**
  even if boot survived.
- `kernel/nostr.rs:191-194` `now_hms` — `#[cfg(feature = "native")]`, safe.
- `crates/nmp-signers/src/signers/nip46/mapper.rs:226` — NIP-46 only, not on
  the MVP wasm path (NIP-07 is), but the same one-line fix applies.

`js_sys::Date::now()` is already used once, in the wasm publish bindings
(`crates/nmp-wasm/src/lib.rs:203`) — precedent for JS-backed wall time.

### Not problems (verified)

- **getrandom** — only `getrandom v0.2.17` is in the wasm tree
  (`cargo tree -p nmp-wasm --target wasm32-unknown-unknown -i getrandom@0.2.17`;
  0.3.4/0.4.2 do not match any package on that target). The `js` feature is
  enabled **by the `nostr` crate itself** (feature tree: `getrandom feature
  "js" ← nostr v0.44.3`), backend = `crypto.getRandomValues`, available in
  Workers. No `.cargo/config.toml` exists and none is needed.
- **Threads / file I/O** — `nmp-core` is consumed `default-features = false`
  (`crates/nmp-wasm/Cargo.toml`); `actor/` + `relay_worker/` are
  `native`-gated; `nmp-store`'s LMDB backend is off by default
  (`crates/nmp-store/Cargo.toml`: `default = []`), so only `MemEventStore`
  compiles in.
- **Reconnect timers** — `BrowserRelayDriver` schedules via `setTimeout`
  closures, no `Instant` (`crates/nmp-network/src/browser_driver.rs:31,53,148-150`).

### Missing panic visibility

`console_error_panic_hook` appears nowhere in `Cargo.lock` or the crates. A
wasm panic today surfaces as an opaque `RuntimeError: unreachable` swallowed
into the worker's generic `worker_exception` error event
(`web/chirp/src/nmp/worker.ts:38-45`).

---

## 2. The fix decision: `web-time`, not a clock fork

**Recommended: (a) `web-time` shim.** Add `web-time = "1.1"` (version 1.1.0
is *already* in `Cargo.lock` via egui/iced/winit — zero new dependency
versions) as a `[target.'cfg(target_arch = "wasm32")'.dependencies]` of
`nmp-core` (and `nmp-signers`), with one internal re-export module:

```rust
// crates/nmp-core/src/time.rs (pub(crate))
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub(crate) use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};
```

On native, `web-time` re-exports `std::time` — the types are *identical*, so
the actor/native paths are byte-for-byte unchanged. On wasm32, `Instant` is
`performance.now()`-backed and `SystemTime` is `Date.now()`-backed (both
available in Worker scope). The mechanical churn is small because most kernel
modules already re-import `Instant` via `super::Instant` from the single
`use std::time::{Duration, Instant};` at `kernel/mod.rs:280`; the only direct
imports to swap are `kernel/mod.rs:280`, `claim_expansion.rs:43`,
`relay_diagnostics.rs:30`, `clock.rs:15`, `routing_trace.rs:33`,
`publish_engine_wire.rs:88`, the fully-qualified call at
`requests/event.rs:288`, and `nmp-signers/…/nip46/mapper.rs:224-226`.

**Rejected: (b) routing all time through an injectable clock.** The existing
`Clock` trait covers only deterministic-replay `SystemTime` reads; extending
injection to ~15 monotonic `Instant` sites means threading a clock handle (or
a `now` parameter) through `make_update`, `insert_wire_sub`, ingest, claims,
and the public `tick()`/`handle_relay_frame` signatures — forking the call
shapes the native actor uses and contradicting the doctrine that the kernel
stays single-source. The `web-time` swap keeps ONE code path with a
target-conditional `use`; the `Clock`/`FixedClock` replay seam is untouched
(`SystemClock` simply reads the shimmed `SystemTime`, which on native is
`std`'s).

Regression guard: the repo already runs a doctrine lint
(`crates/nmp-testing/bin/doctrine-lint/`, smoke at `--test
doctrine_lint_smoke`). Add a rule forbidding direct `std::time::Instant` /
`std::time::SystemTime` in `nmp-core`, `nmp-wasm`, `nmp-signers`, and the
non-native slice of `nmp-network`, allowlisting `time.rs` and
`#[cfg(feature = "native")]`/test files.

---

## 3. Ordered PR sequence

### PR-W1 — **URGENT: wasm-safe time + panic hook + first EXECUTED wasm test**

Nothing else in this program is real until this lands. Land before #1140's
follow-ups and before PR-3/PR-4 acceptance is trusted.

**What it builds**

- `crates/nmp-core/src/time.rs` shim + the import swaps listed in §2;
  `web-time` target-dep in `crates/nmp-core/Cargo.toml` and
  `crates/nmp-signers/Cargo.toml`.
- `console_error_panic_hook` as a wasm32 dep of `nmp-wasm`, installed once in
  `NmpWasmRuntime::new()` (`crates/nmp-wasm/src/lib.rs:64-69`) so any future
  panic reaches the browser console / Playwright trace instead of an opaque
  `unreachable`.
- **The first test that RUNS the wasm**: `crates/nmp-wasm/tests/wasm_boot.rs`
  using `wasm-bindgen-test` (dev-dep, `wasm_bindgen_test_configure!(run_in_browser)`):
  construct `NmpWasmRuntime`, `handle_json(Hello{protocol_version:1})` →
  assert `hello_accepted`; `handle_json(Start{app_id:"chirp",
  database_name:"t", relays:["ws://127.0.0.1:1"]})` → assert
  `runtime_status: "running"` **and** that the snapshot callback received
  decodable `UpdateFrame` bytes with a relay row carrying the configured URL
  (the kernel emits real rows since PR-1's `set_configured_relays`,
  `crates/nmp-wasm/src/runtime.rs:255-260`). The non-routable relay URL makes
  the driver's `onerror`/reconnect path run too — assert no panic, honest
  `connecting`/`failed` row state.
- CI: new step in `.github/workflows/chirp-web.yml` —
  `wasm-pack test --headless --chrome crates/nmp-wasm` (keep
  `CC_wasm32_unknown_unknown: clang`; pin wasm-bindgen-cli compatible with
  the `wasm-bindgen = "0.2.121"` pin in `crates/nmp-wasm/Cargo.toml`).
  ubuntu-latest ships Chrome.

**Acceptance** — the headless `wasm-bindgen-test` above passes in CI
(executes the actual `.wasm` in a real Chrome), plus `cargo test -p nmp-core
-p nmp-wasm` native and the doctrine-lint rule from §2 (can ride here or as a
trailer PR).

**Honest degraded behavior** — with an unreachable relay the runtime stays
up: rows show `connecting`→reconnect backoff, snapshots keep flowing. Note
the limitation honestly: `wasm-bindgen-test` runs in the page context, not a
Worker; Worker-context execution is proven by PR-W3.

**Dependencies / collisions** — none upstream. vs **#1140 (PR-2 tick)**:
disjoint logic; #1140 touches `runtime.rs`/`kernel_reducer.rs`/`protocol.rs`,
W1 touches kernel import lines + `lib.rs` + workflow — low risk, but
coordinate the `chirp-web.yml` edit. vs **PR-3/4 (feed)**: W1 edits import
lines in `ingest/timeline.rs`/`kernel/mod.rs` that PR-3/4 also touch —
trivial merge conflicts only. **W1 should land FIRST**: PR-3/4 light up
`ingest/timeline.rs:227,312,336` and `requests/event.rs:288`, i.e. they add
*more* panic sites to the unexecuted surface; and #1140's 1 Hz tick →
`publish_engine_wire.rs:89` panics every second without W1.

---

### PR-W2 — Build the wasm for real; delete the stale artifact

**What it builds**

- `npm run build:wasm` script in `web/chirp/package.json` wrapping the
  documented command (`web/chirp/README.md:69`): `wasm-pack build
  ../../crates/nmp-wasm --target web --out-dir ../../web/chirp/public/nmp-wasm
  --out-name nmp_wasm`. Delete the checked-in May-24 artifact; gitignore
  `web/chirp/public/nmp-wasm/`.
- `chirp-web.yml`: install wasm-pack, run `build:wasm` **before**
  `npm run build`, with the existing cargo cache. The vite `dist/` now ships
  a wasm built from the same SHA as the Rust crates.
- A guard so the silent-degrade trap can't return: a vitest (or build-step
  assertion) that `public/nmp-wasm/nmp_wasm.js` exists and exports
  `NmpWasmRuntime` before `vite build` is allowed to succeed.
- Vercel: `vercel.json` `buildCommand` must become
  `npm run build:wasm && npm run build` with a Rust toolchain installed in
  `installCommand` (rustup + wasm32 target + wasm-pack), **or** the deploy
  consumes a CI-built artifact. This is a deploy-infrastructure decision —
  flag it in the PR; do not let the web deploy silently keep the stub.

**Acceptance** — CI green with the artifact deleted (proves the build step,
not a stale copy, feeds `dist/`); PR-W3's smoke is the runtime proof. Until
W3 lands, state honestly in the PR body that this PR only guarantees a fresh
*build*, not execution.

**Dependencies / collisions** — needs W1 (a freshly built wasm without W1
panics at `Start`, which would make the deployed app *worse* than the stub —
this is why W2 must not land first). Web-only + workflow; no collision with
PR-3/4/#1140.

---

### PR-W3 — Playwright harness + fixture relay + boot smoke in CI

**What it builds**

- `@playwright/test` devDep, `web/chirp/playwright.config.ts`: chromium
  project, `webServer` = `vite preview` serving the built `dist/`
  (build:wasm + build as pre-step). `npm run test:e2e`.
- **Fixture relay** — `web/chirp/tests/fixtures/fixtureRelay.ts`: a ~100-line
  Node `ws` server speaking minimal NIP-01: `REQ` → canned `EVENT`(s) +
  `EOSE`; `EVENT` → `OK`; `CLOSE` → `CLOSED`. Started per-test-run on
  `127.0.0.1:0`. This is the lightest honest option: the repo's `StubRelay`
  (`crates/nmp-testing/tests/common/stub_relay.rs`) only drops connections,
  and the real-relay suites (`crates/nmp-testing/tests/real_relay_*.rs`,
  `.github/workflows/real-relay-nightly.yml`) depend on public-relay uptime —
  wrong for a merge-blocking smoke. (A public-relay variant can ride the
  nightly later.)
- **Relay override hook** — relay policy is host policy, not framework default
  (#1125): the web client always sends explicit `relays` + `relay_bootstrap` on
  `start` (`chirpStartRelays` in `web/chirp/src/nmp/client.ts`). The Chirp web
  defaults (`wss://relay.primal.net`, `wss://purplepag.es`) live in
  `web/chirp/src/chirpConfig.ts` (mirrors the Rust source of truth
  `apps/chirp/crates/nmp-chirp-config/src/lib.rs`). A `?relay=<url>` URL-param injected
  via `client.start(relays)` overrides those defaults. Honest and dev-useful;
  no serde defaults and no test-only forks in Rust.
- **Boot smoke** (`web/chirp/tests/e2e/boot.spec.ts`): navigate to
  `/?relays=<fixture>`; the app auto-starts (`web/chirp/src/App.tsx:28`).
  Assert, in order: (1) **no** `wasm_bridge_unavailable` / `worker_exception`
  error event surfaced (kills the silent-degrade trap, §0.4); (2) the
  RuntimePanel shows the real running status
  (`web/chirp/src/features/RuntimePanel.tsx:17`) — only the wasm kernel emits
  `"running"`; the degraded stub emits `{degraded:…}`; (3) a relay row for
  the **fixture URL** with status `connected`
  (`RuntimePanel.tsx:33`) — this value travels relay socket → kernel →
  FlatBuffers `UpdateFrame` → worker → decoder → DOM, so its presence proves
  a real decoded snapshot frame end-to-end, in a Worker, per ADR-0047
  (`docs/decisions/0047-browser-worker-runtime.md` §1).
- CI: `chirp-web.yml` job step (or sibling job) — `npx playwright install
  chromium --with-deps`, `npm run test:e2e`, upload trace on failure.

**Acceptance** — the smoke passes headless in CI against the freshly built
wasm; deliberately breaking it (e.g. deleting the wasm artifact) makes it
fail on assertion (1), not pass.

**Dependencies / collisions** — hard on W1 + W2. Web-only; zero collision
with PR-3/4/5.

---

### PR-W4 — Inbound-event smoke: a real Nostr event through the kernel

**What it builds** — extends the fixture relay with a signed kind:0 fixture
event; the test dispatches the existing claim verb
(`nmp.claim_profile` — already wired through
`crates/nmp-wasm/src/runtime.rs:391-427` and `KernelReducer::claim_profile`)
for the fixture author, the fixture answers the resulting `REQ` with the
kind:0 `EVENT` + `EOSE`, and the test asserts the resolved profile appears in
the UI / typed projection (`nmp.profile.resolved` + `claimed_profiles` ride
the Tier-2 sidecar since PR-1). This proves: outbound REQ compile + fan-out,
inbound EVENT ingest (`kernel/ingest/mod.rs:371` — a W1 panic site), event-id
verification (exercising getrandom/secp in-browser), projection emission, and
frame decode — the full read loop.

**Acceptance** — smoke passes in CI; assert `events_rx > 0` on the relay row
as a secondary signal.

**Dependencies / collisions** — needs W3. Independent of PR-3/PR-4 at the
code level (claims are not the feed path), but **W4 is the template for
PR-4's own acceptance**: once `nmp.feed.home` exists, the same harness
asserts a feed item from the fixture relay.

---

### PR-W5 (small, anytime after W1) — doctrine lint: no direct `std::time` in wasm-reachable crates

The grep-rule described in §2, in `crates/nmp-testing/bin/doctrine-lint/`
with fixtures + smoke coverage, so `Instant::now()` can never silently
re-enter the hot path. (Fold into W1 if reviewers prefer.)

---

## 4. Where this slots vs the MVP feature sequence

```
W1 (time fix + first executed wasm test)   ← LAND FIRST, unblocks everything
 ├─→ #1140 / PR-2 tick: merge after or rebase on W1 (its 1 Hz tick calls
 │    tick_publish_engine_for_now → SystemTime::now(), kernel/publish_engine_wire.rs:89;
 │    its deferred poll_claim_expansion parity is unblocked by web_time::Instant)
 ├─→ W2 (real wasm build, delete stale artifact)
 │     └─→ W3 (Playwright + fixture relay + boot smoke)
 │           └─→ W4 (inbound-event smoke)
 └─→ PR-3 / PR-4 / PR-5 (feed work) — may proceed in PARALLEL code-wise
      (only import-line conflicts), but their "it works" claims are only
      honest once W3/W4 run them in a browser; PR-4's acceptance should
      reuse the W3/W4 harness.
```

Does the Instant fix need to come before PR-3/4/5? **Strictly: yes for
honesty, not for compilation.** PR-3/4 compile fine without W1, but they
specifically activate `ingest/timeline.rs:312,336` and the claim-expansion
tracker (`requests/event.rs:288`) — i.e. they widen the panic surface of a
runtime that cannot boot. Landing W1 first is one small PR and removes the
need to ever reason about this again.

## 5. Flagged as unverified

- The `Instant::now()`/`SystemTime::now()` panic on `wasm32-unknown-unknown`
  is upstream `std` behavior (documented; `web-time`'s raison d'être), not
  reproduced in this repo — precisely because nothing here can execute wasm
  yet. PR-W1's test is the in-repo proof.
- PR #1140's review comments were not retrievable via `gh` (only the PR body,
  which itself documents the Instant incompatibility); the "#1140 review"
  framing comes from the task brief.
- No local `wasm-pack build` was run during this investigation (read-only);
  the wasm-bindgen-cli ↔ `wasm-bindgen = "0.2.121"` version-match note in
  W1/W2 should be validated when the CI step is written.
- Vercel build-image Rust availability (W2's deploy note) was not tested.
