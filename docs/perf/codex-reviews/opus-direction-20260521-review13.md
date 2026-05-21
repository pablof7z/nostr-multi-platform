# Opus direction review #13 — the dispatch seam rotted from non-use

Review #12 asked "is `dispatch_action` wired well enough?" and found two
ceilings. Ceiling 1 (hardcoded `match`) is now fixed — `execute_action`
(`crates/nmp-core/src/ffi/action.rs:135-138`) is a one-liner delegating to
`app.action_registry.execute(...)`. But the deeper finding this review adds:
**the generic seam was built, and then nothing was migrated onto it.** It is
already rotting. That is the story of review #13.

## 1. The honest status — what NMP is genuinely good at

The kernel is a real, disciplined actor runtime, and the parts that are
*used* are good. Concrete evidence:

- **The actor boundary is clean.** `crates/nmp-core/src/actor/mod.rs:1-15`
  documents a dual-channel design where commands (`try_recv`) never get
  dropped under a relay-event flood. The kernel (`kernel/mod.rs:192`) is the
  single writer; `EventStore` is `Arc<dyn EventStore>` with interior
  mutability so D4 holds even with a shared handle.
- **D2 and D6 are clean at the FFI surface.** Every `unwrap()`/`expect()`
  the audit grep found in `ffi/` is inside a `#[cfg(test)]` module — zero
  panics at public boundaries. All 56 `#[no_mangle]` symbols in
  `ffi/identity.rs` guard with `let Some(app) = app_ref(app) else { return }`
  and surface decode failures as `ActorCommand::ShowToast` rather than
  silent no-ops (`ffi/identity.rs:155-162`). This discipline is real and
  worth protecting.
- **The v1 read-side extension seam works.** `KernelEventObserver`
  (`substrate/mod.rs:21-33`) is genuinely exercised: `nmp-app-chirp`
  registers `Arc<dyn KernelEventObserver>` projections
  (`apps/chirp/nmp-app-chirp/src/ffi.rs:115`, `marmot/ffi.rs:245`) with no
  kernel edit. A new app *can* read without touching `nmp-core`.
- **`substrate/mod.rs` tells the truth.** Its module doc openly states the
  v2 trait family has "no kernel-side registry" and that a prior
  `ModuleRegistry` was "documentation theater" and was deleted. Honest
  self-documentation is rare and load-bearing for a 6-month-later reader.

## 2. The three highest-impact gaps

**Gap 1 — the action seam exists but no shell uses it.**
`nmp_app_dispatch_action` is exported, in `ios/Chirp/Chirp/Bridge/NmpCore.h`,
and unit-tested. But a grep for callers across `apps/` and `ios/` finds the
symbol *only in the header* — every iOS/Marmot path still calls the per-verb
`nmp_app_publish_note` / `nmp_app_react` / `nmp_app_follow`. The generic
dispatcher has zero production callers. A seam with no traffic is not a
feature; it is dead weight that will silently drift. Shape of fix: pick one
verb, delete its per-verb symbol, force the migration (see §3, §5).

**Gap 2 — no host registration seam (review #12 Ceiling 2, still open).**
`NmpApp.action_registry` (`ffi/mod.rs:225`) is a private field built once via
`default_registry()` (`ffi/mod.rs:369`), which registers only `PublishModule`
(`action_registry.rs:235`). There is still no `nmp_app_register_action_module`
FFI and no public `NmpApp` builder. A host literally cannot add an action
module. NMP remains multi-app for *reading*, single-app for *writing*. Shape
of fix: one FFI symbol (or builder) that calls `ActionRegistry::register` +
`register_executor` before the actor spins.

**Gap 3 — D0 violations are baked into the wire schema, not just symbols.**
`KernelSnapshot` (`kernel/types.rs:505-564`) hardcodes `accounts`,
`publish_queue`, `publish_outbox`, `bunker_handshake`, and (gated)
`wallet_status` — protocol/app nouns in the JSON every shell decodes against.
Symbol-level D0 violations can be deleted; a wire-format field every app's
decoder depends on cannot. Until snapshots carry a generic `projections`
map (the never-shipped ViewRegistry), every new app domain forces a
`KernelSnapshot` edit. Shape of fix: a generic `projections: Map<String,
Value>` field, populated by registered `ViewModule`s, replacing the bespoke
fields one at a time.

## 3. The one thing NMP should stop doing

**Stop shipping per-verb FFI symbols. Specifically, delete
`nmp_app_publish_note` (`ffi/identity.rs:109`), then `nmp_app_react:347`,
`nmp_app_follow:370`, `nmp_app_unfollow:384`.**

These are not just D0 cosmetic violations ("app nouns in `nmp-core`"). Their
second-order cost is the disease behind Gap 1: *as long as a working
`nmp_app_publish_note` exists, no shell will ever migrate to
`dispatch_action`.* The generic seam was built, and these symbols guaranteed
it would never get traffic. Every review since #9 has recommended deleting
`nmp_app_publish_note` to force the migration; the codebase shows it was
never executed. Keeping them means review #14 finds the identical split.

## 4. The bet that scares you

**The multi-app thesis itself — that a second, non-social app can ship with
zero `nmp-core` changes — has never been demonstrated and current evidence
points the other way.** Two unfalsified claims are baked in:

1. The fixture app, the supposed proof of multi-app, does not prove it:
   `apps/fixture/.../ffi.rs` routes kernel actions through `KernelReducer`
   but returns `UriRejected` for *every* app-domain action (review #12 §3).
   It proves `cargo check` passes, nothing more.
2. The "few hundred lines" budget (`overview-and-dx.md` §3.2: iOS ≤ 400 LOC)
   is violated by 21×. Chirp's iOS shell is 8,384 hand-written Swift LOC.
   The spec calls exceeding the budget "a framework-design failure."

What would falsify the bet — i.e. prove it right: a second non-social app
dispatching a real domain action through `dispatch_action` (registered via a
host seam, not a kernel edit) with its platform shell under the 400 LOC
budget. Nothing in the tree does this today. Until it exists, "multi-platform
Nostr SDK" is an aspiration, not a tested property.

## 5. The next concrete PR

**Delete `nmp_app_publish_note` and migrate Chirp's iOS compose path to
`nmp_app_dispatch_action("nmp.publish", …)`.** One week, one engineer.

Why this one and not review #12's `nmp_app_register_action_module`: that PR
adds capability nobody is forced to use — it would sit unexercised exactly
like `dispatch_action` does now. This PR is a *forcing function*. It:

- proves the generic dispatch path end-to-end against a real shell (the
  first production caller `dispatch_action` has ever had);
- removes one D0 violation permanently;
- surfaces every rough edge in the JSON action contract while the blast
  radius is one verb;
- makes Gap 2 (host registration) the *obviously* next PR, because the
  second verb to migrate (`react`/`follow`) will be an app noun that needs a
  host-registered module.

If `dispatch_action` cannot carry `publish` for one real app, the multi-app
thesis is falsified cheaply and early — which is exactly what review #13
should buy. Restating "add a register FFI" buys nothing; executing the
delete does.
