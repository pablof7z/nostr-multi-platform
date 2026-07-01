---
type: research-record
date: 2026-06-29
session: 898a41b5-68e0-4b0f-b16c-c6072454bd6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/898a41b5-68e0-4b0f-b16c-c6072454bd6a.jsonl
source_lines: 1132-1245
agent_attribution: a96c931d134848ae1
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Codegen seam investigation tracing whether app-owned event kinds get first-class generated builders; verdict: NO, with file:line evidence across 5 evidence sections"
captured_at: 2026-06-29T11:11:03Z
---

Codegen seam investigation tracing whether app-owned event kinds get first-class generated builders; verdict: NO, with file:line evidence across 5 evidence sections

---

User: [Agent task result: Agent "Trace app-owned kind codegen seam" finished]
## Verdict

**NO** — app-owned event kinds cannot get first-class typed write builders today.

---

## Evidence

### 1. **Codegen pipeline is hardcoded to NMP's schemas**

- **Entry point:** `crates/nmp-codegen/src/action_builders.rs:96–104` (`render()` function)
  - Reads from the **hardcoded** `ACTION_BUILDERS` registry (line 62: `pub use registry::{..., ACTION_BUILDERS}`)
  - No mechanism to pass external schema set

- **Schema source-of-truth:** `crates/nmp-codegen/src/action_builders/registry/table.rs:27–408`
  - `ACTION_BUILDERS` is a public const array: `pub const ACTION_BUILDERS: &[ActionBuilder] = &[...]`
  - Every action is hand-coded as an `ActionBuilder` struct entry
  - List includes ONLY NMP's built-in NIPs (nip02, nip18, nip25, nip51, nip84, nip22, etc.)
  - There is no loop over external files, no callback to merge app-provided entries, no dynamic registration

- **CLI invocation:** `crates/nmp-codegen/src/main.rs:50`
  - `"action-builders" => cli_action_builders::run_gen_action_builders(args, &h)`
  - No `--schema-dir` or `--registry-path` argument supported
  - Reads only from the hardcoded registry

- **CI gate:** `.github/workflows/codegen-drift.yml:78–85`
  - Drift check runs: `cargo run --quiet -p nmp-codegen -- gen action-builders --platform ts --check`
  - Output path: `web/packages/runtime-web/src/actionBuilders.generated.ts`
  - Reads hardcoded ACTION_BUILDERS; no extensibility hook

### 2. **ActionModule CAN be registered by apps, but without generated builders**

- **ActionModule trait is public:** `crates/nmp-core/src/substrate/action.rs:130–221`
  - Trait definition: `pub trait ActionModule: Send + Sync + 'static`
  - Public `execute()` method taking `&self` and `action: Self::Action`

- **ActionRegistrar trait is public:** `crates/nmp-core/src/substrate/action.rs:228–248`
  - `pub trait ActionRegistrar { fn register_action<M: ActionModule + 'static>(&mut self, module: M) -> Result<(), RegistrationError> }`
  - Comment explicitly names this as "the path app-specific verbs (Chirp's NIP-29, wallet, …) use"
  - Apps can register custom action modules at composition time

- **Composition pattern:** `crates/nmp-defaults/src/lib.rs:69–84`, `crates/nmp-defaults/src/composition.rs`
  - App crate calls `nmp_defaults::register_substrate(host, ...)` then calls `app_core::register(host)`
  - App's composition function can call `host.register_action(MyCustomModule)`

### 3. **Tracked gap: nip57 zap send has NO generated builder (intentional)**

- **Location:** `crates/nmp-codegen/src/action_contract/table.rs:399–401`
  ```rust
  builder_support: BuilderSupport::NotGenerated {
      reason: "post-v1 zap send surface removed from v1 generated host builders (#2318)",
  },
  ```
  - This is the ONLY `NotGenerated` entry for a non-wallet action
  - Shows the codegen gap is known and tracked, but intentional (post-v1)

### 4. **Architectural intent: app-specific features stay in app crates**

- **Crate boundaries (§8):** `docs/architecture/crate-boundaries.md:265–274`
  > "If a feature would be useful to a different Nostr app, it belongs in an NMP crate. If it is specific to one app's product domain, it belongs under `apps/<app>/`."

- **App composition (§9):** `docs/architecture/crate-boundaries.md:305–313`
  > "App crates under `apps/<app>/` compose `nmp-defaults` plus app-specific state **and own all operator policy**… Mutating product behavior should flow through registered actions or protocol commands."

- **Builder guide (§15):** `docs/builder-guide/15-codegen-and-ffi.md:39–46`
  > "The canonical way to compose an app is explicit Rust composition… There is NO generated per-app FFI crate; the app core owns explicit Rust composition and the raw C-ABI surface is shared."
  > "(The old `gen modules` scaffolder was deleted.)"

### 5. **No precedent: zero app-owned ActionModules in the codebase**

- Search across codebase: only NMP's core + protocol crates (`nmp-nip*`) define ActionModule impls
- Test files only: `nmp-core` test modules (JsonOnlyModule, PanicFirstModule, etc.) in `kernel/action_registry/*_tests.rs`
- No in-tree `apps/<app>` crate implements `impl ActionModule`
- External consumers (29er, hl, podcast-player, win-the-day) are outside this repo; no visibility into their patterns

---

## What's Missing (Gap Analysis)

For an app to get first-class typed builders TODAY, it would need to:

1. **Hand-roll FlatBuffers schema** (`.fbs` file in app crate)
2. **Manually implement ActionPayload** with encode/decode logic
3. **Manually implement ActionModule** with execute() and optionally decode_payload()
4. **Hand-spell the native builders** (Swift/Kotlin/TS) — NO codegen path exists
5. **Register at composition time** via `host.register_action(MyModule)`

To reach FIRST-CLASS (all 5 criteria from your question), an app would need:

- A way to feed app-owned `.fbs` schemas into `nmp-codegen`'s ACTION_BUILDERS generator
- A CLI flag or composition hook: `gen action-builders --external-registry <path>`
- The generated output written to app's bindings dirs (not NMP's)
- Native codegen running as part of app's build, not just NMP's CI
- Full drift gate coverage for app-owned builders (currently only NMP's builders are gated)

**None of these exist today.** The codegen pipeline is intentionally NMP-scoped, not extensible.

---

## Precedent

**No documented or visible precedent** of any app (in-tree or mentioned) using app-defined event kinds with typed builders. All evidence points to:

- Apps use only the NMP default action set (through composition)
- If an app needs custom behavior, it either:
  - **Proposes it to NMP** (if it's reusable → becomes a protocol crate)
  - **Hand-rolls a monomorphic ActionModule** (if app-specific → stays in app code, no codegen)

---

## Summary

The architecture **allows** app-owned ActionModules at the registration seam (public trait + public registrar), but **denies** them first-class codegen machinery. This is intentional: the doctrine splits reusable protocol/product features (→ NMP crates with codegen) from app-specific behavior (→ app crates with hand-rolled Rust + manual builder). An app's custom kind can execute through a hand-rolled ActionModule, but it will not get typed native builders unless someone extends the codegen entry point (currently a breaking change to ACTION_BUILDERS).
