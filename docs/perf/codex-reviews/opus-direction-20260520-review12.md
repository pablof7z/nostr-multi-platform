# Opus direction review #12 — the multi-app thesis

Prior reviews asked "what to delete." This one asks: could a second
non-social app ship with **zero** `nmp-core` changes? The answer splits in
two — and the split is the finding.

## 1. What would make NMP genuinely multi-app

Multi-app has two halves. **Views are proven.** `nmp_app_chirp_register`
(`apps/chirp/nmp-app-chirp/src/ffi.rs:90`) builds a projection and calls
`app.register_event_observer(Arc<dyn KernelEventObserver>)`
(`crates/nmp-core/src/ffi/mod.rs`). A new app writes its own projection +
per-app static lib and registers it — no kernel edit. That is the working
multi-app seam; preserve it.

**Actions are not proven.** Every app-specific action still requires a
`nmp-core` change. Until that is fixed, NMP is multi-app for *reading* and
single-app for *writing*.

## 2. Is `dispatch_action` wired well enough? No — two stacked ceilings.

- **Ceiling 1: `execute_action` is a hardcoded `match`.**
  `crates/nmp-core/src/ffi/action.rs:157-186` is `match namespace { "nmp.publish" => …, _ => Err }`.
  A host's `ActionModule` cannot execute even if registered — this `match`
  rejects it. The registry's `start()` validates generically; execution
  does not.
- **Ceiling 2: no host registration seam.** `NmpApp.action_registry`
  (`ffi/mod.rs:225`) is a private field built once via `default_registry()`
  (`ffi/mod.rs:369`), which registers only `PublishModule`
  (`action_registry.rs:275-279`). There is no `nmp_app_register_*` FFI.
  A host literally cannot add a module.

So `dispatch_action` is a single-verb façade (`nmp.publish`), not a generic
dispatcher. `ActionRegistry::reduce` being dead (`action_registry.rs:231`)
is a symptom, not the disease.

## 3. Does the fixture prove multi-app? No — it proves compilation only.

`apps/fixture/nmp-app-fixture/src/ffi.rs:44-49`: `FfiApp::dispatch` routes
`AppAction::Kernel` through `KernelReducer` but returns `UriRejected` for
`FixtureTodoCore` and `Nip29PublishPlan` — i.e. for *every app-domain
action*. The one path that would prove multi-app is explicitly a no-op. The
fixture proves `cargo check` passes, nothing more.

## 4. The ONE change for the next PR

Make execution a trait method, not a kernel `match`. Add `execute(&self,
ctx, action_json, actor_tx) -> Result<(), String>` to `ErasedActionModule`
/ `ActionModule`; rewrite `execute_action` as
`app.action_registry.execute(namespace, …)`. Pair it with one FFI symbol —
`nmp_app_register_action_module` — or a public `NmpApp` builder that calls
`register` before the actor spins.

This is small, deletes the `nmp.publish` special-case, and is the first
change that makes a second app's actions runnable without touching
`nmp-core`. Without it, review #13 finds the same split — wider.
