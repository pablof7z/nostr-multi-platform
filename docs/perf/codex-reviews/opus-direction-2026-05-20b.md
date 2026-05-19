# NMP Directional Review — 2026-05-20b

Reviewer: principal engineer, distributed systems. Audience: lead developer.
Verdict-first, opinionated, evidence-cited. This is a *direction* review, not a
bug list.

## Preface — what changed since `opus-direction-2026-05-20.md`

The earlier review is partly stale and partly wrong. I read the code; the
corrections matter because they change the priority ordering:

- **Stale (already fixed):** `ModuleRegistry` is deleted — `substrate/mod.rs:15`
  documents the removal honestly. NWC is feature-gated — `nmp-core/Cargo.toml:17`
  (`default = ["wallet"]`, `wallet = ["dep:nmp-nwc"]`). `SignerOp` already has a
  non-blocking `poll()` path — `nmp-signer-iface/src/op.rs:67`.
- **Wrong:** "collapse the thin NIP crates." They are not thin. `nmp-nip29` is
  2,892 LOC, `nmp-reactions` 2,332, `nmp-nip22` 1,358, `nmp-nip23` 1,342,
  `nmp-nip57` 1,012 — each with real tests. Crate count is not the problem.
- **Wrong:** "point codegen at Chirp." `nmp-codegen` generates **Rust** modules
  (`generate.rs:21-28` emits `lib.rs`, `action.rs`, `update.rs`, `ffi.rs` — all
  `.rs`). It does not and was never meant to generate Swift. Chirp's Swift FFI
  bindings are 100% hand-written (`KernelBridge.swift`, 579 LOC). Codegen and the
  C-ABI surface are unrelated problems.

The genuine load-bearing findings the earlier review got right and I re-confirm
with citations: the actor blocks on remote signing
(`actor/commands/identity.rs:230`), LMDB is still env-gated
(`kernel/mod.rs:382`), and D2 is not enforced in production
(`subs/lifecycle.rs:52`, `coverage_hook: None`).

The single most important *new* finding is in §3: the snapshot emitter
re-serializes full kernel state on every tick, with O(events) metric scans.
That is the architectural risk at scale and it outranks everything else.

---

## 1. What NMP does that it SHOULDN'T

### 1a. The five-trait `substrate` is a contract with no runtime — and it is now a *liability disguised as resolved*

`substrate/mod.rs:4-19` is admirably honest: the five traits (`ViewModule`,
`ActionModule`, `DomainModule`, `CapabilityModule`, `IdentityModule`) are
labelled "the **v2** extension design" and the doc says the dispatch runtime
"does not exist yet." Good. But the traits are still compiled, still `pub`,
still implemented by real types (`publish/action.rs:87` `impl ActionModule for
PublishModule`, `publish/view.rs:121` `impl ViewModule for PublishStatusView`).

The problem: there are now **two** extension stories in the tree —
`KernelEventObserver` (the one the kernel actually drives,
`kernel/event_observer.rs`) and the five traits (invoked only by static dispatch
in tests). A reader cannot tell from the code which is real. The module doc
*tells* them, but doc-as-disambiguation is a smell: when the code has two shapes
and only prose distinguishes them, the prose rots.

**Recommendation.** Do not keep "v2 design, static-dispatch-only" as a permanent
state. Either (a) delete `ActionModule` / `ViewModule` / `DomainModule` and keep
`PublishModule` / `PublishStatusView` as plain structs with inherent methods —
the trait buys nothing without a registry — or (b) put the v2 dispatch runtime on
a dated roadmap line with an owner. Drifting indefinitely is the worst option.
My call: **(a)**. `CapabilityModule` and `IdentityModule` may have a real
abstraction reason (multiple signer backends); audit those two individually. The
other three are premature interface extraction.

### 1b. `nmp-codegen` validates a strawman

`nmp-codegen` (~4 source files, `ffi_gen.rs` / `generate.rs` / `manifest.rs`)
generates Rust glue modules from a `nmp.toml` manifest. The `justfile` only ever
points it at `apps/fixture/nmp-app-fixture` (`justfile:7`). No real app consumes
generated modules — Chirp's `nmp-app-chirp` is hand-written Rust (1,445 LOC).

So codegen generates code for an app that exists only to be generated. That is a
closed loop with no external validator. It is not *wrong*, but it is unfinished
infrastructure being carried as if it were a feature. Until a real app consumes
generated modules, codegen is unproven and should be labelled experimental, not
counted toward v1.

**Recommendation.** Decide codegen's fate against the *real* migration target.
The `docs/research/highlighter/` survey says Highlighter ports onto NMP modules.
If codegen is meant to scaffold that port, prove it there. If not, freeze
codegen — do not keep polishing a generator with one synthetic consumer.

### 1c. The `apps/podcast/*` skeleton is dead weight in the workspace

`Cargo.toml` still lists six podcast members (`apps/podcast/nmp-app-podcast`,
`podcast-core`, `podcast-feeds`, `podcast-audio`, `podcast-rag`, `podcast-llm`).
`MEMORY.md` records podcast was explicitly killed ("NMP-only, 2 agents",
2026-05-18). Every workspace member is a `cargo build` edge, a `cargo test`
target, a dependency-resolution cost, and a thing a reader must mentally
exclude. `podcast-rag` and `podcast-llm` in particular have nothing to do with a
Nostr SDK.

**Recommendation.** Remove the podcast members from the workspace. If the code
has reuse value, move it to an out-of-workspace `attic/` directory or a branch.
A workspace should contain only what v1 ships or directly supports.

### 1d. Snapshot metrics that re-scan the event store every emit

`kernel/update.rs:44-52` computes, *on every snapshot*:
`self.events.values().filter(|e| e.kind == 1).count()`,
`...filter(|e| e.relay_count > 1).count()`, `self.events.len()`. These are
diagnostic counters. They are O(events) full scans, run inside the 60Hz emit
path. On a quiet timeline this is invisible; on a 50k-event store under firehose
load it is three full HashMap walks per emit. Diagnostic fields should be
incrementally maintained counters (bump on insert), not recomputed scans.

**Recommendation.** Convert `note_events`, `duplicate_events`, `stored_events`
to running counters updated at ingest. This is small and it removes a scaling
cliff from the hot path. See §3 for the bigger version of this problem.

---

## 2. What NMP should support that it DOESN'T (and that actually blocks shipping)

I am ruthless here. Only things that block real apps or force rework if deferred.

### 2a. A wired content-rendering pipeline — the crate is BUILT but not CONNECTED

This is the sharpest finding in the review. `nmp-content` is a complete,
well-architected Rust content tokenizer: `tokenize(content, tags, mode) ->
ContentTree`, FFI-stable wire types in `nmp-content/src/wire/`, a recursion
guard, an embed-claim registry. The crate doc (`nmp-content/src/lib.rs`) shows a
serious design — one entry point, one parser shape, explicitly avoiding the
NDKSwift three-overlapping-APIs anti-pattern.

And `nmp-core` does not use it. `grep -rln nmp_content crates/nmp-core/` returns
nothing. The kernel emits raw `content: String` and the shell parses it.

`ios/Chirp/Chirp/Components/NoteContentView.swift:131` runs the regex
`/nostr:[a-z0-9]+|https?:\/\/\S+|#[a-zA-Z]\w*/` over note content **in Swift**,
branches on `raw.hasPrefix("nostr:")`, classifies media by file extension, and
shortens bech32 keys for display. That is protocol-level content parsing in the
shell. The file comment even admits it: "Full resolution ... lives in the kernel
and will be wired when the `ContentTreeDto` projection is added."

So the doctrine ("Rust owns all logic, native renders") has *already lost* in
the one app meant to prove it — and the loss is not a missing feature, it is an
**unwired seam**. The Rust side is done. The Swift duplicate is inferior (no
NIP-30 emoji, no embeds, no nprofile vs npub distinction) and divergent (Android
will write a *third* tokenizer).

**Recommendation.** This is a one-week wiring job, not a feature build. Have the
kernel call `nmp_content::tokenize` when projecting timeline items, emit
`ContentTree` in the snapshot (the wire types exist), and reduce
`NoteContentView.swift` to a pure renderer over typed segments. Delete the Swift
tokenizer. Measure the Swift LOC delta — that delta is the doctrine scorecard.
**Do this before Android.** Every week it waits, the cost of a third tokenizer
gets locked in.

### 2b. Snapshot schema versioning

Snapshots cross the FFI as JSON with no version tag. `make_update`
(`kernel/update.rs`) builds `KernelUpdate` and `tick.rs:51` wraps it as
`{"t":"snapshot","v":...}` — the `"t"` discriminates frame *type*, not schema
*version*. Any field rename or removal silently desyncs an older shell. With
Android and desktop shells coming, multiple shells will pin different schema
expectations against one kernel.

**Recommendation.** Add a `schema_version: u32` to the snapshot envelope now,
while there is exactly one shell to migrate. Bump it on any breaking change; the
shell logs/degrades (D1) on mismatch instead of mis-decoding. Cheap now,
expensive after Android ships.

### 2c. Production persistence as the *default*, not an env var

`kernel/mod.rs:382`: production store selection is
`if let Ok(path) = std::env::var("NMP_LMDB_PATH") { LmdbEventStore::open(...) }
else { MemEventStore::new() }`. Default is in-memory. The factory at
`store/mod.rs:46` already takes a `StorageBackend` enum and constructs either
backend. The LMDB backend exists (`store/lmdb/`, ~1,400 LOC with insert/query).

This is not a missing feature — it is a missing *default and FFI plumbing*. A
real client that loses its entire timeline, follow list, and (critically) MLS
group state on every cold launch is not shippable. And shipping Marmot/MLS on an
in-memory store is data-loss-by-design.

**Recommendation.** Make LMDB the default. Plumb a storage path through
`nmp_app_new` (the shell already owns the app sandbox directory). Keep
`MemEventStore` for tests. This is plumbing, not research.

### 2d. Nothing automated crosses the FFI boundary

CI runs `cargo test` (Rust isolation) and `just stress-*` FFI stress harnesses
(`nmp-testing/ffi-stress` — good, those exercise the C ABI for crashes/leaks).
But nothing decodes a snapshot the way Swift does and asserts on its *shape*.
The header-drift gate (`ci/check-ffi-header-drift.sh`) checks the C signatures
match; it does not check the JSON contract. The architecture's central
promise — kernel emits JSON, shell decodes it — is validated only by a human
running Chirp.

**Recommendation.** One contract-test target: load the dylib, register an
observer, feed canned events, assert on decoded snapshot JSON (golden files).
This is days of work and it guards every future schema change. Pair it with 2b.

What is **not** missing and should not be added for v1: more NIP crates, Android
shell, web shell, multi-account atomic switch, the v2 substrate runtime. None of
those block a credible single-platform v1.

---

## 3. The single-actor TEA model — is it right?

**The actor model is right. The snapshot model is the problem.**

### The actor spine is correct — keep it

Single-writer (D4) on a dedicated thread, TEA-style command-in/snapshot-out, is
the correct architecture for a Nostr client and you should not second-guess it.
The dual-channel design in `actor/mod.rs:404-501` is genuinely good: commands
drain via `try_recv` in a priority loop so a relay-event flood can never starve
`CreateAccount`. The `changed_since_emit()` idle gate (`tick.rs:23,39`) is the
discipline most Nostr clients lack — it means a quiet app emits nothing. Keep
all of this.

### Failure mode 1 — the actor blocks on remote signing (spine-level defect)

`actor/commands/identity.rs:223-237`, `sign_active`: for a remote signer it
calls `handle.sign(unsigned).wait(REMOTE_SIGN_TIMEOUT)` — a blocking
`recv_timeout` of 5 seconds on the actor thread. The doc comment is candid:
"blocks the actor thread for up to ... 5s ... a non-blocking `SignerOp::poll`
path is the follow-up."

A single-writer actor that blocks is a single-writer actor that freezes
*everything* — relay ingest, subscription recompiles, UI snapshots — for up to
5s on every bunker sign. This is not a latency nuisance; it is a correctness
defect in the spine. And the fix is *already half-built*: `SignerOp::Pending(rx)`
(`op.rs:29`) carries a receiver, and `poll()` (`op.rs:67`) is non-blocking and
disconnect-safe. The actor just calls `wait()` instead of `poll()`.

**Recommendation.** Model an in-flight sign as actor state. On a `PublishNote`
that needs a remote signer: issue `sign()`, stash the `SignerOp::Pending(rx)` in
an actor-local map keyed by a request id, return immediately. Poll pending
signer ops in the idle-tick the same way relay channels are polled. When the
result arrives, resume the publish pipeline. Treat a bunker response as just
another async fact arriving on the actor — identical in shape to a relay event.
This is medium effort and it is a v1 blocker.

### Failure mode 2 — full-state re-serialization on every emit (the scaling cliff)

`make_update` (`kernel/update.rs:5`) constructs the **entire** `KernelUpdate`
fresh on every emit: full `visible_items()`, `profile_card()`, `author_view()`,
`thread_view()`, `relay_status()`, `relay_statuses()`, plus the O(events) metric
scans flagged in §1d. Then `serde_json` serializes the whole struct.

It *does* compute `inserted/updated/removed` diffs (`update.rs:20`,
`diff_items`) — but it still ships the full `items` array **and** the three diff
arrays in the same payload. So a diff is computed and then thrown into a payload
that also carries the full state. The shell gets both. That is the worst of both
worlds: the cost of diffing plus the bandwidth of a full snapshot.

D5 says snapshots are bounded — and `visible_items()` is bounded by
`visible_limit`, so the *items* array is capped. But `relay_statuses()` grows
with relay count, `metrics` re-scans the unbounded event store, and the whole
thing is JSON-encoded at up to 60Hz. On mobile this is the battery/CPU question
the task asks about, and the answer today is: **the emit cost grows with store
size even when the visible window does not.**

**Is 60Hz the right abstraction?** 60Hz is a *ceiling*, not a *rate* — the
`changed_since_emit` gate means an idle app emits 0Hz. That part is right. What
is wrong is that *when* it emits, it pays full-snapshot cost. The right model is
either (a) emit diffs only (the `inserted/updated/removed` arrays already exist —
make them authoritative, drop the full `items` array from steady-state frames,
send full only on a resync request), or (b) if full snapshots are kept for
simplicity, make the emit cost O(visible window) strictly — no event-store scans,
incremental metric counters.

**Recommendation.** Pick (a). The diff infrastructure is already built; finish
it by making diffs the steady-state payload and full snapshots an explicit
resync. Combine with the §2b `schema_version` and a `is_full: bool` flag. This
is the highest-value architectural change in the review because it is the one
thing that gets *worse* the more successful the app is (more follows, more
events, larger store).

### Failure mode 3 — relay fan-in is fine; storage write-amplification is the open question

Many relays funnelling into one `relay_rx` is *not* a concern — the actor's
per-iteration work is bounded and the channel is unbounded mpsc; the actor is a
deduplication point, which is exactly what you want. The real scaling question is
the **store write path** under firehose: every accepted event hits
`EventStore::insert`. With LMDB as the default (§2c), that is a disk write per
event on the actor thread. Negentropy (D2) is the mitigation — it is supposed to
prevent re-fetching events you already have — which is precisely why D2 not being
wired (§5) compounds this.

**Verdict on §3:** the actor model is right, keep it. The two things to fix are
the blocking signer (spine defect) and the full-snapshot emit (scaling cliff).
Neither requires abandoning TEA; both are within the model.

---

## 4. The FFI surface — is hand-maintained C headers the right call?

**For v1: yes, keep the hand-written header. For v2: UniFFI, but not naively.**

The FFI surface is ~2,239 LOC across `nmp-core/src/ffi/` and is disciplined: D6
is enforced (`catch_unwind`, null-degrades-silently), `ffi-surface.md` documents
every symbol with its error posture. The header-drift CI gate
(`ci/check-ffi-header-drift.sh`) closes the most dangerous failure mode — the
header is a hand-maintained superset across three static archives and the gate
scans all three FFI roots so the C signatures cannot silently drift from Rust.

So the *immediate* risk (silent signature drift) is already mitigated. The
*residual* risk the gate does not cover is the **JSON contract** — the header
proves `nmp_app_dispatch` exists with the right signature; it proves nothing
about the shape of the JSON that flows through it. That gap is §2d's
contract-test, not a header problem.

UniFFI is on the roadmap as M14 (`docs/plan.md:49`) and the bible layout assumes
it (`aim.md:185,203` — generated Swift/Kotlin checked in). The honest reason
UniFFI has not happened: **UniFFI does not cleanly model the patterns NMP
actually uses.** `aim.md:242` says it directly — "UniFFI gives callback
interfaces, not native reactive streams." NMP's whole reactivity model is the
observer/callback channel: `nmp_app_register_event_observer` takes a
`*mut c_void` context + a raw function pointer (`actor/mod.rs:52-55,67-70`). That
is the part of a C ABI UniFFI is *worst* at. A naive UniFFI migration would force
the reactive snapshot stream into UniFFI's callback-interface shape and you would
fight the generator.

**The right migration path and trigger:**

- **Trigger to start UniFFI:** the second shell. As long as Chirp is the only
  consumer, hand-written bindings + the drift gate are *cheaper* than a
  generator. The moment Android needs the same surface, hand-maintaining two sets
  of bindings (Swift + Kotlin) against one C header is where UniFFI pays for
  itself. Do not migrate before Android; do not ship Android with a second
  hand-written binding set.
- **Scope it correctly:** UniFFI for the *request/response* surface (commands,
  capability dispatch, the value types) where it shines. For the *reactive
  snapshot stream*, keep an explicit, hand-designed `Subscription` handle +
  callback — `aim.md:242` already identifies this as needing a bespoke
  cross-platform reactive protocol. Do not let UniFFI dictate the streaming
  shape.
- **Before any of that:** add `schema_version` (§2b) and the contract-test
  (§2d). UniFFI generates the marshalling; it does not generate a *contract
  guarantee*. You want the contract test in place before you swap the
  marshalling layer, so a UniFFI migration is verifiable.

**Recommendation.** Keep hand-written headers for v1. Write a one-paragraph ADR
recording the trigger ("UniFFI migration begins when the Android shell starts;
reactive stream stays bespoke") so this is a decision, not a drift. Do not treat
M14 as inevitable-but-undated.

---

## 5. What's the realistic v1 scope?

The mandate is "absolute reliability is our north star." That mandate *defines*
v1 by exclusion: v1 is the smallest surface that cannot lose data, cannot freeze,
and cannot silently mis-sync. Apply that test literally.

### v1 definition — one platform (iOS/Chirp), and these properties hold:

1. **No cold-launch data loss.** LMDB is the default store, path plumbed through
   FFI (§2c). Non-negotiable — "reliability" with an in-memory store is a
   contradiction.
2. **The actor never freezes.** Remote signing is non-blocking (§3 failure
   mode 1). A crashed bunker degrades to a toast, never a 5s app-wide freeze.
3. **Snapshots are versioned and contract-tested** (§2b + §2d). A schema change
   cannot silently desync a shell, and CI catches it.
4. **D2 is actually enforced.** Right now `PlanCoverageHook` defaults to `None`
   (`subs/lifecycle.rs:52`) — `CompiledPlan` permits a REQ with no negentropy
   gate. A doctrine the planner does not enforce is a comment. Either wire the
   coverage hook into production kernel startup (the seam and its tests exist —
   `subs/coverage_hook_tests.rs`) or make an un-gated REQ unrepresentable in the
   `CompiledPlan` type. Without this, every subscription re-downloads history on
   every launch — a reliability *and* a battery defect.
5. **Content rendering comes from the kernel** (§2a). Wire `nmp-content`; delete
   the Swift tokenizer. This is the doctrine proof; if v1 ships with Swift
   parsing `nostr:` URIs, the framework's central claim is unproven on day one.
6. **The emit path does not scale with store size** (§3 failure mode 2). At
   minimum, remove the O(events) metric scans from `make_update`; ideally make
   diffs the steady-state payload.

### Cut from v1 — explicitly defer:

- **Marmot / MLS encrypted groups.** `nmp-marmot` is 3,685 LOC and real, but
  shipping encrypted *group state* on top of a store that is not yet the
  hardened default (item 1) is a priority inversion — MLS ratchet state loss is
  unrecoverable. Marmot ships *after* LMDB is the proven default. This is the
  single most important cut.
- **NWC / wallet.** Already feature-gated — keep it off for the v1 build. It is
  not on the reliability-critical path.
- **Android and desktop shells.** v1 is one platform done right. A second shell
  before the snapshot contract is versioned and tested just doubles the
  un-guarded surface.
- **The v2 substrate runtime and `nmp-codegen`-for-real-apps.** Experimental;
  not counted toward v1 (§1a, §1b).
- **`apps/podcast/*`.** Remove from the workspace entirely (§1c).

### The honest framing

NMP today is a strong kernel idea with the *foundation unpoured*: the production
store is in-memory, the actor can freeze, the central JSON contract is
human-validated, and two doctrines (D0-clean content rendering, D2 negentropy)
are aspirational rather than enforced. None of the v1 list above is a research
problem. Every item is *wiring something that already exists* — the LMDB
backend, the `SignerOp::poll` path, the `nmp-content` tokenizer, the
`PlanCoverageHook` seam, the `inserted/updated/removed` diff arrays. That is the
good news: v1 is an integration sprint, not an invention sprint.

---

## Top 3 highest-leverage changes for one sprint

Ranked by leverage = (reliability impact) × (rework-cost-if-deferred) ÷ effort.
These three are the leverage maximum, not the whole of v1 — every item in the
§5 v1 list is still v1; this is the subset to attack first if a sprint is all
you have.

### 1. Wire LMDB as the default store + plumb the path through FFI

**Why #1:** Every other feature writes data. An in-memory default means
guaranteed cold-launch data loss, and it makes Marmot/MLS unsafe to ship at all.
This is the unpoured foundation — nothing else about "reliability" is true until
this is. **Effort:** medium. The backend exists (`store/lmdb/`), the factory
exists (`store/mod.rs:46`); the work is flipping the default at
`kernel/mod.rs:382` and threading a path argument through `nmp_app_new`.

### 2. Make the actor non-blocking for remote signing

**Why #2:** A single-writer actor that blocks 5s on a bunker round-trip freezes
the entire app — relay ingest, subscriptions, UI. It is a spine-level
correctness defect, not a latency tweak. The fix is already half-built:
`SignerOp::Pending(rx)` and `poll()` exist (`op.rs:29,67`); the actor just calls
`wait()` (`identity.rs:230`). **Effort:** medium. Stash pending `SignerOp`s in an
actor-local map, poll them in the idle tick, resume the publish pipeline on
completion — the same pattern already used for relay channels.

### 3. Wire `nmp-content` into the kernel; emit `ContentTree`; delete the Swift tokenizer

**Why #3:** This is the cheapest possible proof of the framework's central
doctrine, and the cost of *not* doing it compounds — every week it waits, a third
content tokenizer (Android) gets closer to being written. The Rust crate is
complete and FFI-stable; the only missing piece is the snapshot seam. **Effort:**
low-to-medium — call `nmp_content::tokenize` in the timeline projection, add
`ContentTree` to the snapshot, reduce `NoteContentView.swift` to a renderer.

Honourable mention (do it the same sprint if capacity allows): add
`schema_version` to the snapshot envelope (§2b) — it is a few lines and it is
*much* cheaper before Android than after.

Items 1–2 make v1 *possible*. Item 3 makes v1 *honest*. Everything else in this
review is downstream of those three.
