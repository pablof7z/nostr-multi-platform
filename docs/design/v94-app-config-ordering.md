# V-94 design — type-and-runtime enforcement of pre-start wiring order

Status: IMPLEMENTED (2026-05-30) — `NmpAppBuilder<S>` typestate shipped in PR #858;
the three open design decisions (§7.i-iii) are resolved as: consume-and-return
typestate, phantom-typed states, builder-is-the-AppHost.

Backlog: V-94 (issue #618). Co-designed with F-08 (NmpAppBuilder) and V-95
(issue #619, WalletRuntime init order).

## 1. Problem (code-grounded)

`nmp_app_new()` (crates/nmp-ffi/src/lib.rs:617) spawns the actor thread
immediately. The actor blocks on `command_rx.recv()` for the FIRST command
(crates/nmp-core/src/actor/mod.rs:1169-1178) — an explicit race-absorber so the
host can run setters after `nmp_app_new` but before the kernel is built. The
kernel is constructed only after that first command arrives, reading every
wiring slot at construction time (actor/mod.rs:1185-1308): storage_path,
routing_substrate, publish_resolver, ingest_dispatcher, dm_inbox_relay_lookup,
blocked_relays, bootstrap_self_kinds, coverage_hook, req_frame_interceptor, etc.

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
root — `nmp_app_template::register_defaults` (crates/nmp-app-template/src/lib.rs)
and per-app `register.rs` (apps/chirp/nmp-app-chirp/src/ffi/register.rs:53). So
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
  composition root (where `register_defaults` and per-app wiring live). It makes
  "wire then start" the only expressible sequence in Rust.
- A **runtime guard + diagnostic (b)** is **irreducible** for the **C-ABI** —
  it is the only mechanism that can catch a misordered Swift/Kotlin host.

The correct end-state uses **both**, each scoped to the surface it can actually
police, unified under the single `NmpAppBuilder` type the crate-boundary spec
already blesses (docs/architecture/crate-boundaries.md:269, :835). V-94's
"builder" and F-08's `NmpAppBuilder` are ONE construct, not two competing ones.

## 3. Recommended architecture (end-state)

### 3.1 `NmpAppBuilder` in `nmp-app-template` (the Rust enforcement, (a) + F-08)

A single config/builder type that owns the wiring phase and makes start the only
terminal transition. It is the home V-48 (`nmp-app-template`) was created to be
and the type F-08 names.

- `NmpAppBuilder::new()` — begins a config session. Owns the in-construction
  `NmpApp` (or its slots) in an un-started state.
- It IMPLEMENTS `AppHost` during the config phase, so every existing
  `register_actions` / `register_defaults` / per-NIP wiring call works unchanged
  against `&mut builder`. No NIP crate changes.
- `register_defaults(&mut self)` becomes an inherent method (or stays a free fn
  taking `&mut impl AppHost`; either way the builder is the host passed in).
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

**Implementation (PR #858):** `NmpAppBuilder<S>` uses phantom-typed states
(`Unstarted` → `StorageSet`) and the consume-and-return pattern. The builder
implements `AppHost + ActionRegistrar` directly (builder-is-the-AppHost). The
`start()` method exists only on `NmpAppBuilder<StorageSet>` — calling it without
a storage choice is a compile error, proven by a `compile_fail` doctest.

### 3.2 C-ABI runtime guard + diagnostic frame (b, irreducible for FFI)

For hosts that drive the raw C-ABI directly (Chirp's Swift bridge, Kotlin), add
a runtime guard:

- The `NmpApp` carries a `started: AtomicBool` (set by the Start dispatch).
- Each C-ABI `nmp_app_set_*` setter, when called after `started`, does NOT
  silently mutate-and-be-ignored. It emits a single **late-wiring diagnostic**
  on the EXISTING update-channel envelope — the same channel the actor panic
  frame already rides (actor/mod.rs catch_unwind → `update_tx`; envelope spec at
  docs/design/0001-ffi-update-channel-envelope.md). This is NOT a new delivery
  channel and NOT a new top-level diagnostic subsystem.
- The frame is a small typed variant ("LateWiring { symbol, ignored }") — the
  minimal "KernelDiagnostic" the backlog gestured at, framed as POST-START
  late-wiring detection, not presence-of-everything validation.
- `nmp_app_set_storage_path` specifically: emit the late-wiring diagnostic AND
  (since the default is data-loss) escalate it to a distinct severity so the host
  surfaces it. This is the one slot where "ignored" is materially harmful.

**Status (PR #858):** §3.2 is NOT implemented. The open backlog follow-up (V-94)
tracks this remaining work.

### 3.3 V-95 folded in

`install_wallet_runtime` and the other "before-first-dispatch" runtime injections
route through the same builder phase: `nmp-app-template`'s wallet wiring becomes
a builder step, so the runtime is installed during config, before `start`. The
runtime-guard diagnostic (§3.2) covers the C-ABI path for the same defect class
(a wallet action dispatched before the runtime is installed already returns a
typed `Err`; the diagnostic makes the *ordering* mistake observable rather than
only the *use* mistake).

## 4. New crates / types

- No new crate. `nmp-app-template` (exists, V-48) gains `NmpAppBuilder`.
- New types:
  - `nmp_app_template::NmpAppBuilder` (config-phase host; implements `AppHost`).
  - `nmp_app_template::RunConfig` (the visible_limit / emit_hz that
    `nmp_app_start` takes today, made a typed value passed to `builder.start`).
  - A late-wiring diagnostic variant on the existing FFI update-channel envelope
    (likely in nmp-core where the envelope variants live, or nmp-ffi if the
    envelope is FFI-local — decided by where ActorCommand::LifecycleEvent /
    panic frames are defined).
  - `NmpApp::started: AtomicBool` (new slot) + per-setter guard.

## 5. Ordered steps (for the implementer, after ADR sign-off)

1. Add the late-wiring diagnostic variant to the FFI update-channel envelope;
   document it alongside the panic frame in
   docs/design/0001-ffi-update-channel-envelope.md.
2. Add `NmpApp::started: AtomicBool`; set it in the Start dispatch arm. Guard
   each C-ABI `nmp_app_set_*` setter: if `started`, emit the diagnostic instead
   of silently mutating an already-read slot. Escalate severity for
   `nmp_app_set_storage_path`.
3. Introduce `NmpAppBuilder` in `nmp-app-template` implementing `AppHost`;
   move `register_defaults` to operate on it (free-fn-taking-`&mut impl AppHost`
   keeps working). Add `RunConfig` + terminal `start(self, RunConfig)`.
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

**Steps 3-5 are complete as of PR #858.** Steps 1-2 and 6-8 remain open.

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
iii. Does `NmpAppBuilder` BE the `AppHost` impl during config (so all NIP
    `register_actions` calls bind to it directly), or wrap an inner `NmpApp`?
    — **Resolved: builder-is-the-AppHost.** The builder implements `AppHost +
    ActionRegistrar` directly.
