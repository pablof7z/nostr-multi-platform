# ADR-0053 — Host-declared projection subscriptions: the kernel serializes only the projections a host declares it consumes

- **Status:** Accepted (2026-06-13); amended by ADR-0070
- **Amends / partially supersedes:** **ADR-0039** (push projection seam is canonical).
  ADR-0039's Decision 1 (PUSH is the single canonical seam) and its rejection of a
  generic *pull* accessor stand unchanged. This ADR **corrects ADR-0039's reasoning
  that ALL consumer-side projection selection is a view-state leak** and adds the
  missing affordance: a host declares, once at init, the static set of projection
  keys it consumes, and the kernel emits only those.
- **Relates to:** ADR-0037 (typed FlatBuffers sidecar — the per-key encoding this
  change gates, not alters), ADR-0048 (`signer_state` generalisation — a Tier-1
  built-in, already self-gating), ADR-0042 (`author_view`/`thread_view` projection
  removal), the `SnapshotRegistry` change-gate mechanism (`ChangeGate`, a *separate*
  per-tick re-serialization optimization this composes with). Established
  consumer-side declaration precedents this generalises: relay `push_interest` /
  interest lattice, unified ref resolution (`resolve_ref` / `release_ref`), and
  dynamic observed-projection registration.
- **API naming update (#2089):** the muted-observer + replay open described by
  earlier feed registration language is now the
  `ObservedProjectionRegistrar::open_observed_projection` /
  `close_observed_projection` door. The public filterless accepted-event
  observer lane has been deleted and must not be used for host/product
  projection selection.
- **Scope:** WHICH projection keys are serialized into each pushed `SnapshotFrame`.
  NOT their content (owned by each projection's producer), NOT their decode path
  (owned by each shell), NOT per-tick change-diffing (a separate future optimization).

**Current disposition:** bounded pushed output remains the UI-state path. The
app-facing composition language changes: projection declarations and tiers are
executor machinery, not the normal production app API. Typed read sessions own
output demand, replay, status, and teardown for product reads.

## Context

The kernel pushes a full snapshot at up to 4 Hz (`Kernel::make_update`). Today, on
every emit it serializes **every kernel-owned built-in projection unconditionally**
and every host-registered projection that has been registered, then every host
decodes the frame. Concretely there are two tiers (code-grounded, 2026-06-13):

1. **Tier-1 — host-registered projections** (`SnapshotRegistry` closures, registered
   via `register_typed_snapshot_projection` /
   `register_feed_with_observer`): `wallet`, `bunker_handshake`, `nip46_onboarding`,
   `signer_state`, `nmp.feed.home`, the dynamic per-view feeds `nmp.feed.author.<pk>`
   / `nmp.feed.thread.<id>`, and the protocol-crate projections `nmp.nip29.*`,
   `nmp.nip17.*`, `nmp.nip57.*`, `nmp.marmot.*`. **These already self-gate by
   registration**: a key is emitted iff the host (or a protocol crate the host wired)
   registered it, and dynamic feeds are `remove()`d from the registry when the view
   closes. Registration *is* a declaration of consumption.

2. **Tier-2 — kernel built-in projections** (`KERNEL_BUILTIN_PROJECTION_KEYS`, 18
   keys inserted directly by `make_update` via
   `snapshot_projections_with_publish_cluster`): `publish_queue`, `publish_outbox`,
   `outbox_summary`, `configured_relays`, `relay_role_options`, `settings_hub`,
   `action_results`, `signed_events`, `action_stages`, `action_lifecycle`,
   `accounts`, `active_account`, `profile`, `relay_diagnostics`, `mention_profiles`,
   `claimed_profiles`, `claimed_events`, `resolved_profiles`. **These have no
   consumption gate** — they are serialized into every frame regardless of whether
   any screen in the host reads them.

The canonical waste is `relay_diagnostics`: a debug screen virtually no user opens,
whose snapshot pre-rolls every relay row + wire-subscription aggregate + relative-time
label, serialized (JSON *and* typed FlatBuffers sidecar) into every frame ~4×/sec
forever, and decoded by every host on every frame. It is a Tier-2 built-in, so no
host can opt out of paying for it.

### Why ADR-0039 banned consumer-side selection, and why that reasoning is wrong here

ADR-0039 and its Marmot-messages amendment rejected letting the host influence what
the kernel emits, on the grounds that it "leaks view state into the kernel" and
breaks one-way data flow (D1). The amendment's worked example is the
**"active group"** problem: projecting only the *currently-viewed* group's messages
would require the host to tell the kernel which group is on screen — a host→kernel
round-trip that changes as the user navigates, i.e. genuine dynamic view state in the
kernel. ADR-0039 correctly rejected that.

The error is **over-generalisation**. ADR-0039 refuted *dynamic, per-view, navigation-
coupled* selection (a real D1 hazard) and then banned *all* consumer-side selection,
including the **static, build-time, navigation-independent** set of projection keys
an app can consume. Those are different categories:

- **"Which group is the user looking at right now?"** — dynamic, changes on every
  navigation, must round-trip from host to kernel to be honoured → view-state leak,
  correctly out of scope (see Decision 4).
- **"Which projection *keys* can any screen in this app ever read?"** — a fixed
  property of the compiled app, known at build time, identical for the whole process
  lifetime → a **consumer interest set**, not view state.

The second is *exactly* isomorphic to consumer-interest seams the kernel **already
honours today**:

- **relay `push_interest` / the interest lattice** — the host declares a static set of
  relay filters; the kernel coalesces and serves only those.
- **unified ref resolution** (`resolve_ref(...)`) — a host component declares "I
  consume this profile or event ref"; the kernel owns the fetch policy and
  surfaces it under `refs.profile` / `refs.event`.
- **dynamic feed keys** (`register_feed_with_observer`) — a host declares "I consume a
  feed under key K".

Every one of these is a host telling the kernel **which resource it consumes**, with
Rust still owning all the data and all the policy. Declaring "I consume the projection
keys `profile`, `accounts`, `resolved_profiles`, …" is the **output-side sibling of
relay `push_interest`** — the same family, applied to the snapshot output surface. No
one calls a relay filter, a profile claim, or an event claim a view-state leak; the
projection declaration is no different.

The precise boundary (codex-corroborated): a host-declared projection set is
**refcounted consumer interest / resource ownership** ("consumer X needs projection
key K"), which is fine. It becomes the rejected anti-pattern **only** if the kernel
starts depending on *transient navigation facts* — "the user is on the group screen
right now". Static/declared interest = legitimate; the kernel tracking the active view
= the exact thing ADR-0039 was right to reject. Decision 4 holds that line.

The cost of the over-generalisation is permanent: every consumer pays serialize +
encode (+ the host pays decode/verify) for every Tier-2 built-in producer, 4×/sec,
for the life of the app, with no opt-out.

## Decision

1. **A host declares, once at app init, the static set of projection keys it
   consumes. The kernel serializes only Tier-2 built-in keys that are in that set.**
   The declared set is the **union of every projection any of the app's screens can
   consume**, known at app build time. It is fixed for the process lifetime — it does
   NOT change as the user navigates (Decision 4 keeps dynamic selection out).

2. **The seam.** The declaration lives **inside the existing `SnapshotRegistry`** —
   the `Arc<Mutex<SnapshotRegistry>>` slot already shared between the host
   (registration side) and the actor-thread kernel (`make_update` read side), and
   already preserved across `Reset`. No new actor parameter, no new shared slot, no
   new lifetime to manage. The API mirrors typed projection registration exactly:

   - **Rust / `AppHost` trait:** `fn declare_consumed_projections<I, K>(&self, keys: I)
     where I: IntoIterator<Item = K>, K: Into<String>;` — additive (unions into the
     declared set), `&self` (lock-and-extend), call before `nmp_app_start`.
   - **C-ABI:** `nmp_app_declare_consumed_projections(app, keys: *const *const c_char,
     len: usize)` — the host passes its static key array. Null `app` / null `keys` is
     a silent no-op (D6).
   - **JNI:** `Java_org_nmp_android_KernelBridge_nativeDeclareConsumedProjections`
     forwarding a `String[]` into the C-ABI shape.

   The declared set is the **single source of truth already present** in
   `nmp-codegen`'s `SNAPSHOT_PROJECTIONS` registry — the same list that generates each
   shell's typed decoders and that the producer-completeness gate cross-checks. The
   shells declare exactly that list (emitted as a generated constant), so the
   declaration cannot drift from what the shells actually decode.

3. **Gating semantics — Tier-2 built-ins only; Tier-1 is already self-gated.**
   - **Tier-2 built-ins** (`KERNEL_BUILTIN_PROJECTION_KEYS`): each key is inserted
     into the snapshot `projections` map (and its typed sidecar) **iff the declared
     set permits it** (see Decision 4 for the empty-set semantic).
     `snapshot_projections_with_publish_cluster` consults the declared set per key. The
     matching typed-sidecar builders (`builtin_typed_projections`) gate on the same
     set, in the same tick, from the same captured value (preserving the ADR-0037
     "JSON and typed cannot diverge" invariant — for the captured built-ins, the gate
     is applied at the capture site, so a skipped key sets its `captured_*` field to
     `None` and the typed path's existing `if let Some(..)` naturally omits it). The
     producer work for a gated-out key is **skipped entirely** — no
     `serde_json::to_value`, no FlatBuffers encode, no `relay_diagnostics_snapshot()`
     roll-up.
   - **Tier-1 host/protocol projections**: unchanged. A key is emitted iff registered.
     Registration is the declaration. The dynamic feeds (`nmp.feed.author.*` etc.)
     continue to be added on view-open and `remove()`d on view-close — that is the
     correct *lifecycle* gate for genuinely dynamic keys and is NOT what this ADR
     touches. (This is also why prefix/wildcard declaration is unnecessary: dynamic
     feeds are Tier-1, gated by registration, never Tier-2.)

4. **Projection-consumption intent is explicit and mandatory — a tri-state, NOT a
   silent empty=everything default (AMENDED by Workstream-E4, 2026-06-16).**

   > **History.** The original Decision 4 (2026-06-13) made an *empty* declared set
   > mean "no narrowing / emit everything," by analogy with the relay interest
   > lattice. In practice that silent default was a footgun: an app (or an internal
   > consumer) that simply *forgot* to declare got the full 4Hz firehose with no
   > signal, indistinguishable from a deliberate "I consume everything." Workstream-E4
   > retires the implicit path. There is now exactly **one** way to mean "everything"
   > and **one** way to narrow; "undeclared" is a loud bug, not a silent opinion.

   [`DeclaredProjections`](../../crates/nmp-core/src/kernel/snapshot_registry/declared.rs)
   is a tri-state:

   - **`Undeclared` (the forgotten-declaration footgun).** No intent was expressed.
     To stay behaviour-preserving in release it still `permits()` everything (so a
     production app never crashes and never goes dark, and a kernel tick under any
     not-yet-declared consumer still emits the full set rather than dropping it), but
     it is **loud**: `nmp_app_start` trips a `debug_assert!` (panic in dev/prod debug
     builds) and emits a non-fatal `tracing::warn!` in release. It is NOT a silent
     "emit everything" opinion. (The `debug_assert!` is compiled out under test
     harnesses — `cfg(test)` / the `test-support` feature — so the ~24 existing test
     call sites and the kernel unit tests need no per-site declaration; the kernel
     primitive's `Undeclared` default still permits everything, so their behaviour is
     unchanged. There is **no implicit `All` default in production**.)
   - **`Narrow(set)` (the narrowing path, via `declare_consumed_projections`).** Only
     the declared Tier-2 keys are emitted; every other Tier-2 built-in is skipped
     entirely (producer not run). An empty declaration is a no-op (it never produces
     an emit-nothing `Narrow(∅)`).
   - **`All` (the explicit firehose, via `consume_all_builtin_projections`).** The ONE
     non-footgun way to receive every Tier-2 built-in. Full clients (chirp-tui,
     chirp-desktop, the Chirp iOS/Android shells, the gallery) call it; the
     `nmp-defaults` builder typestate forces every app to choose `Narrow` or `All`
     before `start()` compiles.

   **Why this still delivers the headline win.** Both Chirp shells *do* consume
   `relay_diagnostics` (they ship diagnostics screens), so Chirp is a full client and
   uses `consume_all`. The apps that benefit from narrowing are the non-diagnostics
   consumers (podcast-player, hl, win-the-day): they declare their smaller set, which
   excludes `relay_diagnostics`, and the kernel stops serializing it for them. The
   acceptance criterion — "`relay_diagnostics` is no longer serialized unless
   declared" — holds for **every host that declares a `Narrow` set**.

   **No present-vs-absent ambiguity.** The host authored the declared set, so it knows
   exactly which Tier-2 keys it asked for. A declared key with no data this tick follows
   each projection's existing convention (`resolved_profiles` is `{}` when empty per
   D1; `action_results` is drain-on-emit, absent in steady state). Under incremental
   apply, omission of an enabled typed key means "unchanged, retain cache"; teardown is
   explicit through `state = Cleared`. A *gated-out* Tier-2 key is simply never produced
   because the host declared it unused. Dynamic Tier-1 feeds are different: they
   self-gate by registration and unregister on close.

5. **One-way data flow is preserved (D1).** The declared set is written **once,
   before the kernel emits its first real frame**, and never again. It is a property
   of the *consumer*, read by the kernel; the kernel never reflects it back to the
   host, and the host never re-derives it from a frame. There is therefore **no
   host→kernel→host feedback loop**: the value the kernel reads (the declared set) is
   not a function of any snapshot the kernel produced. Contrast the rejected "active
   group" design, where the kernel's output (the message list) would depend on a host
   signal (current group) that itself changes in response to rendering the kernel's
   prior output — a genuine cycle. The static declaration has no such dependency
   edge; it is identical in kind to the relay interest set the kernel already reads.

6. **Subsume the existing ad-hoc gates into one model.** Before this change there are
   three parallel "is this key present?" mechanisms:
   - **unconditional insert** (most Tier-2 built-ins),
   - **drain-on-emit / `Null → omit`** (`action_results`, `signed_events`,
     `action_stages`, `action_lifecycle` — present only on ticks where something
     settled), and
   - **D5 view-open gating** (historically `author_view` / `thread_view`; now removed
     per ADR-0042, but the convention of "skip insert when the view isn't open"
     remains documented).

   After this change the **declared-set gate is the outer gate** for Tier-2: a key is
   considered for emission only if declared; *within* a declared key the existing
   drain-on-emit / empty conventions are unchanged (they decide *content presence*,
   the declared set decides *consumer interest*). The two compose cleanly and
   orthogonally: declared-but-nothing-settled `action_results` is still absent this
   tick (drain returned `Null`); undeclared `action_results` is absent every tick (not
   in the set). There is no third path. The change-gate (`ChangeGate`) memo is a
   *third orthogonal axis* (skip re-serializing an unchanged declared key) and is
   untouched here.

## Consequences

- **`relay_diagnostics` is no longer serialized for any host that declares a
  non-empty consumed set unless that set includes it.** A host that ships a
  diagnostics screen declares `"relay_diagnostics"`; a host that declares a non-empty
  set excluding it pays zero serialize/encode/decode for it, ever. This is the
  headline win and the acceptance criterion for this work. (A host that declares
  nothing is `Undeclared` — the loud forgotten-wiring case (Decision 4, amended): it
  still receives everything in release so it never goes dark, but `nmp_app_start`
  warns + `debug_assert!`s. The optimization is opt-in by declaring a `Narrow` set;
  consuming everything is the explicit `consume_all`.)
- The Tier-2 built-in producers (`relay_diagnostics_snapshot()`, `accounts_enriched()`,
  `resolved_profiles()`, …) are only invoked for declared keys — the most expensive
  roll-ups become pay-for-what-you-use.
- **Correctness invariant intact.** Host state remains a pure function of the latest
  snapshot + monotonic `rev`. We change *which keys* appear in the frame, not the
  full-snapshot semantics and not the rev contract. No deltas, no fragile diffing.
- **Composed with adjacent work that has since landed** (orthogonal — this ADR governs
  *which* keys ship, not their content or decode):
  - relay_diagnostics raw-timestamps (relay_diagnostics ships raw timestamps):
    changed the *content* of a Tier-2 built-in; this ADR only decides *whether* that
    content is emitted. No overlap.
  - dropping the FlatBuffers verifier on trusted decode (Swift unchecked `getRoot`):
    changed the *decode* path; this ADR changes the *producer's emit* set. No overlap.
- **Composes with the change-gate (`ChangeGate`) and future per-key diffing.** The
  declared-set gate runs *before* the change-gate memo: an undeclared key is never
  serialized; a declared-but-unchanged key may still be served from the change-gate
  memo (Tier-1) or recomputed (Tier-2, until a per-built-in change gate is added).
  Per-projection change-gating for Tier-2 built-ins remains a separate future
  optimization and is explicitly NOT implemented here.
- **Orthogonal to the per-key revision/manifest transport redesign (in flight).** A
  separate effort is designing the *per-key revision transport* — the
  changed / unchanged / cleared contract for HOW MUCH of each projection key ships per
  frame (a per-key manifest in the snapshot envelope). This ADR is the **WHICH-keys**
  axis; that work is the **HOW-MUCH-of-each-key** axis. They compose by design: the
  declared set simply *scopes which keys participate* in that per-key manifest — an
  undeclared key is absent from the manifest entirely; a declared key participates in
  the changed/unchanged/cleared protocol. **This ADR deliberately does NOT restructure
  the `update_envelope` / `SnapshotFrame` manifest shape** — it gates emission at the
  producer (`make_update` / `snapshot_projections_with_publish_cluster` /
  `builtin_typed_projections`), leaving the envelope structure untouched so the
  manifest redesign can layer on without conflict. If the two efforts must touch the
  same envelope structure, the manifest redesign owns it and this gate scopes into it.
- **Drift protection.** A bidirectional gate in `nmp-app-chirp`'s
  `declared_projections` test module enforces that the declared set cannot drift from
  what Chirp actually decodes:
  - **Direction 1** (`every_chirp_declared_key_is_a_kernel_builtin`): every key in
    `CHIRP_CONSUMED_BUILTIN_PROJECTIONS` must exist in `KERNEL_BUILTIN_PROJECTION_KEYS`.
    A producer-side rename that ships without updating the declared list trips this at
    test time.
  - **Direction 2** (`every_codegen_decoded_builtin_is_declared`): every Tier-2 built-in
    key that has a codegen-generated Swift decoder in `SNAPSHOT_PROJECTIONS` must be in
    `CHIRP_CONSUMED_BUILTIN_PROJECTIONS`. A new Tier-2 built-in added to the codegen
    registry without updating the declared list trips this at test time — preventing the
    key from silently going dark once Chirp narrows.
  Together the two directions ensure the declared set equals the decoded set for the
  codegen-driven Tier-2 built-ins, matching the ADR's original "cannot drift by
  construction" intent. Keys decoded outside the codegen registry (e.g. Android-only
  JSON consumers like `mention_profiles`) remain covered by direction 1 alone.

## Out of scope (explicitly deferred — the real D1 hazard ADR-0039 worried about)

- **Dynamic, per-view subscription** ("only the currently-viewed group's messages",
  "only the active thread"). This requires the kernel to track *which view is active*,
  a host→kernel signal that changes on every navigation — a genuine view-state leak
  and a real feedback loop. It stays OUT of the kernel. Today's mechanism for
  navigation-coupled data is the **Tier-1 dynamic-feed registration lifecycle**
  (`register_feed_with_observer` on view-open, `unregister_feed` on view-close), which
  keeps the navigation decision entirely host-side. This ADR does not change that and
  does not open a door to it.
- **Per-projection change-diffing / granular deltas.** Separate optimization; the
  full-snapshot + rev contract (aim.md §2 invariant 10, D-snapshot) is preserved.

## Alternatives considered

- **Empty declared set = silent "emit everything".** Rejected. A forgotten
  declaration would be indistinguishable from a deliberate "consume everything"
  and would ship the full 4 Hz firehose with no signal. Current clients choose
  explicitly: `consume_all_builtin_projections` for all built-ins or
  `declare_consumed_projections` for a narrowed set.
- **Empty declared set = omit ALL Tier-2 built-ins (the task's literal default).**
  Also rejected: forcing `Undeclared` to emit *nothing* would make a forgotten
  declaration silently blank every screen (and break behaviour-preservation on
  master). The loud-but-permissive `Undeclared` above is strictly safer: it never goes
  dark, and the `debug_assert!` catches the omission in dev/test immediately.
- **Gate ALL projections (Tier-1 + Tier-2) on the declared set.** Rejected as
  redundant and risk-additive: Tier-1 already self-gates by registration, and the
  dynamic feeds need *lifecycle* gating (add/remove), not a static declared set.
  Forcing dynamic feeds through a static declaration would either require
  prefix/wildcard matching (`nmp.feed.author.*`) — reintroducing a dynamic concern
  into a static mechanism — or a redundant second gate. Dynamic typed feed teardown
  emits `Cleared`; the static declaration set never owns their lifecycle. Gating only
  the un-gated tier (Tier-2) is the minimal correct surface.
- **A new dedicated FFI slot for the declared set, separate from `SnapshotRegistry`.**
  Rejected: it would add a parallel `Arc<Mutex<…>>`, a new actor parameter, and a
  second Reset-survival contract for no benefit. The registry is already the shared,
  Reset-surviving, lock-once-per-tick object the kernel consults for projections.
- **Declare per-screen and union dynamically as screens mount.** Rejected: that is
  exactly the dynamic per-view coupling Decision 4 keeps out. The union is computed at
  build time (it is the codegen registry) and declared once.
- **Keep the status quo (everything always emitted).** Rejected: it is the documented
  problem — permanent, un-opt-out-able serialize/encode/decode waste, 4×/sec, forever.
