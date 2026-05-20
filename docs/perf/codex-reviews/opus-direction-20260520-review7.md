# Opus Direction Review #7 — The Scaffold That Admits the Gap

**Date:** 2026-05-20
**Reviewer:** Opus (director-level direction review)
**Predecessor:** review #6 (`4e5cf938`, landed today 13:11) — read it first; this memo escalates it.

---

## The headline

This is the **seventh direction review in a single day**. Six of them
(`opus-direction-2026-05-20.md`, `…-2026-05-20b.md`, `…-0903.md`,
`…-actions-design.md`, `…-ffi-contract.md`, `…-operational.md`, `…-review6.md`)
all converge on the *same structural finding*: NMP has well-designed extension
contracts (`ViewModule`, `ActionModule`) and **no runtime seam that carries
their payloads to a shell**. Review #6 named the fix precisely: wire
`ActionRegistry::reduce`, build a `ViewRegistry`, add one generic
`projections: BTreeMap<String, Value>` slot to `KernelUpdate`.

In the ~2.5 hours of commits after review #6 landed (13:11), the team shipped
`#32` (M6 publish execution), `#34` (delta wire versioning), Chirp UI polish,
relay-role fixes, and a `DomainModule` NAMESPACE refactor. **Neither of review
#6's two named fixes shipped.** `ViewRegistry` still does not exist anywhere in
the tree (`grep -rn ViewRegistry crates --include='*.rs'` → zero non-comment
hits). `ActionRegistry::reduce` is still `#[allow(dead_code)]`
(`kernel/action_registry.rs:231`).

The question the brief asks — *"is there a pattern of good design followed by
empty registry theater?"* — has an evidence-backed answer: **yes.** The pattern
is not laziness; it is mis-prioritization. Design memos accumulate, scaffolds
accumulate, and the load-bearing seam never gets built because each day's work
goes to the social client (Chirp) instead.

---

## The new evidence: the scaffold documents its own failure

Review #6 said "the thesis is untested — only this review tested it." Review #7
has stronger evidence. **The team has now wired up the second app, and the FFI
it generated returns a rejection for every non-kernel action.**

`apps/fixture/nmp-app-fixture/src/ffi.rs:44-50`:

```rust
// ── Module-projected actions (coverage boundary) ─────────────
// Module crates expose no generic reducer reachable here.
other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {
    uri: other.namespace().to_string(),
    reason: "module-projected action has no generated reducer; \
routing requires a module-reducer seam (see NMP-145)".to_string(),
}),
```

`apps/fixture` is the real second-app proof — it composes `nmp-core` +
`nmp-nip29` + `fixture-todo-core` via `apps/fixture/nmp.toml`. Its
`AppAction` enum has three arms: `Kernel`, `FixtureTodoCore`, `Nip29PublishPlan`
(`action.rs:11-17`). Exactly **one** of those three (`Kernel`) reduces to
anything. Dispatch a `FixtureTodoCore(Action::Add{...})` — a literal todo
item — and the kernel hands back `UriRejected`. The codebase is not hiding the
gap; it is *narrating* it in a string constant.

`fixture-todo-core` itself (273 LoC) is a complete set of trait impls —
`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`,
`IdentityModule`. But every `ViewModule` hook body is a no-op:
`on_event_inserted` returns `None`, `on_projection_changed` returns
`state.payload.clone()` of a `Default`-constructed empty list
(`fixture-todo-core/src/lib.rs:85-123`). It is a **compile-time** proof that
the traits fit together. It is not a **runtime** proof that an app can be
built. A todo list that can never contain a todo is not a second app.

So review #6's conclusion stands and tightens: the substrate thesis
(`<100 LoC Rust + <300 LoC Swift`) is not "unproven" — it is, on the current
code, **disproven for any non-social app**. App #2 today must either (a) add
`#[cfg(feature = "todo")]` fields to `KernelUpdate` inside `nmp-core` (the D0
violation `wallet_status` already committed — `kernel/types.rs:530`), or (b)
parse kind tags in Swift via `nmp_app_register_raw_event_observer` (an
`aim.md` violation).

---

## 1. The substrate bet — is ViewRegistry the highest-leverage fix?

**Yes. There is no cheaper path, and you should not look for one.** The gap is
not a missing app — it is a missing *seam*. Three previous reviews have proposed
"build a different proof app"; that advice is wrong now, because the proof app
*already exists half-built* (`apps/fixture` + `fixture-todo-core`). A third
scaffold would just be a third thing returning `UriRejected`.

The two-part fix, in dependency order:

1. **Read side — `ViewRegistry` + one generic field.** Add
   `projections: BTreeMap<String, serde_json::Value>` to `KernelUpdate`
   (`kernel/types.rs:476`). Build a `ViewRegistry` that holds
   `dyn ErasedViewModule` trait objects (mirror `ActionRegistry`'s erasure
   pattern from `action_registry.rs:61-162` — that pattern is sound and
   already proven for the write side). On each accepted event, the kernel
   drives `ViewModule::on_event_inserted` → `snapshot` and writes the
   serialized `Payload` into `projections["<namespace>"]`. The shell reads
   `update.projections["fixture.todo.view"]` and decodes it. **No new
   `KernelUpdate` field per app, ever again.**

2. **Write side — give `ActionRegistry` a real executor.**
   `execute_action` (`ffi/action.rs:152`) hard-codes `match namespace
   { "nmp.publish" => …, _ => Ok(()) }` — the `_` arm is a silent no-op
   that returns a correlation id for an action that never ran. That is worse
   than a rejection; it is a *fake success*. Either wire `ActionRegistry::reduce`
   into the actor mailbox so each registered module's `reduce` drives real
   steps, or — at minimum, before that — make the `_` arm return
   `Err("no executor for namespace")` so a dispatched todo-add fails loudly
   instead of lying.

**The concrete proof app:** the one already in the tree. Make
`fixture-todo-core::TodoViewModule` have real hook bodies (an
`on_event_inserted` that appends, an `on_projection_changed` that recomputes
`open_count`), drive it through the new `ViewRegistry`, and render the list in
`nmp-desktop` (egui). Do not build anything new. Finish what is started.

---

## 2. What to stop — the dual dispatch systems

`nmp_app_dispatch_action` is FFI symbol **#48**. It did not *replace* the
bespoke verb surface; it joined it. Counting only the **write verbs** that the
registry is meant to subsume (excluding lifecycle/observer/`free`/`new`):
~20 symbols — `publish_note`, `react`, `follow`, `unfollow`, `add_relay`,
`remove_relay`, `publish_unsigned_event`, `publish_signed_event`,
`publish_signed_event_to`, `wallet_connect`, `wallet_disconnect`,
`wallet_pay_invoice`, `signin_nsec`, `signin_bunker`, `switch_active`,
`remove_account`, `create_new_account`, `claim_profile`, `open_author`,
`open_thread`, `open_firehose_tag`. Each feeds a bespoke `ActorCommand`
variant (`PublishNote`, `React`, `Follow`, … — `actor/mod.rs:120`+).

**Migration path — put it on a calendar, not in a doctrine doc:**

- **Now:** every bespoke write verb gets a `// DEPRECATED: route via
  dispatch_action("<ns>", …)` doc-comment, and a CI lint that *fails the
  build* if a new `pub extern "C" fn nmp_app_*` write verb is added. New
  write surface goes through the registry or it does not land.
- **Within 30 days:** the registry has an executor for `nmp.react`,
  `nmp.follow`, `nmp.relay-edit`. As each lands, its bespoke verb becomes a
  thin shim that calls `dispatch_action` internally — same symbol, zero shell
  changes, one code path underneath.
- **Within 90 days:** delete the shims; bump the FFI schema version.

**Is the registry at risk of becoming documentation theater again?** It
already is — for 89 days running. `ActionRegistry::reduce` is dead code with a
comment promising "the M6 ledger is the intended caller." `default_registry()`
registers exactly one module. The 20 `ViewModule` impls are tested *statically*
and read by *nothing*. The cure for theater is not more traits — it is a
deletion deadline. If the bespoke verbs are not on a deletion calendar, the
registry will be ornamental in review #8.

---

## 3. What's missing that matters

- **Schema migration is a version *number*, not a *plan*.** `#34` added
  `KERNEL_SCHEMA_VERSION` so a shell can *detect* a mismatch
  (`kernel/update.rs:16`). It cannot *survive* one. There is no migration
  path: when the number bumps, an old shell gets a frame it knows is wrong and
  can do nothing but degrade. That is acceptable for v1 — but it must be
  *written down* as a deliberate decision (an ADR: "shells hard-require an
  exact schema match; cross-version is an app-store update"), not left as an
  implied gap. With `projections` as a generic map, additive changes stop
  needing version bumps at all — another reason #1 is the keystone.

- **No backpressure on the command channel.** `MEMORY.md` records the D8
  "no polling" rule and an unbounded command channel ("T114b — unbounded
  command channel cannot drop" — `kernel/types.rs:464`). Opus review #1
  already flagged this: unbounded queue + single actor + no backpressure =
  unbounded memory under a fast producer (a relay firehose, a misbehaving
  shell loop). "Cannot drop" is being sold as a feature; it is an
  out-of-memory vector. Before any scale claim, the command channel needs a
  bound and a defined shed policy.

- **Developer-day test fails.** "Can a new engineer add NIP-51 bookmarks in a
  day?" No. They would write `BookmarkModule: ActionModule` +
  `BookmarkView: ViewModule`, register both, and hit the exact `UriRejected`
  wall `apps/fixture` already documents. Until #1 ships, the honest answer to
  every "add a NIP" question is "fork `KernelUpdate` or write Swift."

---

## 4. What NMP should stop doing

- **Stop adding `#[cfg(feature = "…")]` app fields to `nmp-core`.**
  `wallet_status` behind `#[cfg(feature = "wallet")]` (`kernel/types.rs:530`)
  is a D0 violation with a feature flag painted over it. A Cargo feature does
  not make `wallet` a protocol primitive; it makes the violation
  *configurable*. Every future app would copy this exact move. The CI lint in
  §2 should also reject new `KernelUpdate` fields outright once `projections`
  exists.

- **Stop writing more `ViewModule` impls.** There are ~20, all orphans. More
  orphans is not progress — it is more code to migrate when `ViewRegistry`
  lands, and more false signal that "the substrate works." Freeze new
  `ViewModule` impls until they have a registry to plug into.

- **`nmp-testing` is 20,686 LoC** — larger than every protocol crate
  *combined* (nip01+22+23+29+42+57+59+77 ≈ 14.5K). A test-support crate
  bigger than the thing under test is a smell worth one audit pass: how much
  of it is consumed by current tests vs. scaffolding for milestones that
  shifted? Not urgent, but name it before it doubles.

- **`nmp-codegen` (1,286 LoC) generates glue that returns `UriRejected`.**
  The codegen path produces `apps/fixture/nmp-app-fixture` — and that
  generated `ffi.rs` cannot dispatch a module action. Generating *correct
  glue around a missing seam* is wasted sophistication. Either the codegen is
  blocked on #1 (then say so and pause it), or it is generating the wrong
  thing. Do not invest further in codegen until the seam it generates against
  exists.

---

## 5. The 90-day bet — the single falsifiable deliverable

**Ship `apps/fixture/nmp-app-fixture` running on `nmp-desktop` (egui)
displaying a working, mutable todo list, driven entirely by
`fixture-todo-core` through a new `ViewRegistry` and a
`projections["fixture.todo.view"]` slot on `KernelUpdate` — with the new
code budget held to <100 LoC in `fixture-todo-core` and <300 LoC in
`nmp-desktop`.**

This is binary and demoable. Either the todo list renders and accepts
"add" / "toggle" / "clear completed" with no `nmp-core` changes naming a
todo — in which case the substrate thesis is **proven** — or it does not,
and the thesis is **dead** and NMP should be honestly relabeled a *Nostr
social-client framework* (which is a fine thing to be — but stop selling the
substrate story to every review).

Do not measure this with a passing `cargo test`. `fixture-todo-core`
*already* has a green test (`action_rejects_empty_todo_title`) and proves
nothing about runtime. Measure it with a screenshot of a desktop window
showing three todos, one of them checked.

If review #8 is written and this demo does not exist, the finding will not be
"the architecture is wrong." It will be "the team commissioned eight direction
reviews and built a social client instead." Close the seam. Then there is
nothing left to review.

---

## Appendix — evidence index

| Claim | Location |
|---|---|
| Second app returns `UriRejected` for module actions | `apps/fixture/nmp-app-fixture/src/ffi.rs:44-50` |
| `fixture-todo-core` ViewModule hooks are no-ops | `crates/fixture-todo-core/src/lib.rs:85-123` |
| `ViewRegistry` does not exist | `grep -rn ViewRegistry crates --include='*.rs'` → 0 non-comment hits |
| `ActionRegistry::reduce` is dead code | `crates/nmp-core/src/kernel/action_registry.rs:231` |
| `default_registry()` registers one module | `crates/nmp-core/src/kernel/action_registry.rs:275-279` |
| `KernelUpdate` is monolithic, no `projections` slot | `crates/nmp-core/src/kernel/types.rs:476-534` |
| `wallet_status` is `#[cfg(feature="wallet")]` in core | `crates/nmp-core/src/kernel/types.rs:530` |
| `execute_action` `_` arm is a silent no-op | `crates/nmp-core/src/ffi/action.rs:175` |
| 48 `nmp_app_*` FFI symbols; ~20 are write verbs | `grep 'pub extern "C" fn nmp_app' crates/nmp-core/src/ffi` |
| Bespoke `ActorCommand` write verbs coexist | `crates/nmp-core/src/actor/mod.rs:120`+ |
| Review #6's two fixes not shipped post-13:11 | `git log 4e5cf938..HEAD` |
