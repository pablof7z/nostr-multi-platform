---
type: research-record
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 6376-6455
agent_attribution: Post-hoc review merged Decision-2 (a1ffdfeca42f52771)
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Post-hoc architectural review of merged ADR-0053 (host-declared projections): verdict HAS-DEBT-FIX-FORWARD — empty=everything footgun, missing drift-protection gate, unenforced init-only invariant"
captured_at: 2026-06-13T21:44:49Z
---

Post-hoc architectural review of merged ADR-0053 (host-declared projections): verdict HAS-DEBT-FIX-FORWARD — empty=everything footgun, missing drift-protection gate, unenforced init-only invariant

---

User: [Agent task result: Agent "Post-hoc review merged Decision-2" completed]
All tests green. I have a complete picture. Let me write the verdict.

---

# POST-HOC REVIEW: ADR-0053 "Host-declared projection subscriptions" (PR #1339, merged f97eb5a3c)

## VERDICT: **HAS-DEBT-FIX-FORWARD**

The mechanism is well-engineered, thread-safe, Reset-correct, and the JSON/typed parity is genuinely preserved. The seam itself is clean. But against the owner's bar ("hard for an app dev to fuck up," "no comfort defaults," "all callers updated not a compat default," "the architecturally right change not the smallest"), there are **three real debts** — one of which means the headline optimization *delivers literally zero benefit to the only app in the tree*, and one of which is a documented gate that **does not exist**.

---

## Item 1 (THE FOOTGUN): empty=everything is DEBT — verdict DEBT, fix-forward to option (c)+(d)

The default is `empty ⇒ permits everything` (`declared.rs:73-75`). The ADR defends this at length (Decision 4, Alternatives) by analogy to the relay interest lattice. **The analogy is real but the conclusion is wrong for this surface**, for two concrete reasons the ADR's prose elides:

1. **Failure mode asymmetry.** An empty relay filter producing "everything" is observable — you see traffic, screens populate. An app that forgets `declare_consumed_projections` gets the **full 4Hz firehose silently** — identical correct behavior, just the exact perf waste this ADR exists to kill, with **no error, no warning, no log, nothing** (`kernel_access.rs:98-106` degrades silently; `snapshot.rs:203` no-ops on bad args silently). The waste is *invisible by construction*. That is the definition of a footgun under this repo's bar.

2. **The ADR's own escape hatch is unbuilt.** Decision 4 explicitly says: *"If a future milestone wants to require declaration, a `debug_assert` / doctrine-lint that a production host's declared set is non-empty can be added on top."* That "on top" guard is **not in the merged code** — there is no `debug_assert`, no lint, no one-time `tracing::warn!`. So the ADR shipped the comfort default and deferred the safety rail to a "future milestone" that has no issue, no owner, no gate. That is textbook debt: the convenient half landed, the discipline half is a promise.

**This is not hand-waving — here is the proof it bit immediately:** the *only* app in the repo, Chirp, declares **all 18** built-ins (`declared_projections.rs:29-56`; I diffed it against `KERNEL_BUILTIN_PROJECTION_KEYS` — the sets are identical, zero keys excluded). So for Chirp, `is_narrowing()` is true but `permits()` returns true for every key — **Chirp narrows nothing and `relay_diagnostics` still ships 4×/sec.** The headline acceptance criterion ("relay_diagnostics no longer serialized") is **not met by any shipping consumer**; it's only met by hypothetical external apps (gallery, hl, podcast-player) that *don't exist in this tree and have not been updated to call the seam* (grep confirms zero `declare_consumed_projections` callers outside Chirp + nmp-defaults pass-through). The win is entirely theoretical today.

**Fix-forward (new PR):**
- **Adopt (c) loud diagnostic + (d) builder enforcement.** Add a one-time `tracing::warn!` the first time `make_update` emits with an empty declared set in a `NmpApp` that came through the `nmp-defaults` builder (the production composition root) — "host declared no consumed projections; emitting all N Tier-2 built-ins every tick (perf waste). Call declare_consumed_projections() at init." This makes the waste discoverable without breaking the empty=everything semantic for the kernel's own Rust consumers (chirp-tui/desktop/tests legitimately want no narrowing).
- **Keep empty=everything as the *kernel primitive* semantic** (it is correct there — test helpers and embedded Rust consumers genuinely have "no opinion"), but make declaration **mandatory at the `nmp-defaults` builder layer** (the app-dev-facing surface): `NmpDefaults`/builder should require a declared set (type-state or a `debug_assert!(!keys.is_empty())` at build) so an *app* cannot silently forget. The distinction the ADR itself draws (kernel primitive vs app) is exactly the layer at which to split the default: permissive primitive, strict builder.

The owner's "always right, never smallest" rule applies directly: the right change is to enforce at the builder, not to ship a permissive default and call the safety rail future work.

---

## Item 2 (DEBT): the "Drift protection" gate the ADR promises does NOT exist

ADR Consequences §"Drift protection" (lines 266-271): *"We extend the model so the shells' declared set is generated from the same registry — a shell decoding a key it never declares (or declaring a key it never decodes) is caught by an extended gate."*

**No such extended gate was built.** What exists:
- `apps/chirp/.../producer_completeness/registry_coverage.rs:176-191` — the **pre-existing** `every_codegen_registry_key_is_registered_at_runtime` gate. It checks every codegen `SNAPSHOT_PROJECTIONS` key has a runtime *producer*. It says nothing about the *declared* set. It predates this ADR.
- `declared_projections.rs:88` `every_chirp_declared_key_is_a_kernel_builtin` — pins declared ⊆ builtins. **One direction only.** It does NOT check declared ⊇ what-shells-decode. A Chirp screen that decodes `outbox_summary` while the declared list omits it would compile, pass all tests, and **silently go dark** — the precise "#1084-class hole" the ADR claims to close. The declared set is hand-maintained (`declared_projections.rs:29`), explicitly contradicting the ADR's "generated from the same registry / cannot drift."

So the ADR's central drift-safety claim is **aspirational text describing unbuilt work**, presented in the "Consequences" (i.e. "this is true now") section rather than "Out of scope." Under this repo's zero-debt / single-source-of-truth bar, a merged ADR asserting a gate that doesn't exist is itself the debt.

**Fix-forward (new PR):** Build the gate the ADR describes. Add a test (in `nmp-app-chirp` or `nmp-codegen`) asserting `CHIRP_CONSUMED_BUILTIN_PROJECTIONS == { every Tier-2 builtin json_key in SNAPSHOT_PROJECTIONS that has a generated Chirp decoder }` — i.e. declared set equals the decoded set, both directions. Better still, *generate* `CHIRP_CONSUMED_BUILTIN_PROJECTIONS` from the codegen registry (filtered to Tier-2) so it cannot drift at all, which is what the ADR actually claimed. Until then, the hand-kept list (currently = all 18) is a latent dark-screen hazard exactly when someone *does* narrow it.

---

## Item 3 (MINOR DEBT): declaration is silently mutable mid-session; no init-only guard

ADR Decision 5 leans hard on "written once, before the first real frame" to argue D1 one-way-flow is preserved. But the code does **not enforce init-only**:
- `declare_consumed_projections` is plain additive union (`snapshot_registry.rs:405-411`), callable anytime.
- `nmp_app_declare_consumed_projections` (`snapshot.rs:195`) checks null but **not** the `started: AtomicBool` flag that `NmpApp` already carries (`lib.rs:607`) — grep confirms no `started` check in `snapshot.rs`/`app_host_impl.rs`.

Because the set is **additive-only** (no removal API), a mid-session call can only *widen* the emitted set, never narrow it — so it cannot cause a screen to go dark mid-flight, and it cannot leak transient view-state *into a narrowing* (the D1 hazard ADR-0039 feared). So this is **not currently a correctness bug**. But the D1 argument in Decision 5 rests on an invariant the type system doesn't hold; the moment anyone adds an `undeclare`/removal API (a plausible future), mid-session mutation becomes a real view-state-leak vector, and there's no guard standing in the way.

**Fix-forward (low priority, fold into the Item 1 PR):** Either (a) `debug_assert!(!self.started.load(Acquire))` in `nmp_app_declare_consumed_projections` to pin the "init-only" invariant the ADR asserts, or (b) explicitly document in the ADR that the invariant is *enforced by additive-only semantics* rather than by call timing, and that any future removal API must add the started-guard. Right now the ADR claims an invariant the code leaves to convention.

---

## What is SOUND (verified, no action)

- **Item 3 thread-safety / Reset survival: CORRECT.** The declared set lives inside the `Arc<Mutex<SnapshotRegistry>>` slot. Reset preserves it via `take_snapshot_projection_handle_for_reset` → `set_snapshot_projection_handle` (`dispatch.rs:1353,1401-1403`); the slot Arc outlives the discarded kernel, so the declared set correctly **survives account-switch/Reset** (matching the event-observer slot contract). It is read once per tick under the single lock already taken (`kernel_access.rs:98-106`), cloned to avoid per-key re-locking (`projections.rs:96`). Poisoned-lock / unbound-slot degrades to empty=permit-everything (D6, fail-open not fail-dark) — correct, never panics at the boundary. Declaring a non-existent key is harmless (it just never matches a builtin). Declaring is idempotent (BTreeSet union). All sound.

- **Item 2 (composition with incremental per-key emission): CLEAN, NO CONFLICT.** The gate runs at the *producer* (`snapshot_projections_with_publish_cluster` / `builtin_typed_projections`), and the ADR deliberately left `update_envelope`/`SnapshotFrame` untouched (lines 254-265). The declared set narrows WHICH keys; the planned rev/manifest work narrows churn WITHIN keys. They are orthogonal axes and the envelope shape is unmolested, so the manifest redesign can layer on without conflict. This is genuinely well-sequenced — the one place the ADR's "composes cleanly" claim fully holds.

- **JSON/typed parity under gating: CORRECT and carefully done.** Two-layer defense: capture-site gating sets `captured_*` to `None` for gated-out drain/diagnostics keys (`projections.rs:163,213,289-292`) so the typed sidecar's `if let Some` naturally omits them; plus a final `out.retain(permits)` belt-and-suspenders on the live-accessor clusters (`typed_projections/mod.rs:385-388`). `declared_set_gates_typed_sidecar_in_lockstep_with_json` proves it. The ADR-0037 divergence invariant is preserved through the gate.

- **Item 4 (Tier-1 vs Tier-2 split): JUSTIFIED, not debt.** Tier-1 genuinely self-gates by registration (`register*` is the declaration; dynamic feeds `remove()` on close) — verified by `tier1_host_projection_is_not_gated_by_declared_set`. The two gating mechanisms aren't "two ways to do one thing"; they're lifecycle-gating (dynamic, add/remove) vs static-set-gating (build-time union). Forcing dynamic feeds through a static declaration would need prefix/wildcard matching — strictly worse. The split is correct.

- **Item 5 (thin-shell): CONFIRMED CLEAN.** iOS `KernelBridge.swift:56` and Android `lib.rs:62` each make exactly one static call (`nmp_app_chirp_declare_consumed_projections`), zero logic, the key list lives Rust-side in `nmp-app-chirp` (`declared_projections.rs:29`). `chirp_declares_a_non_trivial_set` guards against collapse. No LOC concerns, no compat aliases.

- **Item 6 (forbidden compat accommodation): the empty=everything default IS partly this**, and the ADR is candid about it (Alternatives, lines 288-300: omit-all "would have ~24 test call sites declare a set in the same change or lose all built-ins"). That is the "avoid updating callers" motive the owner forbids — *for the test/embedded-Rust callers*. The mitigation in Item 1 (permissive kernel primitive + strict builder) resolves this honestly: the kernel-internal callers keep the permissive primitive (legitimate "no opinion"), while the app-facing builder is made strict (callers that are *apps* get updated/enforced). That threads the owner's rule correctly rather than blanket-permissive.

---

## Recommended fix-forward PRs (priority order)

1. **P1 — Close the loop the ADR opened.** Make declaration enforced at the `nmp-defaults` builder (app-facing) + add the one-time empty-set `warn!` in `make_update`. This is the difference between "merged a mechanism" and "delivered the optimization." Today no shipping app benefits.
2. **P1 — Build the drift gate the ADR claims exists.** Generate (or bidirectionally pin) `CHIRP_CONSUMED_BUILTIN_PROJECTIONS` from the codegen registry. Until then, narrowing the Chirp list risks silent dark screens.
3. **P3 — Pin the init-only invariant** with a `started`-guard `debug_assert` (or amend the ADR to state it's held by additive-only semantics, and require future removal APIs to add the guard).

Scoped tests (`cargo test -p nmp-core declared`) are **green** (10/10) — the merged code does what its tests assert; the debt is in what the tests/ADR *don't* enforce, not in a broken implementation.
