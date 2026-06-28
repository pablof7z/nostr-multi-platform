# ADR-0049: Defaults yield; composition is observable

Status: ACCEPTED (owner-approved 2026-06-12); amended by ADR-0069

> Numbering note: `0048` is the highest decision in `docs/decisions/` on master
> at authoring time (there is a known `0041` duplicate, both accepted). This ADR
> takes the next free number, `0049`, per the repo's single-source-of-truth /
> no-duplicate-id discipline.

Current disposition: composition observability survives. The ledger must explain
an explicit production composition root, including what installers registered,
skipped, yielded, or require from the runtime. It must not justify hidden
production `register_defaults()` behavior.

## Context

NMP composes a running app by registering modules, parsers, projections, and
wiring slots against a host (`nmp_defaults::register_defaults` for the canonical
defaults; per-app crates for app-specific verbs — ADR-0046). Two latent defects
made that composition fragile and opaque. Both have well-studied prior art.

### Defect 1 — registration is order-dependent and inverted (the Spring lesson)

The action registry installed modules with a bare `HashMap::insert`
(`crates/nmp-core/src/kernel/action_registry.rs`): silent last-writer-wins. An
app registering its own module under a default namespace **before** the defaults
run was silently clobbered when `register_defaults` ran afterward. Chirp worked
only by accident of call order — it calls `register_defaults` first
(`apps/chirp/crates/nmp-app-chirp/src/ffi/register.rs:108`), so the defaults install and
nothing later overrides them.

This is exactly inverted from how a composition framework must behave. **Spring
Boot's deep lesson**: framework defaults must yield to application registrations
**regardless of call order**. Auto-configuration is processed *after* all user
beans, and a default bean is annotated `@ConditionalOnMissingBean` — it installs
only if the application has not already provided one. Order independence is the
whole point.

Spring Boot 1.x allowed a silent override plus a log line; that failed at scale,
and **Boot 2.1 flipped to fail-fast** (`BeanDefinitionOverrideException`) for an
accidental bean-name collision. NMP's D6 forbids panics across the C-ABI, so a
literal fail-fast everywhere is not available — but the *distinction* Boot drew
(a default yielding to an app is fine; two non-defaults colliding is a bug) is
the right one.

### Defect 2 — composition is silent, with no way to explain it (the report gap)

D6 makes NMP's composition **silent by design**: a yield, an override, or a
setter dropped because it ran after `nmp_app_start` never surfaces an error.
Spring proved silent composition is only viable *with* an explain surface — its
`ConditionEvaluationReport` answers "which beans matched, which were excluded,
and why". NMP had no analog. Worse, `crates/nmp-defaults/src/builder.rs`
documented a `KernelDiagnostic::LateWiring` diagnostic for the
setter-after-start case that did not exist — the gap was named and left open.

Bevy's `DefaultPlugins` provides the complementary data point: it detects
duplicate plugin registration and treats it as an error rather than a silent
overwrite — duplicate-detection is a first-class concern for a defaults bundle.

## Decision

Two coupled changes.

### Part 1 — Directional registry semantics (order-independent yielding defaults)

`ActionRegistry` (and the host seams that drive it) now distinguish two
registration intents and track per-entry **provenance** (`Default` vs `App`):

1. **`register_default::<M>()`** — entry-or-insert. Installs `M` **only** if its
   namespace is unclaimed; returns `bool` (`false` = yielded). This is the
   `@ConditionalOnMissingBean` shape: an app can pre-empt a default whether it
   registers before *or* after `register_defaults`.

2. **`register::<M>()`** (the app path) — keeps insert semantics, so an app
   intentionally overriding a default stays legal and silent-ish
   (`ReplacedPrevious`). It gains a **loud failure when it replaces another
   *app* registration** (app-over-app collision): a hard `debug_assert!` in
   dev/test builds, recorded-but-soft in release (D6 — no panic across the
   C-ABI; last-writer-wins is preserved so release behaviour is unchanged).

3. The **canonical NMP defaults** switch to the yielding path:
   `nmp_nip02::register_actions`, `nmp_nip17::register_actions`,
   `nmp_nip57::register_actions`, and the NIP-65 `publish_relay_list` module in
   `nmp-router` now call `register_default_action::<M>()`. App-specific
   registrations (Chirp's NIP-29, the wallet stack, visible-note relations)
   keep `register_action` (insert).

The host seam mirrors the registry: `ActionRegistrar::register_default_action`
is added with a default impl that delegates to `register_action` (so
non-recording / test impls stay valid); the kernel's `ActionRegistry` overrides
it with the real entry-or-insert behaviour. `NmpApp` exposes both
`register_action` and `register_default_action`.

### Part 2 — Composition ledger + report (`ConditionEvaluationReport` analog)

A new append-only **`CompositionLedger`** (`nmp-core`,
`kernel/composition_ledger.rs`) records every host-init composition decision:

```
CompositionRecord { seam: &'static str, key: String, provider: String,
                    disposition: Disposition, replaced: Option<String> }

Disposition::{ Installed, ReplacedPrevious, YieldedToExisting, DroppedLateWiring }
```

Recorded at the AppHost registration paths in `nmp-core`/`nmp-ffi`:

- **action registry** — all three dispositions (`Installed` /
  `ReplacedPrevious` / `YieldedToExisting`), with `provider` =
  `std::any::type_name::<M>()` and `replaced` naming the prior holder.
- **ingest parsers** — `Installed` pre-start, `DroppedLateWiring` post-start
  (additive seam; keyed by `kind`).
- **snapshot projections** — `Installed` pre-start, `DroppedLateWiring`
  post-start (keyed by projection key).
- **read-once wiring slots** — `set_routing_substrate`,
  `set_publish_resolver_factory`, `set_coverage_hook`, `set_host_op_handler`,
  `set_nostrconnect_bootstrap_relay`, storage-path/signing-broker init, and the
  other AppHost/config slots: `Installed` on first write, `ReplacedPrevious` on
  overwrite where the slot has a prior value, `DroppedLateWiring` after
  `nmp_app_start`.
- **dropped late wiring** — a `started: AtomicBool` on `NmpApp` flips when
  `nmp_app_start` sends `ActorCommand::Start`. From that point the actor has
  read every wiring slot once at kernel construction, so a later setter call is
  recorded as `DroppedLateWiring`. **This finally implements the
  `KernelDiagnostic::LateWiring` promise** `nmp-defaults/src/builder.rs`
  documented. The recording lives in `nmp-core`/`nmp-ffi` where the wiring is
  dropped — `nmp-defaults` is *not* restructured.

One FFI symbol, **`nmp_app_composition_report(app) -> *mut c_char`**, returns
the ledger as JSON, mirroring `nmp_app_recent_routing_decisions` exactly
(heap-owned C string freed via `nmp_free_string`; empty well-formed document for
a null app or a serialisation failure — D6). The symbol is declared in
`apps/chirp/ios/Chirp/Bridge/NmpCore.h` to satisfy the `ffi-drift` CI gate.

The ledger is a `Vec` behind a `Mutex`, written only during registration
(host-init) and the rare runtime slot replacement — **zero hot-path cost; no
polling, no background work (D8).**

## Consequences

- An app may register its own module under any default namespace, before or
  after `register_defaults`, and win — the order-dependence trap is gone.
- An accidental app-over-app namespace collision now fails loudly in dev/test
  instead of silently clobbering, while staying D6-safe (soft) in release.
- Hosts and diagnostics screens can finally answer "what did the composition
  install, what yielded, what was dropped late?" through one pull accessor — the
  `ConditionEvaluationReport` NMP lacked.
- `KernelDiagnostic::LateWiring`, previously documented-but-absent, is realised
  as the ledger's `DroppedLateWiring` disposition.

### Prior art

- Spring `@ConditionalOnMissingBean` — defaults yield to user beans regardless
  of order (Part 1).
- Spring Boot 2.1 override flip (`BeanDefinitionOverrideException`) — collisions
  among non-defaults are bugs, not silent overrides (Part 1 app-over-app
  assert; adapted to D6's no-panic rule via the dev-loud/release-soft split).
- Spring `ConditionEvaluationReport` — the explain-the-composition surface
  (Part 2 ledger + report).
- Bevy `DefaultPlugins` duplicate-detection — a defaults bundle treats
  duplicate registration as a first-class concern.

### Scope notes

- `nmp-defaults` is intentionally **not** restructured (a parallel session owns
  its tier-split). The late-wiring recording is wired in `nmp-core`/`nmp-ffi`,
  where the drop actually happens.
- The kernel's seeded `PublishModule` (installed by `default_registry()` before
  the ledger is attached) is the one registration not ledger-recorded: it is
  constant across every app and carries no composition decision. Every
  host-init registration after construction is recorded.
