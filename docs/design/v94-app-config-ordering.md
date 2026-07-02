# V-94 design — type-and-runtime enforcement of pre-start wiring order

Status: IMPLEMENTED (2026-05-30) — `NmpAppBuilder<S>` typestate shipped in PR #858.
ADR-0072 / #2205 later moved the durable native runtime owner to
`nmp-native-runtime`; the three open design decisions (§7.i-iii) remain
resolved as: consume-and-return typestate, phantom-typed states,
builder-is-the-AppHost.

ADR-0069 later replaced defaults-era production composition, and the 2026-06-30
defaults deletion removed the defaults bundle entirely. References below to
`the deleted defaults bundle` or `the hidden defaults preset` describe the historical caller shape this
typestate work had to support, not the current production app architecture.
Current roots compose `nmp-substrate` plus explicit protocol/app installers.

Issue: V-94 (#618). Co-designed with F-08 (NmpAppBuilder) and V-95 (issue
#619, WalletRuntime init order). Follow-up #618 Stage 1 (2026-06-16) moved the
native actor spawn from `nmp_app_new` to `nmp_app_start`: `nmp_app_new` is now a
passive handle with a command channel and update listener, and pre-start
commands queue until the actor is spawned with the final configuration.

## 1. Problem (code-grounded)

Root cause: `nmp_app_new()` spawned the actor thread immediately.
The actor then blocked on the first command before constructing the kernel — an
explicit race absorber so the host could run setters after `nmp_app_new` but
before the kernel was built. The kernel construction point read every wiring
slot: storage_path, routing_substrate, publish_resolver, ingest_dispatcher,
dm_inbox_relay_lookup, blocked_relays, bootstrap_self_kinds, coverage_hook,
req_frame_interceptor, etc.

Consequence: any setter that runs AFTER the first command is silently ignored
(the slot was already read). Ordering is documented in prose only (18 sites in
lib.rs; the AppHost trait doc at substrate/app_host.rs:106-114). Nothing —
compile-time or runtime — prevents wiring-after-start.

Two distinct defects hide under one item:
- **Late-wiring (ordering):** a setter called after the actor read its slot is
  a no-op. This is the general bug across all ~18 sites.
- **Surprising-default omission:** most slots degrade gracefully by design
  (in-memory store, `NoopOutboxResolver`, `EmptyOutboxRouter`, `coverage_hook
  None`). The one that loses user data silently is `nmp_app_set_storage_path`
  omission → permanent in-memory store. That is the only "must-be-present" slot;
  the rest are legitimately optional substrate graceful-degradation.

Critical scope correction vs. the backlog: of the ~18 "setters", only FOUR are
C-ABI symbols (`nmp_app_set_update_callback` :2148, `nmp_app_set_storage_path`
:2188, `nmp_app_start` :2204, `nmp_app_configure` :2221). The rest are `AppHost`
**Rust trait methods** (substrate/app_host.rs) invoked from the Rust composition
root — `explicit owner composition` (crates/explicit composition/src/lib.rs)
and per-app `register.rs` (apps/chirp/crates/nmp-app-chirp/src/ffi/register.rs:53). So
the enforcement surface splits cleanly in two.

V-95 is the same root shape: `nmp_nip47::install_wallet_runtime` must run before
any wallet action dispatches; today it is runtime-guarded with an `Err` string
("wallet runtime not installed", crates/nmp-nip47/src/action.rs:76) rather than
type-enforced — an ordering contract with no compile-time backstop.

## 2. Why (a) builder and (b) runtime diagnostic are NOT alternatives

Rust's type system cannot enforce call-ordering across an `extern "C"` boundary.
A Swift/Kotlin host calling `nmp_app_set_storage_path` then `nmp_app_start` gets
zero compile-time guarantee — no typestate token crosses FFI. Therefore:

- A **typestate/builder (a)** is the correct enforcement for the **Rust**
  composition root (where the deleted hidden defaults preset used to live
  beside per-app wiring). It makes "wire then start" the only expressible
  sequence in Rust.
- A **runtime guard + diagnostic (b)** is **irreducible** for the **C-ABI** —
  it is the only mechanism that can catch a misordered Swift/Kotlin host.

The correct end-state uses **both**, each scoped to the surface it can actually
police, unified under the single `NmpAppBuilder` type the crate-boundary spec
already blesses (docs/architecture/crate-boundaries.md:269, :835). V-94's
"builder" and F-08's `NmpAppBuilder` are ONE construct, not two competing ones.

## 3. Recommended architecture (end-state)

### 3.1 `NmpAppBuilder` in `nmp-native-runtime` (the Rust enforcement, (a) + F-08)

A single config/builder type that owns the wiring phase and makes start the only
terminal transition. PR #858 first proved the V-94/F-08 builder shape; ADR-0072
settled the durable crate owner as `nmp-native-runtime`. The deleted defaults
bundle is no longer a composition target.

- `NmpAppBuilder::new()` — begins a config session. Owns the in-construction
  `NmpApp` (or its slots) in an un-started state.
- It IMPLEMENTS `AppHost` during the config phase, so explicit
  substrate/protocol/app installer calls bind to `&mut builder`.
- A terminal `start(self, RunConfig) -> NmpAppHandle` consumes the builder and
  drives the lifecycle. After `start`, no `AppHost` setter is reachable because
  the builder value is moved — late wiring is a compile error in Rust callers.
- `storage_path` is the one required field: the builder's `start` requires it be
  set (or an explicit `.in_memory()` opt-in), turning the silent data-loss
  default into an explicit choice. Every other slot keeps its graceful default.

This does NOT enforce "all slots present" (that would break substrate
graceful-degradation and test ergonomics). It enforces (1) ordering by move
semantics and (2) the single genuinely-required field by an explicit terminal
precondition.

**Implementation:** `nmp-native-runtime::NmpAppBuilder<S>` uses phantom-typed states
(`Unstarted` → `StorageSet`) and the consume-and-return pattern. The builder
implements `AppHost + ActionRegistrar` directly (builder-is-the-AppHost). The
`start()` method exists only on `NmpAppBuilder<StorageSet>` — calling it without
a storage choice is a compile error, proven by a `compile_fail` doctest.

### 3.2 C-ABI runtime guard + composition-ledger diagnostic (b, irreducible for FFI)

For hosts that drive the raw C-ABI directly (Chirp's Swift bridge, Kotlin), add
a runtime guard:

- The `NmpApp` carries a `started: AtomicBool` (set by the Start dispatch).
- Each init-only config setter, when called after `started`, does NOT silently
  mutate-and-be-ignored. It returns `NmpConfigStatus_AlreadyStarted` on C/JNI
  surfaces (or the Rust enum on internal setters) and records
  `Disposition::DroppedLateWiring` in the composition ledger.
- Hosts pull `nmp_app_debug_info(app, domain=1)` (composition-report domain) to inspect the
  rejected seam/key. This keeps the signal on the ADR-0069 composition surface
  instead of adding a second diagnostic channel.
- `nmp_app_set_storage_path` specifically returns a nonzero status so Swift,
  Kotlin, TUI, and desktop startup paths can assert/fail loudly before the app
  silently falls back to in-memory storage.

**Status (2026-06-16):** §3.2 is implemented as explicit status codes plus the
ADR-0069 composition ledger. No update-channel `LateWiring` frame is used.

### 3.3 V-95 folded in

`install_wallet_runtime` and the other "before-first-dispatch" runtime injections
route through the same builder phase: the native runtime's wallet wiring becomes
a builder step, so the runtime is installed during config, before `start`. The
runtime-guard diagnostic (§3.2) covers the C-ABI path for the same defect class
(a wallet action dispatched before the runtime is installed already returns a
typed `Err`; the diagnostic makes the *ordering* mistake observable rather than
only the *use* mistake).

**Status (issue #619 — DONE):** the reusable wallet wiring lives in
`nmp_nip47::register(&mut impl AppHost, Config::new(storage_path))` (lifted out of the
app-private `nmp-app-chirp::wallet_runtime`), and `nmp-native-runtime` exposes
it as the typed `NmpAppBuilder::<Unstarted>::with_wallet()` builder step. Because
`start(self, RunConfig)` consumes the builder by move, a Rust caller cannot
reach `start()` without `.with_wallet()` having already installed the runtime —
the install-before-dispatch ordering is now expressed in the type system, not in
prose. The C-ABI runtime guard (`NmpApp::started: AtomicBool` set in the Start
dispatch arm; every `nmp_app_set_*` setter records `Disposition::DroppedLateWiring`
into the composition ledger when called post-start) was already shipped by
ADR-0069 Part 2 and remains the irreducible backstop for raw-C-ABI hosts,
alongside the `active_wallet_runtime()` `Err` guard in each wallet
`ActionModule::execute`.

## 4. New crates / types

- No new crate for V-94. `nmp-native-runtime` now owns `NmpAppBuilder`;
  current roots compose `nmp-substrate` and protocol/app installers through
  `AppHost`.
- New types:
  - `nmp_native_runtime::NmpAppBuilder` (config-phase host; implements `AppHost`).
  - `nmp_native_runtime::RunConfig` (the visible_limit / emit_hz that
    `nmp_app_start` takes today, made a typed value passed to `builder.start`).
  - `NmpConfigStatus` (`Ok`, `NullApp`, `AlreadyStarted`, `Unavailable`) for
    init-only C/JNI config calls that must be loud without throwing across FFI.
  - `NmpApp::started: AtomicBool` (new slot) + per-setter guard.

## 5. Ordered steps (for the implementer, after ADR sign-off)

1. Add `NmpConfigStatus` return codes for init-only config calls and document
   `DroppedLateWiring` as the pull diagnostic in the composition ledger.
2. Add `NmpApp::started: AtomicBool`; set it in the Start dispatch arm. Guard
   each init-only config setter: if `started`, return `AlreadyStarted` and
   record `DroppedLateWiring` instead of mutating an already-read slot.
3. Introduce `NmpAppBuilder` implementing `AppHost` in the native runtime;
   route explicit substrate/protocol/app installers through it. Add
   `RunConfig` + terminal `start(self, RunConfig)`.
4. Make `storage_path` the one required field on `start` (or explicit
   `.in_memory()`).
5. Migrate the canonical Rust composition roots (chirp register.rs, fixture
   ffi.rs) to construct via `NmpAppBuilder` and call its terminal `start`.
6. Fold V-95: install the wallet runtime as a builder step in the template's
   wallet wiring; confirm the C-ABI diagnostic covers the misordered path.
7. (ADR decision dependent) Decide the fate of the recv-block race-absorber
   (actor/mod.rs:1169-1178). If the builder guarantees config-complete-before-
   start for Rust callers, the hack is only still needed for raw C-ABI hosts —
   keep it until those migrate, then remove. Treat removal as a separate,
   later change with its own test pass.
8. Update prose: replace the 18 "MUST be called before nmp_app_start" doc blocks
   with one pointer to the builder contract + the diagnostic.

**Steps 3-5 are complete as of PR #858, with durable ownership moved to
`nmp-native-runtime` by ADR-0072 / #2205.** Steps 1-2 and 6-8 remain open.

## 6. Risks

- **ABI churn (highest):** if `start` consumes the builder pointer and returns a
  new handle pointer, every Swift/Kotlin caller's create/start sequence changes.
  An in-place started-flag transition (§7.i) avoids ABI churn but is a weaker
  guarantee. This is the central ADR fork.
- **Startup-semantics change:** removing the recv-block race-absorber changes
  the invariant "first command may be non-Start". Widens blast radius across the
  actor loop tests. Deferred to step 7, gated on the builder covering all
  callers.
- **Test ergonomics:** many tests construct `NmpApp` and send Start directly.
  The builder must keep a low-ceremony test path (e.g. `.in_memory().start(...)`)
  or it taxes every actor test. Enforcing all-slots-present would break these —
  hence §3.1 enforces only ordering + the one required field.
- **Two enforcement surfaces:** the builder (Rust) and the diagnostic (C-ABI)
  must not drift. Mitigate by routing the canonical Rust roots through the
  builder so the diagnostic only ever fires for raw-C-ABI misuse.

## 7. Open decisions (resolved by PR #858)

i.  Does `nmp_app_start` / `builder.start` CONSUME the handle and return a new
    started-handle pointer (strong typestate, ABI break), or transition in place
    via the `started` flag (ABI-compatible, runtime-only guarantee on the
    C-ABI side)? — **Resolved: consume-and-return typestate.** The builder is a
    Rust-only construct; the raw C-ABI path is unchanged. No ABI break.
ii. Typestate (phantom-typed `NmpAppBuilder<Configuring>` → `Started`) vs a
    single runtime-checked builder type. — **Resolved: phantom-typed states**
    (`Unstarted` / `StorageSet`).
iii. Does `NmpAppBuilder` BE the `AppHost` impl during config (so protocol
    `register(app, Config)` calls bind to it directly), or wrap an inner `NmpApp`?
    — **Resolved: builder-is-the-AppHost.** The builder implements `AppHost +
    ActionRegistrar` directly.
