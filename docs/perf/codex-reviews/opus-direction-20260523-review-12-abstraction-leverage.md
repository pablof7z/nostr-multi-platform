---
title: "Opus direction review #12 — abstraction leverage: what's earning its keep"
date: 2026-05-23
author: Opus (senior distributed-systems architect, code-grounded)
scope: Audit which NMP abstractions create leverage and which create indirection.
verified-against: HEAD on master (BACKLOG.md at HEAD 76bc8547; verified files individually).
prior-reviews-built-on: #8 (HostOpHandler), #10 (ActorCommand god enum), #11 (DX after Notes spike).
---

# Verdict on the five probe questions

## 1. `dispatch_action` vs `dispatch_capability` — coherent, but the seam is invisible to Swift

The split **is** coherent, not accidental. Read together:

- `crates/nmp-core/src/ffi/action.rs:99` — `nmp_app_dispatch_action(app, namespace, action_json) -> *mut c_char`. Returns `{"correlation_id":"…"}` (accepted, enqueued, terminal arrives later via `projections["action_stages"]`) or `{"error":"…"}` synchronously. **User-intent → actor**.
- `crates/nmp-core/src/ffi/capability.rs:56` — `nmp_app_dispatch_capability(app, request_json) -> *mut c_char`. Returns a populated `CapabilityEnvelope` synchronously; the actor is **not** involved. **Rust→OS, the host returns data**.

This is exactly RMP bible commandment #6 (capability bridges report, never decide) applied at the FFI seam. They are *opposites*, not duplicates: actions go *into* the kernel for policy; capabilities go *out* of the kernel for execution. Action enqueues; capability synchronously round-trips. Both return malloc'd JSON freed via `nmp_app_free_string` (`capability.rs:73`).

The real critique is not conceptual — it is presentational. The Swift developer faces **two C surfaces with no Swift-level facade unifying them**, plus a third path (typed `*mut NmpApp` setters for `set_update_callback` / `set_capability_callback` / `set_lifecycle_callback`). The model is correct; the marketing is unconveyed. A `KernelHandle` Swift type that exposes `dispatch(action:)` (returns `DispatchResult`, parsed as in `KernelBridge.swift:682`) and `capability(_:)` would make the duality legible without changing a single Rust line.

**Verdict:** *Earning its keep.* Don't merge them. Do build a Swift-side facade.

## 2. `ActionModule` adoption — load-bearing, not premature

`grep -rn "impl ActionModule for"` across the workspace returns roughly twelve non-test implementations spread across **eight crates outside `nmp-core`**:

| Crate | Implementors |
|---|---|
| `nmp-nip29/src/action/{join,content,discover,composed}.rs` | 5 |
| `nmp-nip57/src/action.rs` | 1 (ZapAction) |
| `nmp-nip17/src/{action,dm_relay_list}.rs` | 2 |
| `nmp-nip65/src/lib.rs:200` | 1 (PublishRelayListAction) |
| `apps/chirp/nmp-app-chirp/src/ffi/actions.rs` | 3 (ChirpReact / ChirpFollow / ChirpUnfollow) |
| `apps/marmot/nmp-app-marmot/src/projection/action.rs:177` | 1 (MarmotActionModule) |
| `crates/fixture-todo-core/src/lib.rs:176` | 1 (TodoActionModule — the non-Nostr fixture) |
| `nmp-core` (built-in) | 2 (PublishModule, WalletPayInvoiceModule) |

The trait is touched by **6+ protocol crates, 2 app crates, and 1 deliberately-non-Nostr fixture**, all going through the same `ActionRegistry::start` → `execute` path (`crates/nmp-core/src/kernel/action_registry.rs:188,226`). The single-trait-per-namespace shape (ADR-0027) collapsed the previous dual-closure seam. The fact that a deliberately-non-Nostr `fixture-todo-core` implements it cleanly is the strongest evidence the abstraction is not Nostr-leaky.

**Verdict:** *Earning its keep, and it's the only abstraction in the codebase whose adoption count materially backs the framework thesis.* The doctrine D11 lint (publish goes through `dispatch_action`) is making this seam the canonical write path. Keep going.

## 3. Subscription lifecycle complexity — justified, but currently dual

The kernel does carry serious machinery: `InterestRegistry` (`crates/nmp-core/src/subs/registry.rs:45`), `LogicalInterest`, `CompiledPlan` (`crates/nmp-core/src/planner/plan.rs:202`), `SubscriptionLifecycle`, the M1 hand-rolled `req()` path, coverage hooks, oneshot/long-lived split, fairness scheduling. This is `BACKLOG.md` V-04: two subscription systems coexist on master, Stage 1 of the migration to a single `InterestRegistry` writer landed in PR #368, Stages 2-3 are open. That dual writer is a real D4 violation today.

But the Swift developer **never sees any of this**. Compare:

- `apps/notes/ios/Notes/Bridge/NotesBridge.swift:74` — opens a `[1]` kind filter through `nmp_app_register_raw_event_observer`, gets kind:1 notes back. One call site. Zero exposure to `LogicalInterest` / `CompiledPlan` / `CoverageGate`.
- `crates/nmp-core/src/ffi/mod.rs:1191` — `NmpApp::push_interest(LogicalInterest)` is the *only* surface that exposes a planner type, and it is for **other Rust protocol crates** (Marmot is the named caller in the doc).

The machinery is justified by the multi-relay, multi-role, outbox-aware reality of Nostr (NIP-65 routing, negentropy preferment, AUTH gates). What's currently expensive is **maintaining two of it**. V-04 is the right next debt to pay; the abstraction shape itself is sound.

**Verdict:** *Earning its keep at the boundary, but D4 demands V-04 Stage 2+3 ship before anything else expensive lands on top.*

## 4. The callback signal bus is your single most damning piece of evidence

This is where the code is shouting and the docs are whispering.

The "typed FFI" claim is structurally untrue today. Here's the layered evidence:

- `crates/nmp-core/src/ffi/mod.rs:1387` — `nmp_app_set_update_callback(app, context, callback: Option<UpdateCallback>)`. Where `UpdateCallback = extern "C" fn(*mut c_void, *const c_char)` (line 133). The signal bus is **C-string JSON**. Untyped. Stringly typed.
- `ios/Chirp/Chirp/Bridge/KernelBridge.swift` — **1,892 lines** of Swift. The `Generated/KernelTypes.generated.swift` is **232 lines** of Stage-1 codegen (7 flat record types: Metrics, ActionStages, etc.). The other ~1,700 lines are still **handwritten `Decodable` structs that mirror Rust types by hand**, plus the panic-frame substring scan (`KernelBridge.swift:623`) that consumes `"\"t\":\"panic\""` as untyped JSON.
- The supporting bridge surface is another **~2,300 LOC** (DmBridge 124, FollowListBridge 107, GroupChatBridge 256, GroupDiscoveryBridge 243, KernelModel 640, MarmotBridge 582, TimelineBlock 289).

Total Chirp `ios/Chirp/Chirp/Bridge/` surface: **4,206 LOC**, of which **232 LOC are generated** and the rest (~94%) is still hand-mirrored.

By contrast: `apps/notes/ios/Notes/Bridge/NotesBridge.swift` is **96 LOC** because it never decodes the typed `update_callback` payload — it consumes the raw-event observer JSON (a flat NIP-01 event) directly. Notes ships without paying any of the projection-decoder tax because Notes consumes *zero kernel projections*.

This is a real, present cost. F-05 has Stage-1 codegen landed (7 flat types) and a CI drift gate (`.github/workflows/codegen-drift.yml`), but the sweep through `KernelBridge.swift` to **delete the corresponding hand-mirrored code** has not happened. Every kernel field change is still a two-place edit. `TimelineBlock` and `KernelUpdate` are explicitly named in `BACKLOG.md` F-05 as the next targets and are still handwritten.

**Verdict:** *Not earning its keep.* The callback contract being a C-string JSON blob is fine — that is the right protocol for an FFI seam. What is not earning its keep is the **handwritten decoder layer on the Swift side**, because the framework promised machine-derived schemas and shipped a pilot but did not retire the duplicate. Until the sweep lands, "one source of truth, four delivery paths" is half-true.

## 5. Framework thesis — proven for trivial apps; the gradient to Chirp is the unanswered question

The evidence as it sits on master:

| App | Rust LOC | Swift Bridge LOC | New C-ABI symbols |
|---|---|---|---|
| `apps/notes` | 98 (lib.rs) + 25 (Cargo) ≈ 98 net | 96 (NotesBridge.swift) + 179 (Views/Models/App) = 275 total Swift | 1 marker (`nmp_app_notes_init`, empty body) |
| `apps/longform` | 30 + 215 (projection) + 241 (ffi) = 486 | (read-only; no equivalent measured surface in this audit) | Custom projection FFI |
| `ios/Chirp/Chirp/Bridge` | (Chirp's Rust app crate is 2,314 LOC) | 4,206 LOC | ~48 bespoke `nmp_app_*` |

The apps/notes wiring is `nmp_app_register_raw_event_observer` + `nmp_app_dispatch_action("nmp.publish", ...)` + `nmp_app_signin_nsec` + `nmp_app_signin_bunker` + `nmp_signer_broker_init` + `nmp_app_set_storage_path`. **Five generic substrate calls.** That is what a real framework looks like.

Chirp's wiring is fundamentally different: bespoke `nmp_app_*` symbols per verb, hand-mirrored `Decodable` types per projection, per-domain bridges (DmBridge, FollowListBridge, etc.). The 40× ratio is partly inherent to "social does more" (DMs, profiles, replies, follows, zaps, groups, wallets), but a meaningful slice is **historical debt from before `dispatch_action` and the codegen pilot existed**.

The honest test of the framework thesis is not "can you build Notes?" — that answer is yes. The honest test is **whether the gradient from Notes to Chirp is intrinsic to social complexity or accidental to incomplete migration**. The answer right now is "we don't know" because Chirp predates both the dispatch_action consolidation and the codegen sweep. Until F-05 finishes the codegen sweep AND the 48-symbol deprecation calendar burns down, the gradient remains unmeasured.

**Verdict:** *Thesis proven for the trivial case; unproven for any non-trivial case until Chirp is rebuilt on the post-debt seams.*

---

# Top 3 highest-leverage changes

These are ordered by leverage per LOC of work; each names files.

## A. Finish the F-05 codegen sweep — delete handwritten `TimelineBlock` and `KernelUpdate` decoders this sprint

The pilot has landed (Stage 1: `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift`, 232 LOC, 7 flat types). The drift gate is live (`.github/workflows/codegen-drift.yml`). What hasn't happened: the corresponding **deletions** from `ios/Chirp/Chirp/Bridge/KernelBridge.swift` (1,892 LOC) and `ios/Chirp/Chirp/Bridge/TimelineBlock.swift` (289 LOC).

Concrete next step:
1. Extend the codegen schema dump (`cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas`) to emit `TimelineBlock` + `KernelUpdate` + the projection-keyed snapshot envelope.
2. Regenerate `KernelTypes.generated.swift`.
3. Delete every duplicate `struct`/`enum` from `KernelBridge.swift` and `TimelineBlock.swift` whose source is now generated.
4. Add a `find` rule to the doctrine-lint suite: in `ios/Chirp/Chirp/Bridge/*.swift`, any `struct … : Decodable` that names a kernel projection key must live under `Generated/`.

This is the single highest-leverage change because it is the *only* one that converts the JSON-over-FFI contract from "honor system" to "single source of truth" — and BACKLOG.md F-05 already commits to it as v1-quality. Estimated payoff: ~1,000+ LOC deleted from Chirp's Swift bridge.

## B. Write the 48-symbol `nmp_app_*` deprecation calendar that `BACKLOG.md` admits doesn't exist

`docs/plan.md` TL;DR: *"Largest accumulated debt: 48 bespoke `nmp_app_*` FFI symbols in `crates/nmp-core/src/ffi/mod.rs` (1,487 LOC) competing with `dispatch_action`. … No deprecation calendar exists yet."*

That sentence has been in the plan for a sprint. Without a calendar, "one door per capability" is aspirational copy. Concrete next step:

1. Inventory the 48 symbols once (a shell `grep '#\[no_mangle\] pub extern "C" fn nmp_app_'` against `crates/nmp-core/src/ffi/`).
2. Classify each into one of three buckets: **(a)** content action — migrate to `dispatch_action`; **(b)** lifecycle / handle / control plane (`nmp_app_start`, `nmp_app_cancel_publish`, etc.) — keep, document as "Theme B" in `substrate/action.rs`; **(c)** capability (`nmp_app_dispatch_capability` plus its callback setter) — keep.
3. For bucket (a), add a per-quarter migration target row to `docs/BACKLOG.md` Section 4 (e.g. "Q1: N=8 verbs migrated; Q2: N=12; …"). v1 ships when the bucket is empty.
4. Update the D11 doctrine-lint to refuse any new bucket-(a) symbol added without an ADR override (the existing FFI-surface-freeze gate already checks count; it doesn't check classification).

Until this exists, every PR that adds `nmp_app_<verb>` is a small step backwards from the framework thesis and there is no failure signal.

## C. Either rename `MlsOpHandler` → `HostOpHandler` and prove with a non-MLS consumer, or write an ADR explaining the protocol-named generic seam

This was Opus #8's 30-day call and is unblocked. The current state on master:

- `crates/nmp-core/src/substrate/mls_op_handler.rs:80` — `pub trait MlsOpHandler`. The module doc itself notes (line 34): *"a future host could install its own MlsOpHandler without renaming the seam"*. That's the smell.
- `crates/nmp-core/src/actor/mod.rs:744` — `ActorCommand::DispatchMlsOp`. The arm reads the handler and forwards a JSON action body — there is nothing MLS-specific in the transport.
- `crates/nmp-core/src/ffi/mod.rs:929` — `NmpApp::set_mls_op_handler` accepts `Arc<dyn MlsOpHandler>`. D0 says *"the kernel never names the app noun"*. MLS is an app noun. This name violates D0 at file:line.

Concrete next step (cheapest option that pays the debt):
1. `s/MlsOpHandler/HostOpHandler/g` across `crates/nmp-core/src/substrate/mls_op_handler.rs`, `crates/nmp-core/src/actor/mod.rs`, `crates/nmp-core/src/ffi/mod.rs`. Rename the file to `host_op_handler.rs`. Rename `DispatchMlsOp` → `DispatchHostOp`. Mechanical, single PR.
2. **Then** prove it with a second consumer — `apps/notes` could be extended with a tiny stateful counter projection that uses `set_host_op_handler` for a non-MLS write, or a doc fixture in `crates/fixture-todo-core` could do the same.
3. If the rename is rejected, write an ADR explaining why the generic substrate seam is named after one specific protocol — i.e. document that future hosts will inherit "MLS" in the name.

Doing nothing leaves D0 violated at a named file:line in the kernel.

---

# Honest verdict — single sentence

NMP is closer to **"a single app (Chirp) with an optimistic interface"** than to a real reusable framework, **and the single load-bearing piece of evidence is `ios/Chirp/Chirp/Bridge/KernelBridge.swift` at 1,892 LOC of handwritten `Decodable` structs that the framework promised codegen would replace**.

The substrate is real (`dispatch_action`, `ActionModule`, the snapshot-projection registry, `register_raw_event_observer`, the typed slot pattern, the capability seam) and the Notes spike at 96 LOC of Swift bridge proves it can carry an app. What hasn't shipped is the **debt repayment**: the codegen sweep, the deprecation calendar, the D0 rename. Each is a finite-effort, name-the-files job. None is a research project.

## The single most important thing to do in the next 30 days

**Land F-05 Stage 3 (the codegen sweep through `KernelBridge.swift`) and merge the deprecation calendar PR.** In that order.

Stage 3 is what makes the framework's FFI contract machine-derivable in practice, not just in pilot. The calendar is what makes "one door per capability" a measurable commitment rather than a slogan. Together they collapse the cost-per-new-app curve from "rebuild a 4,200-LOC bridge" to "wire five substrate calls like Notes did," and *that* is the test the framework thesis has to pass before any of the remaining v1 work (M14 UniFFI, wasm Stage 3c, multi-app GA) is honest.

Notes proves the substrate can host trivial apps. Codegen-sweep + calendar prove the substrate's FFI is machine-derivable and bounded. Until both land, every direction review (including this one) will keep saying the same thing.

---

# Cross-references

- Built on: Opus #8 (`opus_direction_review_c_2026_05_23.md`) — HostOpHandler rename motivation.
- Built on: Opus #10 (`opus-direction-20260523-review-10-actor-godobj.md`) — `ActorCommand` closed-enum critique; **not re-litigated here** (still open).
- Built on: Opus #11 (`opus-direction-20260523-review-11-dx-after-spike.md`) — DX after Notes; `NmpAppBuilder` proposal complementary to this report's calendar.
- Source of truth files: `docs/plan.md`, `docs/BACKLOG.md` (V-04, F-01, F-05 entries).
- Codebase evidence files (verified at HEAD `76bc8547` family):
  - `crates/nmp-core/src/ffi/mod.rs` (1,559 LOC)
  - `crates/nmp-core/src/ffi/action.rs` (429 LOC)
  - `crates/nmp-core/src/ffi/capability.rs` (250 LOC)
  - `crates/nmp-core/src/kernel/action_registry.rs` (937 LOC)
  - `crates/nmp-core/src/actor/dispatch.rs` (1,477 LOC)
  - `crates/nmp-core/src/actor/mod.rs` (1,488 LOC; `ActorCommand` enum has 38+ variants spanning lines 203-744)
  - `crates/nmp-core/src/substrate/{action,mls_op_handler}.rs`
  - `apps/notes/nmp-app-notes/src/lib.rs` (98 LOC)
  - `apps/notes/ios/Notes/` (299 LOC total Swift)
  - `ios/Chirp/Chirp/Bridge/` (4,206 LOC total Swift; `KernelBridge.swift` 1,892)
  - `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift` (232 LOC generated)
